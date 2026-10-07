use super::*;
use crate::{engine::KeyEvent, keys};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread,
};

fn key(code: u16, value: i32) -> DeviceEvent {
    DeviceEvent::Key(KeyEvent {
        device_id: 1,
        code,
        value,
    })
}

fn message(generation: u64, event: DeviceEvent) -> Message {
    Message {
        generation,
        event,
        at: SystemTime::now(),
    }
}

struct TestOutput {
    events: Arc<Mutex<Vec<(u16, i32)>>>,
    tx: SyncSender<Message>,
    /// Ввод пользователя, приходящий после первого синтетического нажатия.
    during_fix: Vec<DeviceEvent>,
    fail_after: Option<usize>,
    stopped: Arc<AtomicBool>,
}
impl KeyOutput for TestOutput {
    fn emit_key(&mut self, code: u16, value: i32) -> io::Result<()> {
        let mut events = self.events.lock().unwrap();
        events.push((code, value));
        let count = events.len();
        if count == 1 {
            for event in self.during_fix.drain(..) {
                self.tx.send(message(0, event)).unwrap();
            }
        }
        if self.fail_after == Some(count) {
            return Err(io::Error::other("simulated write failure"));
        }
        if count == 8 {
            self.stopped.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// Полная коррекция слова `a` (скан-код 30) по Insert.
const CORRECTION: [(u16, i32); 8] = [
    (14, 1),
    (14, 0),
    (125, 1),
    (57, 1),
    (57, 0),
    (125, 0),
    (30, 1),
    (30, 0),
];

#[test]
fn event_loop_cancels_replay_and_does_not_reuse_aborted_buffer() {
    let during_fix = vec![
        DeviceEvent::Click,
        key(keys::KEY_INSERT, 1),
        key(keys::KEY_INSERT, 0),
    ];
    let (events, result) = run_test(during_fix, None);
    assert!(result.is_ok());
    assert_eq!(events, vec![(14, 1), (14, 0), (110, 1), (110, 0)]);
}

#[test]
fn event_loop_exits_on_write_failure_after_releasing_key() {
    let (events, result) = run_test(Vec::new(), Some(1));
    assert!(result.is_err());
    assert_eq!(events, vec![(14, 1), (14, 0)]);
}

#[test]
fn event_loop_completes_exact_correction() {
    let (events, result) = run_test(Vec::new(), None);
    assert!(result.is_ok());
    assert_eq!(events, CORRECTION);
}

#[test]
fn test_event_loop_typing_during_fix_replayed_after_it() {
    let (events, result) = run_test(vec![key(48, 1), key(48, 0)], None);
    assert!(result.is_ok());
    assert_eq!(events, [&CORRECTION[..], &[(48, 1), (48, 0)]].concat());
}

fn run_test(
    during_fix: Vec<DeviceEvent>,
    fail_after: Option<usize>,
) -> (Vec<(u16, i32)>, io::Result<()>) {
    let (tx, rx) = mpsc::sync_channel(32);
    for code in [30, keys::KEY_INSERT] {
        for value in [1, 0] {
            tx.send(message(0, key(code, value))).unwrap();
        }
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let watchdog = thread::spawn(move || {
        if finished_rx.recv_timeout(Duration::from_secs(2)).is_err() {
            stop.store(true, Ordering::Relaxed);
        }
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let injector = Injector::with_output(TestOutput {
        events: events.clone(),
        tx,
        during_fix,
        fail_after,
        stopped: stopped.clone(),
    });
    let cfg = Config {
        session_guard: false,
        key_delay_ms: 1,
        post_backspace_ms: 0,
        switch_delay_ms: 0,
        ..Config::default()
    };
    let result = run(
        &rx,
        injector,
        &cfg,
        false,
        &SessionGuard::new(false),
        &Grabs::default(),
        &stopped,
    );
    let _ = finished_tx.send(());
    watchdog.join().unwrap();
    let events = events.lock().unwrap().clone();
    (events, result)
}

/// Ожидание под захватом с `grabbed_at` = сейчас.
fn wait(
    rx: &Receiver<Message>,
    guard: &SessionGuard,
    generation: u64,
    stopped: bool,
    pause: Pause,
) -> (io::Result<()>, usize) {
    let cfg = Config {
        session_guard: false,
        ..Config::default()
    };
    let mut engine = Engine::new(&cfg, Instant::now());
    let stopped = AtomicBool::new(stopped);
    let mut capture = Capture {
        rx,
        guard,
        stopped: &stopped,
        generation,
        grabbed_at: SystemTime::now(),
        queue: Vec::new(),
        switched: false,
    };
    let result = wait_for_input(&mut capture, &mut engine, &cfg, pause);
    (result, capture.queue.len())
}

#[test]
fn new_input_interrupts_long_wait_immediately() {
    let (tx, rx) = mpsc::sync_channel(1);
    tx.send(message(0, DeviceEvent::Click)).unwrap();
    let started = Instant::now();
    let (result, _) = wait(
        &rx,
        &SessionGuard::new(false),
        0,
        false,
        Pause::Fixed(Duration::from_secs(2)),
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn session_change_and_shutdown_cancel_before_next_key() {
    let (_tx, rx) = mpsc::sync_channel(1);
    let guard = SessionGuard::new(true);
    guard.set(Some("session".into()));
    assert!(
        wait(&rx, &guard, 0, false, Pause::Fixed(Duration::ZERO))
            .0
            .is_err()
    );
    assert!(
        wait(&rx, &guard, 1, true, Pause::Fixed(Duration::ZERO))
            .0
            .is_err()
    );
}

#[test]
fn wait_for_input_closed_stream_or_stale_generation_interrupts() {
    let guard = SessionGuard::new(false);
    let (tx, rx) = mpsc::sync_channel(1);
    drop(tx);
    let (result, _) = wait(&rx, &guard, 0, false, Pause::Fixed(Duration::from_secs(2)));
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    let (tx, rx) = mpsc::sync_channel(1);
    tx.send(message(5, DeviceEvent::Click)).unwrap();
    let (result, _) = wait(&rx, &guard, 0, false, Pause::Fixed(Duration::from_secs(2)));
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
}

#[test]
fn test_wait_for_input_key_before_grab_interrupts_after_grab_queued() {
    let guard = SessionGuard::new(false);
    let (tx, rx) = mpsc::sync_channel(2);
    tx.send(Message {
        generation: 0,
        event: key(30, 1),
        at: SystemTime::UNIX_EPOCH,
    })
    .unwrap();
    let (result, queued) = wait(&rx, &guard, 0, false, Pause::Fixed(Duration::ZERO));
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert_eq!(queued, 0);
    let (tx, rx) = mpsc::sync_channel(2);
    let (result, queued) = {
        // Сообщения отправляются после grabbed_at внутри `wait` нельзя,
        // поэтому время берётся из будущего.
        let later = SystemTime::now() + Duration::from_secs(60);
        for event in [key(30, 1), DeviceEvent::Layout(None)] {
            tx.send(Message {
                generation: 0,
                event,
                at: later,
            })
            .unwrap();
        }
        wait(&rx, &guard, 0, false, Pause::Fixed(Duration::ZERO))
    };
    assert!(result.is_ok());
    assert_eq!(queued, 2);
}

#[test]
fn test_wait_for_input_switch_pause_ends_on_new_layout_signal() {
    let guard = SessionGuard::new(false);
    let (tx, rx) = mpsc::sync_channel(1);
    tx.send(Message {
        generation: 0,
        event: DeviceEvent::Layout(Some(crate::layout::Pair::new(
            crate::layout::Lang::Ru,
            crate::layout::Lang::En,
        ))),
        at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    let started = Instant::now();
    let (result, queued) = wait(&rx, &guard, 0, false, Pause::Switch(Duration::from_secs(2)));
    assert!(result.is_ok());
    assert_eq!(queued, 1);
    assert!(started.elapsed() < Duration::from_secs(1));
    let started = Instant::now();
    let (result, _) = wait(
        &rx,
        &guard,
        0,
        false,
        Pause::Switch(Duration::from_millis(50)),
    );
    assert!(result.is_ok());
    assert!(started.elapsed() >= Duration::from_millis(50));
}

#[test]
fn test_replay_key_pressed_under_grab_released_after_it_not_left_held() {
    // Живой случай: пробел нажат под захватом, поток передал его уже после
    // снятия захвата, а отпускание пришло без захвата.
    let (tx, rx) = mpsc::sync_channel(4);
    let grabbed_at = SystemTime::now() - Duration::from_secs(1);
    for (value, at) in [
        (1, grabbed_at + Duration::from_millis(500)),
        (0, SystemTime::now() + Duration::from_secs(60)),
    ] {
        tx.send(Message {
            generation: 0,
            event: key(keys::KEY_SPACE, value),
            at,
        })
        .unwrap();
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let stopped = Arc::new(AtomicBool::new(false));
    let mut injector = Injector::with_output(TestOutput {
        events: events.clone(),
        tx,
        during_fix: Vec::new(),
        fail_after: None,
        stopped: stopped.clone(),
    });
    let cfg = Config {
        session_guard: false,
        ..Config::default()
    };
    let mut engine = Engine::new(&cfg, Instant::now());
    let guard = SessionGuard::new(false);
    let grabs = Grabs::default();
    let capture = Capture {
        rx: &rx,
        guard: &guard,
        stopped: &stopped,
        generation: 0,
        grabbed_at,
        queue: Vec::new(),
        switched: false,
    };
    capture
        .replay(grabs.grab().unwrap(), &mut injector, &mut engine, &cfg)
        .unwrap();
    assert!(!injector.holding());
    assert_eq!(
        *events.lock().unwrap(),
        [(keys::KEY_SPACE, 1), (keys::KEY_SPACE, 0)]
    );
}

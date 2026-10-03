//! Цикл демона: события устройств -> движок -> прерываемая коррекция.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    },
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    engine::{DeviceEvent, Engine},
    injector::{Injector, KeyOutput},
    session::SessionGuard,
};

const CONTROL_INTERVAL: Duration = Duration::from_millis(10);

/// Событие устройства с поколением сессии, в котором оно прочитано.
pub struct Message {
    pub generation: u64,
    pub event: DeviceEvent,
}

/// Главный цикл: копит ввод в движке и выполняет готовые коррекции.
/// Возвращает `Ok` при остановке и ошибку записи в uinput.
pub fn run<T: KeyOutput>(
    rx: &Receiver<Message>,
    mut injector: Injector<T>,
    cfg: &Config,
    verbose: bool,
    guard: &SessionGuard,
    stopped: &AtomicBool,
) -> io::Result<()> {
    let mut engine = Engine::new(cfg, Instant::now());
    let mut generation = u64::MAX;
    loop {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        let context = guard.context();
        if generation != context.generation {
            generation = context.generation;
            log!(
                "punto-rs: {}",
                if context.session.is_some() {
                    "локальная сессия доступна"
                } else {
                    "коррекция приостановлена: сессия недоступна или заблокирована"
                }
            );
            engine.observe(DeviceEvent::Session(context.session), cfg, Instant::now());
        }
        match rx.recv_timeout(CONTROL_INTERVAL) {
            Ok(message) => {
                let paused = engine.paused;
                if message.generation == generation && guard.context().generation == generation {
                    engine.observe(message.event, cfg, Instant::now());
                } else {
                    engine.discard(&message.event);
                }
                if engine.paused != paused {
                    log!(
                        "punto-rs: {}",
                        if engine.paused {
                            "пауза включена"
                        } else {
                            "пауза выключена"
                        }
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if let Some(fix) = engine.take_ready(cfg, Instant::now()) {
            if verbose {
                log!(
                    "punto-rs: исправляю {} нажатий ({})",
                    fix.strokes.len(),
                    if fix.phrase {
                        "фраза"
                    } else {
                        "слово"
                    }
                );
            }
            let result = injector.fix(&fix.strokes, cfg, |delay| {
                wait_for_input(rx, &mut engine, cfg, guard, generation, stopped, delay)
            });
            if let Err(err) = result {
                engine.invalidate();
                if err.kind() != io::ErrorKind::Interrupted {
                    return Err(err);
                }
                log!(
                    "punto-rs: коррекция прервана; буфер сброшен, текст мог быть изменён частично"
                );
            }
        }
    }
}

fn wait_for_input(
    rx: &Receiver<Message>,
    engine: &mut Engine,
    cfg: &Config,
    guard: &SessionGuard,
    generation: u64,
    stopped: &AtomicBool,
    delay: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + delay;
    loop {
        if stopped.load(Ordering::Relaxed) || guard.context().generation != generation {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "завершение или смена сессии",
            ));
        }
        let message = match rx.try_recv() {
            Ok(message) => Some(message),
            Err(TryRecvError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "поток событий закрыт",
                ));
            }
            Err(TryRecvError::Empty) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(());
                }
                match rx.recv_timeout(remaining.min(CONTROL_INTERVAL)) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "поток событий закрыт",
                        ));
                    }
                    Err(RecvTimeoutError::Timeout) => None,
                }
            }
        };
        if let Some(message) = message {
            if message.generation == generation {
                engine.during_fix(message.event, cfg, Instant::now());
            } else {
                engine.discard(&message.event);
            }
            return Err(io::Error::new(io::ErrorKind::Interrupted, "новый ввод"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::KeyEvent, keys};
    use std::{
        sync::{
            Arc, Mutex,
            mpsc::{self, SyncSender},
        },
        thread,
    };
    struct TestOutput {
        events: Arc<Mutex<Vec<(u16, i32)>>>,
        tx: SyncSender<Message>,
        cancel_after: Option<usize>,
        fail_after: Option<usize>,
        stopped: Arc<AtomicBool>,
    }
    impl KeyOutput for TestOutput {
        fn emit_key(&mut self, code: u16, value: i32) -> io::Result<()> {
            let mut events = self.events.lock().unwrap();
            events.push((code, value));
            let count = events.len();
            if self.cancel_after == Some(count) {
                self.tx
                    .send(Message {
                        generation: 0,
                        event: DeviceEvent::Click,
                    })
                    .unwrap();
                for value in [1, 0] {
                    self.tx
                        .send(Message {
                            generation: 0,
                            event: DeviceEvent::Key(KeyEvent {
                                device_id: 1,
                                code: keys::KEY_INSERT,
                                value,
                            }),
                        })
                        .unwrap();
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

    #[test]
    fn event_loop_cancels_replay_and_does_not_reuse_aborted_buffer() {
        let (events, result) = run_test(Some(1), None);
        assert!(result.is_ok());
        assert_eq!(events, vec![(14, 1), (14, 0)]);
    }

    #[test]
    fn event_loop_exits_on_write_failure_after_releasing_key() {
        let (events, result) = run_test(None, Some(1));
        assert!(result.is_err());
        assert_eq!(events, vec![(14, 1), (14, 0)]);
    }

    #[test]
    fn event_loop_completes_exact_correction() {
        let (events, result) = run_test(None, None);
        assert!(result.is_ok());
        assert_eq!(
            events,
            vec![
                (14, 1),
                (14, 0),
                (125, 1),
                (57, 1),
                (57, 0),
                (125, 0),
                (30, 1),
                (30, 0)
            ]
        );
    }

    fn run_test(
        cancel_after: Option<usize>,
        fail_after: Option<usize>,
    ) -> (Vec<(u16, i32)>, io::Result<()>) {
        let (tx, rx) = mpsc::sync_channel(32);
        for code in [30, keys::KEY_INSERT] {
            for value in [1, 0] {
                tx.send(Message {
                    generation: 0,
                    event: DeviceEvent::Key(KeyEvent {
                        device_id: 1,
                        code,
                        value,
                    }),
                })
                .unwrap();
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
            cancel_after,
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
            &stopped,
        );
        let _ = finished_tx.send(());
        watchdog.join().unwrap();
        let events = events.lock().unwrap().clone();
        (events, result)
    }

    #[test]
    fn new_input_interrupts_long_wait_immediately() {
        let (tx, rx) = mpsc::sync_channel(1);
        let cfg = Config {
            session_guard: false,
            ..Config::default()
        };
        let mut engine = Engine::new(&cfg, Instant::now());
        let guard = SessionGuard::new(false);
        tx.send(Message {
            generation: 0,
            event: DeviceEvent::Click,
        })
        .unwrap();
        let started = Instant::now();
        assert_eq!(
            wait_for_input(
                &rx,
                &mut engine,
                &cfg,
                &guard,
                0,
                &AtomicBool::new(false),
                Duration::from_secs(2)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn session_change_and_shutdown_cancel_before_next_key() {
        let (_tx, rx) = mpsc::sync_channel(1);
        let cfg = Config::default();
        let guard = SessionGuard::new(true);
        let mut engine = Engine::new(&cfg, Instant::now());
        guard.set(Some("session".into()));
        assert!(
            wait_for_input(
                &rx,
                &mut engine,
                &cfg,
                &guard,
                0,
                &AtomicBool::new(false),
                Duration::ZERO
            )
            .is_err()
        );
        assert!(
            wait_for_input(
                &rx,
                &mut engine,
                &cfg,
                &guard,
                1,
                &AtomicBool::new(true),
                Duration::ZERO
            )
            .is_err()
        );
    }

    #[test]
    fn wait_for_input_closed_stream_or_stale_generation_interrupts() {
        let cfg = Config {
            session_guard: false,
            ..Config::default()
        };
        let guard = SessionGuard::new(false);
        let mut engine = Engine::new(&cfg, Instant::now());
        let wait = |rx: &Receiver<Message>, engine: &mut Engine| {
            wait_for_input(
                rx,
                engine,
                &cfg,
                &guard,
                0,
                &AtomicBool::new(false),
                Duration::from_secs(2),
            )
            .unwrap_err()
            .kind()
        };
        let (tx, rx) = mpsc::sync_channel(1);
        drop(tx);
        assert_eq!(wait(&rx, &mut engine), io::ErrorKind::Interrupted);
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Message {
            generation: 5,
            event: DeviceEvent::Click,
        })
        .unwrap();
        assert_eq!(wait(&rx, &mut engine), io::ErrorKind::Interrupted);
    }
}

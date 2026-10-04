//! Цикл демона: события устройств -> движок -> коррекция под захватом клавиатур.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    },
    time::{Duration, Instant, SystemTime},
};

use crate::{
    config::Config,
    devices::{Grab, Grabs},
    engine::{DeviceEvent, Engine},
    injector::{Injector, KeyOutput, Pause},
    session::SessionGuard,
};

const CONTROL_INTERVAL: Duration = Duration::from_millis(10);

/// Событие устройства с поколением сессии, в котором оно прочитано.
pub struct Message {
    pub generation: u64,
    pub event: DeviceEvent,
    /// Время события по ядру: отделяет ввод до захвата клавиатур от ввода под ним.
    pub at: SystemTime,
}

/// Ввод во время коррекции. Клавиатуры захвачены с `grabbed_at`: их нажатия
/// не дошли до композитора и копятся в `queue`, чтобы переиграть их после.
struct Capture<'a> {
    rx: &'a Receiver<Message>,
    guard: &'a SessionGuard,
    stopped: &'a AtomicBool,
    generation: u64,
    grabbed_at: SystemTime,
    queue: Vec<Message>,
    /// KDE сообщила о новой раскладке после захвата.
    switched: bool,
}

impl Capture<'_> {
    fn interrupted(&self) -> bool {
        self.stopped.load(Ordering::Relaxed) || self.guard.context().generation != self.generation
    }

    /// Переигрывает перехваченный ввод через `injector`, скармливает его движку
    /// и снимает `grab`. Захват держится, пока переигранные клавиши не отпущены:
    /// отпускание под захватом иначе не дойдёт до композитора.
    fn replay<T: KeyOutput>(
        mut self,
        grab: Grab,
        injector: &mut Injector<T>,
        engine: &mut Engine,
        cfg: &Config,
    ) -> io::Result<()> {
        let mut queued = std::mem::take(&mut self.queue).into_iter();
        loop {
            let message = match queued.next() {
                Some(message) => message,
                None if !injector.holding() || self.interrupted() => break,
                None => match self.rx.recv_timeout(CONTROL_INTERVAL) {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            let captured = message.at >= self.grabbed_at;
            self.deliver(message, captured, injector, engine, cfg)?;
        }
        drop(grab);
        let released_at = SystemTime::now();
        // Ввод под захватом, который поток устройства ещё не успел передать, и
        // отпускание переигранных из него клавиш: оно приходит уже без захвата.
        // Без него виртуальная клавиша остаётся зажатой, и композитор глотает
        // ту же клавишу с настоящей клавиатуры.
        loop {
            let message = match self.rx.recv_timeout(CONTROL_INTERVAL) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) if injector.holding() && !self.interrupted() => {
                    continue;
                }
                Err(_) => break,
            };
            let captured = (self.grabbed_at..released_at).contains(&message.at);
            self.deliver(message, captured, injector, engine, cfg)?;
            if !captured && !injector.holding() {
                break;
            }
        }
        injector.release_all()
    }

    /// Передаёт событие движку; `captured` - композитор его не видел, и
    /// клавишу надо переиграть. Отпускание клавиши, которую держит
    /// виртуальная клавиатура, переигрывается всегда.
    fn deliver<T: KeyOutput>(
        &self,
        message: Message,
        captured: bool,
        injector: &mut Injector<T>,
        engine: &mut Engine,
        cfg: &Config,
    ) -> io::Result<()> {
        if let DeviceEvent::Key(key) = &message.event
            && (captured || key.value == 0)
        {
            injector.forward(key.code, key.value)?;
        }
        if message.generation == self.generation {
            engine.observe(message.event, cfg, Instant::now());
        } else {
            engine.discard(&message.event);
        }
        Ok(())
    }
}

/// Главный цикл: копит ввод в движке и выполняет готовые коррекции.
/// Возвращает `Ok` при остановке и ошибку записи в uinput.
pub fn run<T: KeyOutput>(
    rx: &Receiver<Message>,
    mut injector: Injector<T>,
    cfg: &Config,
    verbose: bool,
    guard: &SessionGuard,
    grabs: &Grabs,
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
                    match (fix.auto, fix.phrase) {
                        (true, _) => "авто",
                        (false, true) => "фраза",
                        (false, false) => "слово",
                    }
                );
            }
            let grabbed_at = SystemTime::now();
            let grab = match grabs.grab() {
                Ok(grab) => grab,
                Err(err) => {
                    log!("punto-rs: клавиатуры не захвачены, исправление пропущено: {err}");
                    engine.invalidate();
                    continue;
                }
            };
            let mut capture = Capture {
                rx,
                guard,
                stopped,
                generation,
                grabbed_at,
                queue: Vec::new(),
                switched: false,
            };
            let result = injector.fix(&fix.strokes, cfg, |pause| {
                wait_for_input(&mut capture, &mut engine, cfg, pause)
            });
            match result {
                Ok(()) => engine.switched(),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                    engine.invalidate();
                    log!(
                        "punto-rs: коррекция прервана; буфер сброшен, текст мог быть изменён частично"
                    );
                }
                Err(err) => return Err(err),
            }
            capture.replay(grab, &mut injector, &mut engine, cfg)?;
        }
    }
}

/// Ждёт `pause`, копя ввод под захватом в очередь. `Interrupted` - коррекцию
/// надо прервать: ввод до захвата, клик, смена сессии или устройств, остановка.
fn wait_for_input(
    capture: &mut Capture,
    engine: &mut Engine,
    cfg: &Config,
    pause: Pause,
) -> io::Result<()> {
    let rx = capture.rx;
    let deadline = Instant::now() + pause.duration();
    loop {
        // Сигнал KDE о новой раскладке приходит после того, как композитор её
        // применил: дальше ждать срок `switch-delay` незачем.
        if capture.switched && matches!(pause, Pause::Switch(_)) {
            return Ok(());
        }
        if capture.interrupted() {
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
            let captured = message.generation == capture.generation
                && message.at >= capture.grabbed_at
                && matches!(message.event, DeviceEvent::Key(_) | DeviceEvent::Layout(_));
            if captured {
                capture.switched |= matches!(message.event, DeviceEvent::Layout(Some(_)));
                capture.queue.push(message);
                continue;
            }
            if message.generation == capture.generation {
                engine.during_fix(message.event, cfg, Instant::now());
            } else {
                engine.discard(&message.event);
            }
            return Err(io::Error::new(io::ErrorKind::Interrupted, "новый ввод"));
        }
    }
}

#[cfg(test)]
mod tests;

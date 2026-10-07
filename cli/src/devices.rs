//! Чтение устройств evdev: отбор клавиатур, поток событий и пересинхронизация.

use std::{
    collections::{HashMap, HashSet},
    io,
    os::fd::AsFd,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::{Duration, SystemTime},
};

use evdev::{
    Device, EventType, InputEvent, KeyCode as Key, SynchronizationCode as Synchronization,
    raw_stream::RawDevice,
};

use crate::{
    VIRTUAL_NAME,
    config::Config,
    daemon::Message,
    engine::{DeviceEvent, KeyEvent},
    keys,
    session::SessionGuard,
};

const RESCAN_INTERVAL: Duration = Duration::from_secs(3);
static NEXT_DEVICE_ID: AtomicU64 = AtomicU64::new(1);

/// Дубликаты fd слушаемых клавиатур по `device_id`. Дубликат - тот же открытый
/// файл, что у читающего потока: захват через него отнимает ввод у композитора,
/// но не у демона.
#[derive(Clone, Default)]
pub struct Grabs(Arc<Mutex<HashMap<u64, RawDevice>>>);

impl Grabs {
    fn devices(&self) -> MutexGuard<'_, HashMap<u64, RawDevice>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Захватывает все клавиатуры (EVIOCGRAB). Возвращает страж, который
    /// отпускает захват при drop. Ошибка - клавиатура занята другим процессом
    /// или на ней нажата клавиша; захват к этому моменту уже отпущен.
    pub fn grab(&self) -> io::Result<Grab<'_>> {
        let grab = Grab(self);
        let mut devices = self.devices();
        for device in devices.values_mut() {
            device.grab()?;
        }
        // Нажатие до захвата видел композитор, а отпускание под захватом он
        // не увидит: клавиша залипнет с автоповтором.
        for device in devices.values() {
            if device.get_key_state()?.iter().next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    tr!("клавиша ещё нажата", "клавішу ще натиснуто"),
                ));
            }
        }
        drop(devices);
        Ok(grab)
    }
}

/// Действующий захват клавиатур.
pub struct Grab<'a>(&'a Grabs);

impl Drop for Grab<'_> {
    fn drop(&mut self) {
        for device in self.0.devices().values_mut() {
            if let Err(err) = device.ungrab() {
                tr!(
                    log!("punto-rs: не удалось отпустить клавиатуру: {err}"),
                    log!("punto-rs: не вдалося відпустити клавіатуру: {err}")
                );
            }
        }
    }
}

/// Фоновый поток: раз в `RESCAN_INTERVAL` подключает новые устройства по конфигу,
/// пока не выставлен `stopped`. Клавиатуры регистрируются в `grabs`.
pub fn watch(
    tx: SyncSender<Message>,
    cfg: &Config,
    guard: SessionGuard,
    grabs: Grabs,
    stopped: Arc<AtomicBool>,
) {
    let watched = Arc::new(Mutex::new(HashSet::new()));
    let filter = cfg.devices.clone();
    let track_mouse = cfg.track_mouse;
    thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            attach_devices(&tx, &watched, &filter, track_mouse, &guard, &grabs);
            thread::sleep(RESCAN_INTERVAL);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    Keyboard,
    Pointer,
}

fn is_keyboard(device: &Device) -> bool {
    device.supported_keys().is_some_and(|keys| {
        keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) && keys.contains(Key::KEY_SPACE)
    })
}
fn is_pointer(device: &Device) -> bool {
    device
        .supported_keys()
        .is_some_and(|keys| keys.contains(Key::BTN_LEFT))
}

fn attach_devices(
    tx: &SyncSender<Message>,
    watched: &Arc<Mutex<HashSet<PathBuf>>>,
    filter: &[String],
    track_mouse: bool,
    guard: &SessionGuard,
    grabs: &Grabs,
) {
    for (path, device) in evdev::enumerate() {
        let name = device.name().unwrap_or_default().to_string();
        if name == VIRTUAL_NAME || !on_default_seat(&path) {
            continue;
        }
        let wanted_keyboard = if filter.is_empty() {
            is_keyboard(&device)
        } else {
            filter.iter().any(|allowed| allowed == &name)
        };
        let kind = if wanted_keyboard {
            DeviceKind::Keyboard
        } else if track_mouse && is_pointer(&device) {
            DeviceKind::Pointer
        } else {
            continue;
        };
        let mut set = watched.lock().unwrap_or_else(PoisonError::into_inner);
        if set.contains(&path) {
            continue;
        }
        let opened = RawDevice::open(&path).and_then(|raw| {
            let duplicate = match kind {
                DeviceKind::Keyboard => {
                    Some(RawDevice::from_fd(raw.as_fd().try_clone_to_owned()?)?)
                }
                DeviceKind::Pointer => None,
            };
            Ok((raw, duplicate))
        });
        let (raw, duplicate) = match opened {
            Ok(opened) => opened,
            Err(err) => {
                let path = path.display();
                tr!(
                    log!("punto-rs: не удалось открыть {path}: {err}"),
                    log!("punto-rs: не вдалося відкрити {path}: {err}")
                );
                continue;
            }
        };
        set.insert(path.clone());
        drop(set);
        let shown = path.display();
        tr!(
            log!("punto-rs: слушаю «{name}» ({shown})"),
            log!("punto-rs: слухаю «{name}» ({shown})")
        );
        let device_id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(duplicate) = duplicate {
            grabs.devices().insert(device_id, duplicate);
        }
        let tx = tx.clone();
        let watched = watched.clone();
        let guard = guard.clone();
        let grabs = grabs.clone();
        thread::spawn(move || {
            if let Err(err) = read_device(raw, &tx, device_id, kind, &guard) {
                tr!(
                    log!("punto-rs: чтение «{name}» остановлено: {err}"),
                    log!("punto-rs: читання «{name}» зупинено: {err}")
                );
            }
            grabs.devices().remove(&device_id);
            let _ = tx.send(Message {
                generation: guard.context().generation,
                event: DeviceEvent::Disconnected(device_id),
                at: SystemTime::now(),
            });
            watched
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&path);
        });
    }
}

fn on_default_seat(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    let id = metadata.rdev();
    let database = format!("/run/udev/data/c{}:{}", libc::major(id), libc::minor(id));
    match std::fs::read_to_string(database) {
        Ok(data) => data
            .lines()
            .find_map(|line| line.strip_prefix("E:ID_SEAT="))
            .is_none_or(|seat| seat == "seat0"),
        Err(err) => err.kind() == io::ErrorKind::NotFound,
    }
}

#[derive(Default)]
struct EventStream {
    dropped: bool,
}
#[derive(Debug, PartialEq)]
enum StreamAction {
    Ignore,
    Lost,
    Resync,
    Key,
}
impl EventStream {
    fn observe(&mut self, event: &InputEvent) -> StreamAction {
        if event.event_type() == EventType::SYNCHRONIZATION
            && event.code() == Synchronization::SYN_DROPPED.0
        {
            self.dropped = true;
            return StreamAction::Lost;
        }
        if self.dropped {
            if event.event_type() == EventType::SYNCHRONIZATION
                && event.code() == Synchronization::SYN_REPORT.0
            {
                self.dropped = false;
                return StreamAction::Resync;
            }
            return StreamAction::Ignore;
        }
        if event.event_type() == EventType::KEY {
            StreamAction::Key
        } else {
            StreamAction::Ignore
        }
    }
}

fn read_device(
    mut device: RawDevice,
    tx: &SyncSender<Message>,
    device_id: u64,
    kind: DeviceKind,
    guard: &SessionGuard,
) -> io::Result<()> {
    let send = |event, generation, at| {
        tx.send(Message {
            generation,
            event,
            at,
        })
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                tr!("поток событий закрыт", "потік подій закрито"),
            )
        })
    };
    let resynced = |device: &RawDevice| -> io::Result<DeviceEvent> {
        let held_keys = if kind == DeviceKind::Keyboard {
            device.get_key_state()?.iter().map(Key::code).collect()
        } else {
            Vec::new()
        };
        Ok(DeviceEvent::Resynced {
            device_id,
            held_keys,
        })
    };
    send(
        resynced(&device)?,
        guard.context().generation,
        SystemTime::now(),
    )?;
    let mut stream = EventStream::default();
    loop {
        let generation = guard.context().generation;
        let events: Vec<_> = device.fetch_events()?.collect();
        for event in events {
            let at = event.timestamp();
            match stream.observe(&event) {
                StreamAction::Ignore => continue,
                StreamAction::Lost => {
                    send(DeviceEvent::LostEvents(device_id), generation, at)?;
                    continue;
                }
                StreamAction::Resync => {
                    send(resynced(&device)?, generation, at)?;
                    // Снимок учитывает и оставшуюся часть уже прочитанного пакета.
                    break;
                }
                StreamAction::Key => {}
            }
            if let Some(message) = key_message(&event, device_id, kind) {
                send(message, generation, at)?;
            }
        }
    }
}

/// Событие клавиши -> сообщение движку: нажатие кнопки указателя - клик,
/// клавиша клавиатуры - `Key`; прочее (отпускание кнопки, клавиши мыши) - `None`.
fn key_message(event: &InputEvent, device_id: u64, kind: DeviceKind) -> Option<DeviceEvent> {
    if keys::is_pointer_button(event.code()) {
        (event.value() == 1).then_some(DeviceEvent::Click)
    } else if kind == DeviceKind::Keyboard {
        Some(DeviceEvent::Key(KeyEvent {
            device_id,
            code: event.code(),
            value: event.value(),
        }))
    } else {
        None
    }
}

pub fn list_devices() {
    let mut found = false;
    for (path, device) in evdev::enumerate() {
        found = true;
        let name = device.name().unwrap_or(tr!("<без имени>", "<без назви>"));
        let tag = if name == VIRTUAL_NAME {
            tr!(
                "— своё виртуальное устройство",
                "— власний віртуальний пристрій"
            )
        } else if is_keyboard(&device) {
            tr!("— клавиатура", "— клавіатура")
        } else if is_pointer(&device) {
            tr!("— указатель", "— вказівник")
        } else {
            ""
        };
        say!("{:<20} {name:<45} {tag}", path.display());
    }
    if !found {
        tr!(
            log!("punto-rs: устройства не видны — проверьте доступ к /dev/input/*"),
            log!("punto-rs: пристроїв не видно — перевірте доступ до /dev/input/*")
        );
    }
}

#[cfg(test)]
mod tests;

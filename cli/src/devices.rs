//! Чтение устройств evdev: отбор клавиатур, поток событий и пересинхронизация.

use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::Duration,
};

use evdev::{Device, EventType, InputEvent, Key, Synchronization, raw_stream::RawDevice};

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

/// Фоновый поток: раз в `RESCAN_INTERVAL` подключает новые устройства по конфигу,
/// пока не выставлен `stopped`.
pub fn watch(tx: SyncSender<Message>, cfg: &Config, guard: SessionGuard, stopped: Arc<AtomicBool>) {
    let watched = Arc::new(Mutex::new(HashSet::new()));
    let filter = cfg.devices.clone();
    let track_mouse = cfg.track_mouse;
    thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            attach_devices(&tx, &watched, &filter, track_mouse, &guard);
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
        let raw = match RawDevice::open(&path) {
            Ok(raw) => raw,
            Err(err) => {
                log!("punto-rs: не удалось открыть {}: {err}", path.display());
                continue;
            }
        };
        set.insert(path.clone());
        drop(set);
        log!("punto-rs: слушаю «{name}» ({})", path.display());
        let device_id = NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed);
        let tx = tx.clone();
        let watched = watched.clone();
        let guard = guard.clone();
        thread::spawn(move || {
            if let Err(err) = read_device(raw, &tx, device_id, kind, &guard) {
                log!("punto-rs: чтение «{name}» остановлено: {err}");
            }
            let _ = tx.send(Message {
                generation: guard.context().generation,
                event: DeviceEvent::Disconnected(device_id),
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
    let send = |event, generation| {
        tx.send(Message { generation, event })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "поток событий закрыт"))
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
    send(resynced(&device)?, guard.context().generation)?;
    let mut stream = EventStream::default();
    loop {
        let generation = guard.context().generation;
        let events: Vec<_> = device.fetch_events()?.collect();
        for event in events {
            match stream.observe(&event) {
                StreamAction::Ignore => continue,
                StreamAction::Lost => {
                    send(DeviceEvent::LostEvents(device_id), generation)?;
                    continue;
                }
                StreamAction::Resync => {
                    send(resynced(&device)?, generation)?;
                    // Снимок учитывает и оставшуюся часть уже прочитанного пакета.
                    break;
                }
                StreamAction::Key => {}
            }
            if let Some(message) = key_message(&event, device_id, kind) {
                send(message, generation)?;
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
        let name = device.name().unwrap_or("<без имени>");
        let tag = if name == VIRTUAL_NAME {
            "— своё виртуальное устройство"
        } else if is_keyboard(&device) {
            "— клавиатура"
        } else if is_pointer(&device) {
            "— указатель"
        } else {
            ""
        };
        say!("{:<20} {name:<45} {tag}", path.display());
    }
    if !found {
        log!("punto-rs: устройства не видны — проверьте доступ к /dev/input/*");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_events_are_ignored_until_report() {
        let mut stream = EventStream::default();
        assert_eq!(
            stream.observe(&InputEvent::new(EventType::SYNCHRONIZATION, 3, 0)),
            StreamAction::Lost
        );
        assert_eq!(
            stream.observe(&InputEvent::new(EventType::KEY, 30, 1)),
            StreamAction::Ignore
        );
        assert_eq!(
            stream.observe(&InputEvent::new(EventType::SYNCHRONIZATION, 0, 0)),
            StreamAction::Resync
        );
        assert_eq!(
            stream.observe(&InputEvent::new(EventType::KEY, 30, 0)),
            StreamAction::Key
        );
    }

    #[test]
    fn key_message_keyboard_and_pointer_events_map_to_engine_input() {
        let key = |code, value| InputEvent::new(EventType::KEY, code, value);
        assert!(matches!(
            key_message(&key(30, 1), 7, DeviceKind::Keyboard),
            Some(DeviceEvent::Key(KeyEvent {
                device_id: 7,
                code: 30,
                value: 1
            }))
        ));
        for kind in [DeviceKind::Keyboard, DeviceKind::Pointer] {
            assert!(matches!(
                key_message(&key(keys::BTN_LEFT, 1), 7, kind),
                Some(DeviceEvent::Click)
            ));
            assert!(key_message(&key(keys::BTN_LEFT, 0), 7, kind).is_none());
        }
        assert!(key_message(&key(30, 1), 7, DeviceKind::Pointer).is_none());
    }

    #[test]
    fn on_default_seat_missing_path_rejected_and_untagged_node_accepted() {
        assert!(!on_default_seat(Path::new("/nonexistent/punto-rs-input")));
        // Обычный файл даёт rdev 0: записи udev для c0:0 нет -> seat по умолчанию.
        assert!(on_default_seat(Path::new("Cargo.toml")));
    }
}

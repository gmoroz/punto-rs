//! Активная раскладка KDE Plasma: session D-Bus `org.kde.keyboard /Layouts`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::{Duration, SystemTime},
};

use dbus::{blocking::Connection, message::MatchRule};

use crate::{
    config::Config,
    daemon::Message,
    engine::DeviceEvent,
    layout::{Lang, Pair},
    session::SessionGuard,
};

const SERVICE: &str = "org.kde.keyboard";
const INTERFACE: &str = "org.kde.KeyboardLayouts";
const TIMEOUT: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(3);

/// Второй язык пары с `us` по короткому имени и варианту раскладки KDE.
/// Для `ua` годятся только варианты с раскладкой клавиш `ua(unicode)`.
fn second_lang(short: &str, variant: &str) -> Option<Lang> {
    match (short, variant) {
        ("ru", _) => Some(Lang::Ru),
        ("ua", "" | "unicode" | "legacy") => Some(Lang::Uk),
        _ => None,
    }
}

/// Пара раскладок с активной `index` из списка KDE (короткое имя, вариант,
/// название). Только us+ru или us+ua: при других наборах хоткей переключения
/// может увести не в ту раскладку, и автоисправление выключается (`None`).
fn lang_at(layouts: &[(String, String, String)], index: u32) -> Option<Pair> {
    let [first, second] = layouts else {
        return None;
    };
    let (us_first, (short, variant, _)) = match (first.0.as_str(), second.0.as_str()) {
        ("us", _) => (true, second),
        (_, "us") => (false, first),
        _ => return None,
    };
    let pair = Pair::new(Lang::En, second_lang(short, variant)?);
    match (index, us_first) {
        (0, true) | (1, false) => Some(pair),
        (0 | 1, _) => Some(pair.swapped()),
        _ => None,
    }
}

/// Фоновый поток при `auto-switch=yes`: шлёт `DeviceEvent::Layout` при
/// подключении и каждой смене раскладки или их списка. Без D-Bus KDE шлёт
/// `Layout(None)` и переподключается. `connect` - подключение к session bus.
pub fn watch(
    cfg: &Config,
    connect: impl Fn() -> Result<Connection, dbus::Error> + Send + 'static,
    tx: SyncSender<Message>,
    guard: SessionGuard,
    stopped: Arc<AtomicBool>,
) {
    if !cfg.auto_switch {
        return;
    }
    thread::spawn(move || {
        let send = |layout| {
            tx.send(Message {
                generation: guard.context().generation,
                event: DeviceEvent::Layout(layout),
                at: SystemTime::now(),
            })
            .is_ok()
        };
        let mut last_error = None;
        while !stopped.load(Ordering::Relaxed) {
            let error = connect()
                .and_then(|connection| follow(&connection, &send, &stopped))
                .err()
                .map(|err| err.to_string());
            // Без KDE ошибка повторяется на каждой попытке: в журнал - только новая.
            if error.is_some() && error != last_error {
                log!(
                    "punto-rs: раскладка KDE недоступна, автоисправление выключено: {}",
                    error.as_deref().unwrap_or_default()
                );
            }
            last_error = error;
            if !send(None) {
                return;
            }
            thread::sleep(RETRY);
        }
    });
}

fn follow(
    connection: &Connection,
    send: &impl Fn(Option<Pair>) -> bool,
    stopped: &AtomicBool,
) -> Result<(), dbus::Error> {
    let changed = Arc::new(AtomicBool::new(true));
    for member in ["layoutChanged", "layoutListChanged"] {
        let changed = changed.clone();
        let mut rule = MatchRule::new_signal(INTERFACE, member);
        rule.sender = Some(SERVICE.into());
        connection.add_match(rule, move |(): (), _, _| {
            changed.store(true, Ordering::Relaxed);
            true
        })?;
    }
    let proxy = connection.with_proxy(SERVICE, "/Layouts", TIMEOUT);
    let mut reported = None;
    while !stopped.load(Ordering::Relaxed) {
        if changed.swap(false, Ordering::Relaxed) {
            let (layouts,): (Vec<(String, String, String)>,) =
                proxy.method_call(INTERFACE, "getLayoutsList", ())?;
            let (index,): (u32,) = proxy.method_call(INTERFACE, "getLayout", ())?;
            let layout = lang_at(&layouts, index);
            if reported != Some(layout) {
                if !send(layout) {
                    return Ok(());
                }
                reported = Some(layout);
            }
        }
        connection.process(Duration::from_millis(100))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::channel::{MatchingReceiver, Sender};
    use std::sync::mpsc;

    fn layouts(names: &[&str]) -> Vec<(String, String, String)> {
        names
            .iter()
            .map(|name| ((*name).to_string(), String::new(), String::new()))
            .collect()
    }

    #[test]
    fn test_lang_at_us_ru_pair_maps_index_to_lang() {
        assert_eq!(lang_at(&layouts(&["us", "ru"]), 0), Some(EN_RU));
        assert_eq!(lang_at(&layouts(&["us", "ru"]), 1), Some(EN_RU.swapped()));
        assert_eq!(lang_at(&layouts(&["ru", "us"]), 1), Some(EN_RU));
    }

    const EN_RU: Pair = Pair::new(Lang::En, Lang::Ru);
    const EN_UK: Pair = Pair::new(Lang::En, Lang::Uk);

    #[test]
    fn test_lang_at_us_ua_pair_with_unicode_variants_only() {
        let variant = |name: &str| {
            vec![
                ("us".to_string(), String::new(), String::new()),
                ("ua".to_string(), name.to_string(), String::new()),
            ]
        };
        assert_eq!(lang_at(&layouts(&["us", "ua"]), 0), Some(EN_UK));
        assert_eq!(lang_at(&layouts(&["us", "ua"]), 1), Some(EN_UK.swapped()));
        assert_eq!(lang_at(&layouts(&["ua", "us"]), 0), Some(EN_UK.swapped()));
        assert_eq!(lang_at(&variant("legacy"), 1), Some(EN_UK.swapped()));
        assert_eq!(lang_at(&variant("winkeys"), 0), None);
        assert_eq!(lang_at(&variant("phonetic"), 0), None);
        assert_eq!(lang_at(&layouts(&["ru", "ua"]), 0), None);
    }

    /// Приватная шина: `dbus-daemon` на время теста, убивается при drop.
    struct Bus {
        daemon: std::process::Child,
        address: String,
    }
    impl Bus {
        fn start() -> Self {
            use std::io::BufRead;
            let mut daemon = std::process::Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("нужен dbus-daemon");
            let mut address = String::new();
            std::io::BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            Self {
                daemon,
                address: address.trim().to_string(),
            }
        }
        fn connect(address: &str) -> Result<Connection, dbus::Error> {
            let mut channel = dbus::channel::Channel::open_private(address)?;
            channel.register()?;
            Ok(Connection::from(channel))
        }
    }
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// Поддельный `org.kde.keyboard`: отвечает текущим состоянием, а каждая
    /// команда из `commands` меняет его и шлёт сигнал `signal`.
    fn fake_kde(
        address: &str,
        commands: mpsc::Receiver<(&'static str, Vec<&'static str>, u32)>,
        stopped: Arc<AtomicBool>,
    ) {
        let connection = Bus::connect(address).unwrap();
        connection
            .request_name(SERVICE, false, true, false)
            .unwrap();
        let state = Arc::new(std::sync::Mutex::new((layouts(&["us", "ru"]), 0_u32)));
        let replies = state.clone();
        connection.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |call, connection| {
                let (list, index) = replies.lock().unwrap().clone();
                let reply = match call.member().as_deref() {
                    Some("getLayoutsList") => call.method_return().append1(list),
                    _ => call.method_return().append1(index),
                };
                connection.send(reply).unwrap();
                true
            }),
        );
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                if let Ok((signal, names, index)) = commands.try_recv() {
                    *state.lock().unwrap() = (layouts(&names), index);
                    let message = dbus::Message::new_signal("/Layouts", INTERFACE, signal)
                        .unwrap()
                        .append1(index);
                    connection.send(message).unwrap();
                }
                connection.process(Duration::from_millis(10)).unwrap();
            }
        });
    }

    #[test]
    fn test_watch_reports_layout_and_its_changes_from_kde() {
        let bus = Bus::start();
        let stopped = Arc::new(AtomicBool::new(false));
        let (commands, received) = mpsc::channel();
        fake_kde(&bus.address, received, stopped.clone());
        let (tx, rx) = mpsc::sync_channel(8);
        let address = bus.address.clone();
        watch(
            &Config::default(),
            move || Bus::connect(&address),
            tx,
            SessionGuard::new(false),
            stopped.clone(),
        );
        let next = || match rx.recv_timeout(Duration::from_secs(5)).unwrap().event {
            DeviceEvent::Layout(layout) => layout,
            _ => panic!("ожидалась раскладка"),
        };
        assert_eq!(next(), Some(EN_RU));
        commands
            .send(("layoutChanged", vec!["us", "ru"], 1))
            .unwrap();
        assert_eq!(next(), Some(EN_RU.swapped()));
        commands
            .send(("layoutListChanged", vec!["us", "ua"], 0))
            .unwrap();
        assert_eq!(next(), Some(EN_UK));
        commands
            .send(("layoutListChanged", vec!["us", "ru", "de"], 1))
            .unwrap();
        assert_eq!(next(), None);
        drop(bus);
        assert_eq!(next(), None);
        stopped.store(true, Ordering::Relaxed);
    }

    #[test]
    fn test_watch_disabled_sends_nothing() {
        let (tx, rx) = mpsc::sync_channel(1);
        let cfg = Config {
            auto_switch: false,
            ..Config::default()
        };
        let connect = || Err(dbus::Error::new_failed("не вызывается"));
        let stopped = Arc::new(AtomicBool::new(false));
        watch(&cfg, connect, tx, SessionGuard::new(false), stopped);
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn test_lang_at_other_sets_or_bad_index_none() {
        assert_eq!(lang_at(&layouts(&["us", "ru", "de"]), 0), None);
        assert_eq!(lang_at(&layouts(&["us", "de"]), 0), None);
        assert_eq!(lang_at(&layouts(&["us"]), 0), None);
        assert_eq!(lang_at(&layouts(&["us", "ru"]), 2), None);
    }
}

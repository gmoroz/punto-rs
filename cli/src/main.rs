//! Пассивное чтение evdev и прерываемая коррекция через uinput.

/// Строка журнала в stderr (под systemd уходит в journald).
/// Ошибку записи некуда сообщить, поэтому она отбрасывается.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

/// Сообщение на языке пользователя: `tr!(русский, украинский)`.
/// Вычисляется только выбранная ветка, поэтому годится и для `log!`/`format!`.
macro_rules! tr {
    ($ru:expr, $uk:expr $(,)?) => {
        if crate::i18n::ukrainian() { $uk } else { $ru }
    };
}

/// Ответ команды в stdout; закрытый канал (`| head`) не ошибка.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

mod config;
mod daemon;
mod devices;
mod engine;
mod i18n;
mod injector;
mod instance;
mod kde;
mod keys;
mod layout;
mod session;
mod state;

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

use signal_hook::consts::{SIGINT, SIGTERM};

use config::Config;
use daemon::Message;
use injector::Injector;
use instance::InstanceLock;
use session::SessionGuard;

const VIRTUAL_NAME: &str = "punto-rs virtual keyboard";
const CONFIG_FILE: &str = "punto-rs/config.conf";

/// Каталог XDG из переменной `var`; пустое значение не считается заданным.
fn xdg_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Конфиг пользователя: `$XDG_CONFIG_HOME/punto-rs/config.conf`,
/// без переменной - `~/.config/punto-rs/config.conf`.
fn default_config() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME")
        .or_else(|| xdg_dir("HOME").map(|home| home.join(".config")))
        .unwrap_or_else(|| {
            die(tr!(
                "не заданы XDG_CONFIG_HOME и HOME: укажите конфиг через --config",
                "не задано XDG_CONFIG_HOME і HOME: вкажіть конфіг через --config"
            ))
        })
        .join(CONFIG_FILE)
}

fn main() {
    i18n::apply(i18n::Language::Auto);
    let mut config_path = None;
    let mut explicit_config = false;
    let mut verbose = false;
    let mut check_config = false;
    let mut args = std::env::args();
    let _program = args.next();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                config_path = Some(args.next().map_or_else(
                    || {
                        die(tr!(
                            "--config требует путь к файлу",
                            "--config потребує шлях до файлу"
                        ))
                    },
                    PathBuf::from,
                ));
                explicit_config = true;
            }
            "-v" | "--verbose" => verbose = true,
            "--check-config" => check_config = true,
            "--check-session" => {
                match session::check() {
                    Ok(Some(id)) => {
                        tr!(
                            say!("punto-rs: локальная графическая сессия {id} доступна"),
                            say!("punto-rs: локальний графічний сеанс {id} доступний")
                        );
                    }
                    Ok(None) => die(tr!(
                        "локальная графическая сессия недоступна, заблокирована или не поддерживается",
                        "локальний графічний сеанс недоступний, заблокований або не підтримується"
                    )),
                    Err(err) => die(&tr!(
                        format!("проверка logind: {err}"),
                        format!("перевірка logind: {err}")
                    )),
                }
                return;
            }
            "-l" | "--list-devices" => {
                devices::list_devices();
                return;
            }
            "-V" | "--version" => {
                say!("punto-rs {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            other => die(&tr!(
                format!("неизвестный аргумент: {other}"),
                format!("невідомий аргумент: {other}")
            )),
        }
    }
    let config_path = config_path.unwrap_or_else(default_config);
    let cfg = if !explicit_config && !check_config && matches!(config_path.try_exists(), Ok(false))
    {
        tr!(
            log!(
                "punto-rs: {} отсутствует, используются значения по умолчанию",
                config_path.display()
            ),
            log!(
                "punto-rs: {} відсутній, використовуються типові значення",
                config_path.display()
            )
        );
        Config::default()
    } else {
        if let Some(language) = Config::declared_language(&config_path) {
            i18n::apply(language);
        }
        Config::load(&config_path).unwrap_or_else(|err| die(&err))
    };
    i18n::apply(cfg.language);
    if check_config {
        tr!(
            say!("punto-rs: конфиг корректен"),
            say!("punto-rs: конфіг коректний")
        );
        return;
    }
    serve(&cfg, verbose);
}

/// Запуск демона: блокировка экземпляра, потоки сессии, раскладки и устройств,
/// главный цикл до сигнала завершения.
fn serve(cfg: &Config, verbose: bool) {
    let runtime = xdg_dir("XDG_RUNTIME_DIR").unwrap_or_else(|| {
        die(tr!(
            "XDG_RUNTIME_DIR не задан: запускайте в пользовательской сессии",
            "XDG_RUNTIME_DIR не задано: запускайте в сеансі користувача"
        ))
    });
    let _instance = InstanceLock::acquire(&runtime.join("punto-rs")).unwrap_or_else(|err| {
        die(&tr!(
            format!("блокировка экземпляра: {err}"),
            format!("блокування екземпляра: {err}")
        ))
    });
    // Старые версии ещё не брали файловую блокировку.
    if evdev::enumerate().any(|(_, device)| device.name() == Some(VIRTUAL_NAME)) {
        die(tr!(
            "виртуальная клавиатура punto-rs уже существует; сначала остановите предыдущий экземпляр",
            "віртуальна клавіатура punto-rs уже існує; спершу зупиніть попередній екземпляр"
        ));
    }
    let stopped = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM] {
        signal_hook::flag::register(signal, stopped.clone()).unwrap_or_else(|err| {
            die(&tr!(
                format!("обработчик завершения: {err}"),
                format!("обробник завершення: {err}")
            ))
        });
    }
    let injector = Injector::new(VIRTUAL_NAME).unwrap_or_else(|err| {
        tr!(
            log!("punto-rs: не удалось создать виртуальную клавиатуру (/dev/uinput): {err}"),
            log!("punto-rs: не вдалося створити віртуальну клавіатуру (/dev/uinput): {err}")
        );
        std::process::exit(1)
    });
    let guard = SessionGuard::new(cfg.session_guard);
    if cfg.session_guard {
        let guard = guard.clone();
        let stopped = stopped.clone();
        thread::spawn(move || guard.monitor(&stopped));
    } else {
        tr!(
            log!("punto-rs: session-guard=no — блокировка экрана и смена сессии не отслеживаются"),
            log!("punto-rs: session-guard=no — блокування екрана і зміна сеансу не відстежуються")
        );
    }
    let (tx, rx) = mpsc::sync_channel::<Message>(1024);
    let grabs = devices::Grabs::default();
    let session_bus = dbus::blocking::Connection::new_session;
    kde::watch(cfg, session_bus, tx.clone(), guard.clone(), stopped.clone());
    devices::watch(tx, cfg, guard.clone(), grabs.clone(), stopped.clone());
    let version = env!("CARGO_PKG_VERSION");
    let (word, phrase, pause) = (&cfg.hotkey, &cfg.phrase_hotkey, &cfg.pause_hotkey);
    tr!(
        log!("punto-rs {version} запущен: слово {word:?}, фраза {phrase:?}, пауза {pause:?}"),
        log!("punto-rs {version} запущено: слово {word:?}, фраза {phrase:?}, пауза {pause:?}")
    );
    if let Err(err) = daemon::run(&rx, injector, cfg, verbose, &guard, &grabs, &stopped) {
        tr!(
            log!("punto-rs: инжект остановлен после ошибки: {err}"),
            log!("punto-rs: введення зупинено після помилки: {err}")
        );
        std::process::exit(1);
    }
}
fn print_help() {
    if i18n::ukrainian() {
        say!(
            "punto-rs — виправлення розкладки набраного тексту\n\nВикористання: punto-rs [опції]\n\n  -c, --config <файл>  конфіг (типово ~/.config/{CONFIG_FILE})\n      --check-config   перевірити конфіг без відкриття пристроїв\n      --check-session  перевірити доступність сеансу через logind\n  -l, --list-devices   показати пристрої введення\n  -v, --verbose        докладний вивід\n  -V, --version        версія\n  -h, --help           довідка\n\nПауза/відновлення: Super+Pause (pause-hotkey).\nДля запуску потрібне членство в групі input (читання /dev/input/*, запис /dev/uinput)\nі сеанс користувача з XDG_RUNTIME_DIR."
        );
        return;
    }
    say!(
        "punto-rs — исправление раскладки набранного текста\n\nИспользование: punto-rs [опции]\n\n  -c, --config <файл>  конфиг (по умолчанию ~/.config/{CONFIG_FILE})\n      --check-config   проверить конфиг без открытия устройств\n      --check-session  проверить доступность сессии через logind\n  -l, --list-devices   показать устройства ввода\n  -v, --verbose        подробный вывод\n  -V, --version        версия\n  -h, --help           справка\n\nПауза/возобновление: Super+Pause (pause-hotkey).\nДля запуска нужно членство в группе input (чтение /dev/input/*, запись /dev/uinput)\nи пользовательская сессия с XDG_RUNTIME_DIR."
    );
}
fn die(message: &str) -> ! {
    log!("punto-rs: {message}");
    std::process::exit(2);
}

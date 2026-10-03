# punto-rs (`cli`)

Демон исправления раскладки: пассивно читает evdev, хранит скан-коды и
переигрывает их через uinput после системного переключения раскладки.

## Модули

- `cli/src/main.rs` — разбор аргументов, запуск потоков, макросы log!/say!.
- `cli/src/daemon.rs` — главный цикл и прерываемое ожидание во время коррекции.
- `cli/src/devices.rs` — отбор устройств, чтение evdev, SYN_DROPPED.
- `cli/src/engine.rs` — буфер набора и хоткеи без доступа к ОС; тесты в `cli/src/engine/tests.rs`.
- `cli/src/injector.rs` — вывод в uinput: стирание, переключение, повтор.
- `cli/src/session.rs` — logind: локальная незаблокированная сессия seat0.
- `cli/src/config.rs`, `cli/config/punto-rs.conf` — формат конфига и образец.
- `cli/examples/e2e.rs` — живой сценарий evdev/uinput; запускать только в VM.
- `cli/scripts/install.sh`, `cli/systemd/punto-rs.service` — установка и unit.

## Запреты

- unsafe, unwrap/expect/panic/todo/println!/eprintln! вне тестов; журнал — log!.
- Зависимости без точной версии = и без прохода cargo deny / cargo machete.
- Порог coverage задаётся только в `mk/rust-cli.mk` (RUST_COVERAGE_LINES).

## Команды

- make install — инструменты и Cargo.lock.
- make format / make lint / make test / make full-check.

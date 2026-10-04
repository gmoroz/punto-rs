# punto-rs

punto-rs — пользовательский демон Linux/Wayland для исправления уже набранного текста в неверной раскладке. Он пассивно читает evdev и переигрывает физические клавиши через uinput после системного переключения раскладки.

## Карта репозитория

- `cli/` — Rust-крейт демона, конфиг, user unit, примеры и тесты.
- `mk/` — цели Rust CLI и проектная проверка установщика, подключаемые из Makefile.
- `.githooks/` — локальные проверки перед commit и push.
- `.github/` — workflow сборки, тестов и релиза.

Подробности устройства демона: `cli/AGENTS.md`. Правила детектора и его данных: `cli/src/layout/AGENTS.md`. Правила регрессионных тестов захвата: `cli/src/daemon/AGENTS.md`.

## Команды

- `make help` — показать доступные публичные цели.
- `make install` — подключить git hooks и установить Rust-инструменты с зависимостями из Cargo.lock.
- `make format` — отформатировать Rust CLI.
- `make lint` — выполнить корневые проверки, fmt, Clippy, cargo deny и cargo machete.
- `make test` — проверить установщик, затем выполнить nextest и coverage; rust lint является зависимостью тестов.
- `make full-check` — параллельно выполнить lint и test; маркер для push записывается только в чистом дереве.
- `make deploy` — собрать release, установить в домашний каталог и перезапустить user-сервис.
- `make push` — потребовать чистое дерево, выполнить full-check и отправить текущую ветку в origin.

Составные цели доступны и отдельно: `make rust-cli-install`, `make rust-cli-format`, `make rust-cli-lint`, `make rust-cli-test`, `make punto-rs-install-test`. Не запускай `git push` напрямую: отправляй изменения только через `make push`.

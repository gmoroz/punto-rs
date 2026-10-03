---
paths:
  - "cli/**"
  - "mk/rust-cli.mk"
---

# rust-cli

- Логика и ошибки — в `lib` (`thiserror`), `main` только CLI и `anyhow`.
- Не добавляй `unwrap`/`expect`/`todo`/`println!` в production-код: clippy `-D warnings`.
- Новые зависимости — с точной версией `=`, затем `cargo deny` и `cargo machete`.
- Сменил пин в `Cargo.toml.jinja` — обнови `Cargo.lock.jinja`: сгенерируй lock в проекте и перенеси его в шаблон, заменив имя корневого пакета на `{{ _external_data.core_answers.project_name }}`.
- Перед завершением: `make full-check`.

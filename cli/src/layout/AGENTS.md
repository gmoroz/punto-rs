# Детектор раскладки

`cli/src/layout.rs` определяет, похоже ли слово на EN, RU или UK (пара берётся из KDE), по триграммной модели и фильтру Блума. Ложное автоисправление хуже пропуска, поэтому меняй пороги только после оценки на наборе.

## Модель и оценка

- `cli/src/layout/en.bin`, `cli/src/layout/ru.bin`, `cli/src/layout/en.bloom`, `cli/src/layout/ru.bloom`, `cli/src/layout/uk.bin`, `cli/src/layout/uk.bloom` — производные данные. Не правь их вручную.
- Пересобирай модель через `cli/scripts/build_layout_model.sh` (EN, RU) и `cli/scripts/build_uk_model.sh` (UK), который запускает `cli/examples/train_layout_model.rs`.
- Оценивай изменения через `cli/examples/layout_eval.rs` на размеченном наборе `cli/tests/data/layout_cases.tsv`.
- Подбирай пороги только на части dev; test используй только для итогового замера.
- Регрессионные тесты детектора находятся в `cli/src/layout/tests.rs`.
- `cli/src/layout/exceptions.txt` — слова живой речи и термины, которые детектор не должен исправлять (по слову на строку, нижний регистр, сортировка). Пополнять скриптом по корпусу сообщений: прогон wrong_layout, в список только слова, на которых он сейчас возвращает true. Не руками по догадке.

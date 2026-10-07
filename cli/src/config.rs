//! Конфиг проверяется целиком до открытия устройств ввода.

use std::{collections::HashSet, fs, path::Path};

use crate::{i18n::Language, keys};

#[derive(Debug)]
pub struct Config {
    pub hotkey: Vec<u16>,
    pub phrase_hotkey: Vec<u16>,
    pub pause_hotkey: Vec<u16>,
    pub layout_switch: Vec<u16>,
    pub key_delay_ms: u64,
    pub post_backspace_ms: u64,
    pub switch_delay_ms: u64,
    pub devices: Vec<String>,
    pub max_strokes: usize,
    pub track_mouse: bool,
    pub session_guard: bool,
    pub buffer_timeout_ms: u64,
    pub auto_switch: bool,
    pub language: Language,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: vec![keys::KEY_INSERT],
            phrase_hotkey: vec![keys::KEY_LEFTMETA, keys::KEY_INSERT],
            pause_hotkey: vec![keys::KEY_LEFTMETA, 119],
            layout_switch: vec![keys::KEY_LEFTMETA, keys::KEY_SPACE],
            key_delay_ms: 1,
            post_backspace_ms: 0,
            switch_delay_ms: 120,
            devices: Vec::new(),
            max_strokes: 512,
            track_mouse: true,
            session_guard: true,
            buffer_timeout_ms: 30_000,
            auto_switch: true,
            language: Language::Auto,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|err| {
            let path = path.display();
            tr!(
                format!("конфиг {path} не прочитан: {err}"),
                format!("конфіг {path} не прочитано: {err}")
            )
        })?;
        Self::parse(&text)
    }

    /// Язык из строки `language=` файла без разбора остального: ошибки
    /// конфига должны выводиться уже на выбранном языке.
    pub fn declared_language(path: &Path) -> Option<Language> {
        fs::read_to_string(path).ok()?.lines().find_map(|raw| {
            let line = raw.split('#').next().unwrap_or("");
            let (key, value) = line.split_once('=')?;
            (key.trim() == "language")
                .then(|| Language::parse(value.trim().trim_matches('"')))
                .flatten()
        })
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut cfg = Self::default();
        let mut errors = Vec::new();
        let mut seen = HashSet::new();
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let number = lineno + 1;
            let error = |message: &str| {
                tr!(
                    format!("строка {number}: {message}"),
                    format!("рядок {number}: {message}")
                )
            };
            let Some((key, value)) = line.split_once('=') else {
                errors.push(error(tr!("нет '='", "немає '='")));
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(value)
                .trim();
            if !seen.insert(key) {
                errors.push(error(&tr!(
                    format!("повтор ключа '{key}'"),
                    format!("повтор ключа '{key}'")
                )));
                continue;
            }
            let applied = match key {
                "hotkey" => assign_combo(value, &mut cfg.hotkey),
                "phrase-hotkey" => assign_combo(value, &mut cfg.phrase_hotkey),
                "pause-hotkey" => assign_combo(value, &mut cfg.pause_hotkey),
                "layout-switch" => assign_combo(value, &mut cfg.layout_switch),
                "key-delay" => assign_number(value, &mut cfg.key_delay_ms, 1, 100),
                "post-backspace-delay" => assign_number(value, &mut cfg.post_backspace_ms, 0, 2000),
                "switch-delay" => assign_number(value, &mut cfg.switch_delay_ms, 0, 2000),
                "buffer-timeout" => assign_number(value, &mut cfg.buffer_timeout_ms, 1000, 300_000),
                "max-strokes" => value
                    .parse::<usize>()
                    .ok()
                    .filter(|v| (1..=4096).contains(v))
                    .map(|v| cfg.max_strokes = v)
                    .is_some(),
                "track-mouse" => assign_bool(value, &mut cfg.track_mouse),
                "session-guard" => assign_bool(value, &mut cfg.session_guard),
                "auto-switch" => assign_bool(value, &mut cfg.auto_switch),
                "language" => Language::parse(value)
                    .map(|language| cfg.language = language)
                    .is_some(),
                "devices" => {
                    cfg.devices = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                    true
                }
                _ => {
                    errors.push(error(&tr!(
                        format!("неизвестный ключ '{key}'"),
                        format!("невідомий ключ '{key}'")
                    )));
                    continue;
                }
            };
            if !applied {
                errors.push(error(&tr!(
                    format!("недопустимое значение '{value}' для '{key}'"),
                    format!("неприпустиме значення '{value}' для '{key}'")
                )));
            }
        }
        check_combos(&cfg, &mut errors);
        if errors.is_empty() {
            Ok(cfg)
        } else {
            Err(errors.join("\n"))
        }
    }
}

/// Комбинации не совпадают друг с другом, а перед последней клавишей - модификаторы.
fn check_combos(cfg: &Config, errors: &mut Vec<String>) {
    let combos = [
        ("hotkey", &cfg.hotkey),
        ("phrase-hotkey", &cfg.phrase_hotkey),
        ("pause-hotkey", &cfg.pause_hotkey),
        ("layout-switch", &cfg.layout_switch),
    ];
    for (i, (name, combo)) in combos.iter().enumerate() {
        for (other_name, other) in &combos[..i] {
            if combo.len() == other.len() && combo.iter().all(|code| other.contains(code)) {
                errors.push(tr!(
                    format!("'{name}' и '{other_name}' совпадают"),
                    format!("'{name}' і '{other_name}' збігаються")
                ));
            }
        }
        // Все префиксные клавиши должны быть модификаторами: иначе они
        // изменят текст/сбросят буфер ещё до распознавания комбинации.
        if combo[..combo.len() - 1]
            .iter()
            .any(|code| !keys::is_shift(*code) && !keys::is_command_modifier(*code))
        {
            errors.push(tr!(
                format!("'{name}': перед последней клавишей допустимы только модификаторы"),
                format!("'{name}': перед останньою клавішею допустимі лише модифікатори")
            ));
        }
    }
}

fn assign_combo(value: &str, target: &mut Vec<u16>) -> bool {
    keys::parse_combo(value).map(|v| *target = v).is_some()
}

fn assign_number(value: &str, target: &mut u64, min: u64, max: u64) -> bool {
    value
        .parse::<u64>()
        .ok()
        .filter(|v| (min..=max).contains(v))
        .map(|v| *target = v)
        .is_some()
}

fn assign_bool(value: &str, target: &mut bool) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" | "on" | "1" => {
            *target = true;
            true
        }
        "no" | "false" | "off" | "0" => {
            *target = false;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_config_matches_defaults() {
        let cfg = Config::parse(include_str!("../config/punto-rs.conf")).unwrap();
        assert_eq!(cfg.hotkey, Config::default().hotkey);
        assert_eq!(cfg.pause_hotkey, Config::default().pause_hotkey);
        assert!(cfg.session_guard);
        assert!(cfg.auto_switch);
        assert!(!Config::parse("auto-switch=no").unwrap().auto_switch);
    }

    #[test]
    fn parses_names_numbers_comments_and_devices() {
        let cfg = Config::parse("hotkey=119 # Pause\nlayout-switch=29+42\ndevices=\"Keyboard A, Keyboard B\"\ntrack-mouse=no\nswitch-delay=200\n").unwrap();
        assert_eq!(cfg.hotkey, vec![119]);
        assert_eq!(cfg.layout_switch, vec![29, 42]);
        assert_eq!(cfg.devices, vec!["Keyboard A", "Keyboard B"]);
        assert!(!cfg.track_mouse);
        assert_eq!(cfg.switch_delay_ms, 200);
        assert_eq!(cfg.language, Language::Auto);
        let cfg = Config::parse("language=uk").unwrap();
        assert_eq!(cfg.language, Language::Uk);
    }

    #[test]
    fn rejects_invalid_and_ambiguous_config() {
        for text in [
            "hotkey=hyperkey",
            "hotkey=0",
            "layout-switch=65535",
            "hotkey=256",
            "hotkey=super+super+insert",
            "hotkey=",
            "hotkey=insert+super",
            "hotkey=super+insert",
            "phrase-hotkey=super+pause",
            "layout-switch=insert",
            "hotkey=insert\nhotkey=pause",
            "unknown=1",
            "missing equals",
            "key-delay=0",
            "key-delay=101",
            "switch-delay=2001",
            "max-strokes=0",
            "max-strokes=4097",
            "buffer-timeout=999",
            "session-guard=maybe",
            "language=de",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn accepts_bounds_and_modifier_only_switch() {
        Config::parse("key-delay=1\npost-backspace-delay=0\nswitch-delay=2000\nmax-strokes=4096\nbuffer-timeout=300000\nlayout-switch=ctrl+shift").unwrap();
    }

    #[test]
    fn declared_language_read_before_full_parse() {
        let dir = std::env::temp_dir().join(format!("punto-rs-lang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.conf");
        std::fs::write(&path, "hotkey=foo\nlanguage = \"uk\" # мова\n").unwrap();
        assert_eq!(Config::declared_language(&path), Some(Language::Uk));
        std::fs::write(&path, "hotkey=insert\n").unwrap();
        assert_eq!(Config::declared_language(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(Config::load(Path::new("/nonexistent/punto-rs.conf")).is_err());
    }
}

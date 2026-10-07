//! Язык сообщений журнала и команд: русский (по умолчанию) или украинский.
//!
//! Выбирается ключом `language` конфига; `auto` смотрит на локаль окружения
//! (`LC_ALL`, `LC_MESSAGES`, `LANG`): `uk*` - украинский, иначе русский.

use std::sync::atomic::{AtomicBool, Ordering};

static UKRAINIAN: AtomicBool = AtomicBool::new(false);

/// Значение ключа `language`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    Auto,
    Ru,
    Uk,
}

impl Language {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "ru" => Some(Self::Ru),
            "uk" | "ua" => Some(Self::Uk),
            _ => None,
        }
    }

    /// Украинский ли язык; `env` - чтение переменной окружения.
    fn is_ukrainian(self, env: impl Fn(&str) -> Option<String>) -> bool {
        match self {
            Self::Ru => false,
            Self::Uk => true,
            Self::Auto => ["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .find_map(|name| env(name).filter(|value| !value.is_empty()))
                .is_some_and(|locale| locale.starts_with("uk")),
        }
    }
}

/// Устанавливает язык сообщений процесса.
pub fn apply(language: Language) {
    let ukrainian = language.is_ukrainian(|name| std::env::var(name).ok());
    UKRAINIAN.store(ukrainian, Ordering::Relaxed);
}

/// Сообщения на украинском.
pub fn ukrainian() -> bool {
    UKRAINIAN.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn test_language_parse_values() {
        assert_eq!(Language::parse("auto"), Some(Language::Auto));
        assert_eq!(Language::parse("RU"), Some(Language::Ru));
        assert_eq!(Language::parse("uk"), Some(Language::Uk));
        assert_eq!(Language::parse("ua"), Some(Language::Uk));
        assert_eq!(Language::parse("en"), None);
    }

    #[test]
    fn test_language_auto_follows_first_set_locale_variable() {
        assert!(Language::Auto.is_ukrainian(env(&[("LANG", "uk_UA.UTF-8")])));
        assert!(!Language::Auto.is_ukrainian(env(&[("LANG", "ru_RU.UTF-8")])));
        assert!(!Language::Auto.is_ukrainian(env(&[("LANG", "en_US.UTF-8")])));
        assert!(!Language::Auto.is_ukrainian(env(&[])));
        let mixed = env(&[("LC_ALL", ""), ("LC_MESSAGES", "uk_UA"), ("LANG", "ru_RU")]);
        assert!(Language::Auto.is_ukrainian(mixed));
        assert!(Language::Uk.is_ukrainian(env(&[("LANG", "ru_RU")])));
        assert!(!Language::Ru.is_ukrainian(env(&[("LANG", "uk_UA")])));
    }
}

//! Определение слова, набранного не в той раскладке (EN <-> RU или EN <-> UK).
//!
//! Модуль самодостаточен: без `crate::` импортов, чтобы генератор модели и
//! оценщик в `examples/` подключали его через `#[path]`.
//!
//! Оценка - триграммная модель букв каждого языка: средняя цена символа в
//! битах. Слово переключается, только если в другой раскладке оно заметно
//! правдоподобнее, чем на экране: ложное срабатывание хуже пропуска.

/// Язык раскладки: определяет, какие символы отдают клавиши.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
    Uk,
}

impl Lang {
    fn alphabet(self) -> &'static [char] {
        match self {
            Lang::En => &EN_ALPHABET,
            Lang::Ru => &RU_ALPHABET,
            Lang::Uk => &UK_ALPHABET,
        }
    }

    fn model(self) -> &'static [u8] {
        match self {
            Lang::En => EN_MODEL,
            Lang::Ru => RU_MODEL,
            Lang::Uk => UK_MODEL,
        }
    }

    fn dictionary(self) -> &'static [u8] {
        match self {
            Lang::En => EN_WORDS,
            Lang::Ru => RU_WORDS,
            Lang::Uk => UK_WORDS,
        }
    }
}

/// Пара раскладок системы: `shown` - активная, `other` - вторая.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub shown: Lang,
    pub other: Lang,
}

impl Pair {
    pub const fn new(shown: Lang, other: Lang) -> Self {
        Self { shown, other }
    }

    /// Та же пара после переключения раскладки.
    pub const fn swapped(self) -> Self {
        Self::new(self.other, self.shown)
    }
}

#[path = "layout/keymap.rs"]
mod keymap;
pub use keymap::key_char;
use keymap::{EN_ALPHABET, RU_ALPHABET, UK_ALPHABET, short_words};

/// Цена символа в таблице: `-log2(p) * COST_SCALE`, округлённая до u8.
pub const COST_SCALE: f64 = 10.0;
const EN_MODEL: &[u8] = include_bytes!("layout/en.bin");
const RU_MODEL: &[u8] = include_bytes!("layout/ru.bin");
const UK_MODEL: &[u8] = include_bytes!("layout/uk.bin");
const _: () = assert!(EN_MODEL.len() == table_len(Lang::En));
const _: () = assert!(RU_MODEL.len() == table_len(Lang::Ru));
const _: () = assert!(UK_MODEL.len() == table_len(Lang::Uk));
/// Словари - фильтры Блума по индексам букв: около 1% ложных «есть в словаре».
const EN_WORDS: &[u8] = include_bytes!("layout/en.bloom");
const RU_WORDS: &[u8] = include_bytes!("layout/ru.bloom");
const UK_WORDS: &[u8] = include_bytes!("layout/uk.bloom");
pub const BLOOM_HASHES: u64 = 7;

/// Слова короче этого не исправляются сами: у 1-2 букв нет статистики,
/// а «в»/«d» и «и»/«b» одинаково правдоподобны в обоих языках.
pub const MIN_LETTERS: usize = 3;
/// Перевес правдоподобия другой раскладки, бит на символ.
pub const MIN_MARGIN: f64 = 1.0;
/// Слово в другой раскладке должно само быть похоже на слово своего языка.
pub const MAX_ALT_COST: f64 = 6.0;
/// Цена неразборчивой записи на экране: буква внутри не из алфавита.
const UNREADABLE_COST: f64 = 25.5;

/// Число символов таблицы: алфавит и граница слова (индекс 0).
pub const fn symbols(lang: Lang) -> usize {
    match lang {
        Lang::En => EN_ALPHABET.len() + 1,
        Lang::Ru => RU_ALPHABET.len() + 1,
        Lang::Uk => UK_ALPHABET.len() + 1,
    }
}

pub const fn table_len(lang: Lang) -> usize {
    symbols(lang) * symbols(lang) * symbols(lang)
}

/// Индекс буквы в таблице языка (с 1), `None` - не буква этого алфавита.
pub fn letter_index(lang: Lang, letter: char) -> Option<usize> {
    let letter = if letter == 'ё' { 'е' } else { letter };
    lang.alphabet()
        .iter()
        .position(|&known| known == letter)
        .map(|position| position + 1)
}

/// Индексы триграмм слова с границами: `^^слово$`.
pub fn trigrams(lang: Lang, word: &[usize]) -> impl Iterator<Item = usize> + '_ {
    let n = symbols(lang);
    let padded = [0, 0].into_iter().chain(word.iter().copied()).chain([0]);
    let padded: Vec<usize> = padded.collect();
    (2..padded.len()).map(move |i| (padded[i - 2] * n + padded[i - 1]) * n + padded[i])
}

/// Номера бит слова в фильтре Блума из `bits` бит: двойное хеширование FNV-1a.
pub fn bloom_bits(word: &[usize], bits: u64) -> impl Iterator<Item = u64> {
    let fnv = |seed: u64| {
        word.iter().fold(seed, |hash, &letter| {
            (hash ^ u64::try_from(letter).unwrap_or(u64::MAX)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let first = fnv(0xcbf2_9ce4_8422_2325);
    // Нечётный шаг обходит все биты при любом размере фильтра.
    let step = fnv(0x8422_2325_cbf2_9ce4) | 1;
    (0..BLOOM_HASHES).map(move |i| first.wrapping_add(i.wrapping_mul(step)) % bits)
}

/// Есть ли слово в словаре языка (с точностью фильтра Блума).
fn known(lang: Lang, word: &[usize]) -> bool {
    let filter = lang.dictionary();
    let bits = u64::try_from(filter.len()).unwrap_or(0) * 8;
    bits > 0
        && bloom_bits(word, bits).all(|bit| {
            usize::try_from(bit / 8)
                .ok()
                .and_then(|byte| filter.get(byte))
                .is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
        })
}

/// Средняя цена символа слова в битах; меньше - правдоподобнее.
fn cost(lang: Lang, word: &[usize]) -> f64 {
    let model = lang.model();
    let (sum, count) = trigrams(lang, word).fold((0_u32, 0_u32), |(sum, count), index| {
        (sum + u32::from(model[index]), count + 1)
    });
    f64::from(sum) / f64::from(count.max(1)) / COST_SCALE
}

/// Буквы слова без пунктуации по краям, в нижнем регистре.
/// `None` - внутри слова символ не из алфавита (цифра, `_`, точка).
fn core(lang: Lang, text: &[char]) -> Option<Vec<usize>> {
    let is_letter = |ch: &char| {
        *ch != '\'' && letter_index(lang, ch.to_lowercase().next().unwrap_or(*ch)).is_some()
    };
    let start = text.iter().position(is_letter)?;
    let end = text.iter().rposition(is_letter)? + 1;
    text[start..end]
        .iter()
        .map(|ch| letter_index(lang, ch.to_lowercase().next().unwrap_or(*ch)))
        .collect()
}

/// Оценка слова в двух раскладках.
#[derive(Clone, Copy, Debug)]
pub struct Scores {
    /// Цена записи на экране, бит на символ.
    pub shown_cost: f64,
    /// Цена записи в другой раскладке.
    pub alt_cost: f64,
    pub shown_known: bool,
    pub alt_known: bool,
}

impl Scores {
    /// Исправлять ли при порогах `margin`/`max_alt`. Нужно словарное слово в
    /// другой раскладке и несловарное на экране: термины вроде `dnf` дают
    /// правдоподобную бессмыслицу в другой раскладке, и по одной модели их
    /// не отличить.
    pub fn should_switch(&self, margin: f64, max_alt: f64) -> bool {
        self.alt_known
            && !self.shown_known
            && self.alt_cost <= max_alt
            && self.shown_cost - self.alt_cost >= margin
    }
}

/// Оценка слова (нажатия до пробела). `None` - слово не кандидат: цифры,
/// меньше `MIN_LETTERS` букв или в другой раскладке внутри не буквы.
pub fn scores(keys: &[(u16, bool)], pair: Pair) -> Option<Scores> {
    let Pair { shown, other } = pair;
    let render = |lang| -> Option<Vec<char>> {
        keys.iter()
            .map(|&(code, shift)| key_char(lang, code, shift))
            .collect()
    };
    let on_screen = render(shown)?;
    if on_screen.iter().any(char::is_ascii_digit) {
        return None;
    }
    let alt_core = core(other, &render(other)?)?;
    if alt_core.len() < MIN_LETTERS {
        return None;
    }
    let shown_core = core(shown, &on_screen);
    Some(Scores {
        shown_cost: shown_core
            .as_ref()
            .map_or(UNREADABLE_COST, |letters| cost(shown, letters)),
        alt_cost: cost(other, &alt_core),
        shown_known: shown_core.is_some_and(|letters| known(shown, &letters)),
        alt_known: known(other, &alt_core),
    })
}

/// Слова живой речи/термины, которые детектор ошибочно считает чужой раскладкой.
/// Пополнять скриптом по корпусу, не руками: см. `layout/AGENTS.md`.
const EXCEPTIONS: &str = include_str!("layout/exceptions.txt");

/// Решает, набрано ли слово не в той раскладке.
/// `pair.shown` - раскладка, в которой слово сейчас на экране.
pub fn wrong_layout(keys: &[(u16, bool)], pair: Pair) -> bool {
    if is_exception(keys, pair.shown) {
        return false;
    }
    scores(keys, pair).is_some_and(|scores| scores.should_switch(MIN_MARGIN, MAX_ALT_COST))
        // Одна буква сама не исправляется: `f`/`а`, `d`/`в` одинаково возможны.
        || (keys.len() == 2 && short_wrong(keys, pair) && !shown_known(keys, pair.shown))
}

/// Запись нажатий в раскладке `lang` в нижнем регистре, `ё` -> `е`.
fn lowercase(lang: Lang, keys: &[(u16, bool)]) -> Option<String> {
    keys.iter()
        .map(|&(code, shift)| {
            let ch = key_char(lang, code, shift)?.to_lowercase().next()?;
            Some(if ch == 'ё' { 'е' } else { ch })
        })
        .collect()
}

/// Есть ли запись на экране в словаре; с не-буквами внутри или по краям - нет.
fn shown_known(keys: &[(u16, bool)], shown: Lang) -> bool {
    lowercase(shown, keys)
        .and_then(|text| {
            text.chars()
                .map(|ch| letter_index(shown, ch))
                .collect::<Option<Vec<_>>>()
        })
        .is_some_and(|letters| known(shown, &letters))
}

/// Короткое слово (1-2 нажатия) в чужой раскладке: в другой раскладке это
/// частое слово из списка, а на экране - нет. Для одной буквы признак слабый,
/// и движок применяет его только перед словом, которое исправляется.
pub fn short_wrong(keys: &[(u16, bool)], pair: Pair) -> bool {
    if is_exception(keys, pair.shown) {
        return false;
    }
    let short = |lang: Lang| {
        lowercase(lang, keys).is_some_and(|text| short_words(lang).contains(&text.as_str()))
    };
    keys.len() <= 2 && short(pair.other) && !short(pair.shown)
}

/// Буквы слова без пунктуации по краям (текст уже в нижнем регистре, `ё` -> `е`).
fn word_core(text: &str) -> Option<&str> {
    let is_letter = |ch: char| ch.is_alphabetic();
    let start = text.find(is_letter)?;
    let end = text
        .char_indices()
        .rev()
        .find(|&(_, ch)| is_letter(ch))
        .map(|(index, ch)| index + ch.len_utf8())?;
    Some(&text[start..end])
}

/// Слово из списка исключений: регистр и краевая пунктуация не важны.
fn is_exception(keys: &[(u16, bool)], shown: Lang) -> bool {
    lowercase(shown, keys)
        .as_deref()
        .and_then(word_core)
        .is_some_and(|word| EXCEPTIONS.lines().any(|line| line == word))
}

// Явный путь: модуль подключают и через `#[path]` из `examples/`.
#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;

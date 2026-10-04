//! Определение слова, набранного не в той раскладке (EN <-> RU).
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
}

impl Lang {
    pub fn other(self) -> Self {
        match self {
            Lang::En => Lang::Ru,
            Lang::Ru => Lang::En,
        }
    }

    fn alphabet(self) -> &'static [char] {
        match self {
            Lang::En => &EN_ALPHABET,
            Lang::Ru => &RU_ALPHABET,
        }
    }

    fn model(self) -> &'static [u8] {
        match self {
            Lang::En => EN_MODEL,
            Lang::Ru => RU_MODEL,
        }
    }

    fn dictionary(self) -> &'static [u8] {
        match self {
            Lang::En => EN_WORDS,
            Lang::Ru => RU_WORDS,
        }
    }
}

const EN_ALPHABET: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];
/// `ё` сводится к `е` до оценки: в словарях и текстах они смешаны.
const RU_ALPHABET: [char; 32] = [
    'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с', 'т',
    'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я',
];

/// Цена символа в таблице: `-log2(p) * COST_SCALE`, округлённая до u8.
pub const COST_SCALE: f64 = 10.0;
const EN_MODEL: &[u8] = include_bytes!("layout/en.bin");
const RU_MODEL: &[u8] = include_bytes!("layout/ru.bin");
const _: () = assert!(EN_MODEL.len() == table_len(Lang::En));
const _: () = assert!(RU_MODEL.len() == table_len(Lang::Ru));
/// Словари - фильтры Блума по индексам букв: около 1% ложных «есть в словаре».
const EN_WORDS: &[u8] = include_bytes!("layout/en.bloom");
const RU_WORDS: &[u8] = include_bytes!("layout/ru.bloom");
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

/// Символ клавиши в раскладке: (без Shift, с Shift). Только клавиши, для
/// которых `keys::is_char` истинно; остальные дают `None`.
pub fn key_char(lang: Lang, code: u16, shift: bool) -> Option<char> {
    const EN: &str = "1234567890-=qwertyuiop[]asdfghjkl;'`\\zxcvbnm,./";
    const EN_SHIFT: &str = "!@#$%^&*()_+QWERTYUIOP{}ASDFGHJKL:\"~|ZXCVBNM<>?";
    const RU: &str = "1234567890-=йцукенгшщзхъфывапролджэё\\ячсмитьбю.";
    const RU_SHIFT: &str = "!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЁ/ЯЧСМИТЬБЮ,";
    // Порядок строк выше = скан-коды 2..=13, 16..=27, 30..=41, 43..=53.
    let position = match code {
        2..=13 => code - 2,
        16..=27 => code - 4,
        30..=41 => code - 6,
        43..=53 => code - 7,
        _ => return None,
    };
    let row = match (lang, shift) {
        (Lang::En, false) => EN,
        (Lang::En, true) => EN_SHIFT,
        (Lang::Ru, false) => RU,
        (Lang::Ru, true) => RU_SHIFT,
    };
    row.chars().nth(usize::from(position))
}

/// Буквы слова без пунктуации по краям, в нижнем регистре.
/// `None` - внутри слова символ не из алфавита (цифра, `_`, точка).
fn core(lang: Lang, text: &[char]) -> Option<Vec<usize>> {
    let is_letter =
        |ch: &char| letter_index(lang, ch.to_lowercase().next().unwrap_or(*ch)).is_some();
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
pub fn scores(keys: &[(u16, bool)], shown: Lang) -> Option<Scores> {
    let render = |lang| -> Option<Vec<char>> {
        keys.iter()
            .map(|&(code, shift)| key_char(lang, code, shift))
            .collect()
    };
    let on_screen = render(shown)?;
    if on_screen.iter().any(char::is_ascii_digit) {
        return None;
    }
    let alt_core = core(shown.other(), &render(shown.other())?)?;
    if alt_core.len() < MIN_LETTERS {
        return None;
    }
    let shown_core = core(shown, &on_screen);
    Some(Scores {
        shown_cost: shown_core
            .as_ref()
            .map_or(UNREADABLE_COST, |letters| cost(shown, letters)),
        alt_cost: cost(shown.other(), &alt_core),
        shown_known: shown_core.is_some_and(|letters| known(shown, &letters)),
        alt_known: known(shown.other(), &alt_core),
    })
}

/// Решает, набрано ли слово не в той раскладке.
/// `shown` - раскладка, в которой слово сейчас на экране.
pub fn wrong_layout(keys: &[(u16, bool)], shown: Lang) -> bool {
    scores(keys, shown).is_some_and(|scores| scores.should_switch(MIN_MARGIN, MAX_ALT_COST))
        // Одна буква сама не исправляется: `f`/`а`, `d`/`в` одинаково возможны.
        || (keys.len() == 2 && short_wrong(keys, shown) && !shown_known(keys, shown))
}

/// Частые слова из 1-2 букв: статистики у них нет, решает закрытый список.
const EN_SHORT: [&str; 29] = [
    "a", "i", "am", "an", "as", "at", "be", "by", "do", "go", "he", "hi", "if", "in", "is", "it",
    "me", "my", "no", "of", "oh", "ok", "on", "or", "so", "to", "up", "us", "we",
];
const RU_SHORT: [&str; 41] = [
    "а", "в", "и", "к", "о", "с", "у", "я", "ж", "бы", "во", "вы", "да", "до", "ее", "ей", "же",
    "за", "из", "им", "их", "ко", "ли", "мы", "на", "не", "ни", "но", "ну", "об", "ой", "он", "от",
    "по", "со", "та", "те", "то", "ты", "уж", "ах",
];

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
pub fn short_wrong(keys: &[(u16, bool)], shown: Lang) -> bool {
    let short = |lang: Lang| {
        let words: &[&str] = match lang {
            Lang::En => &EN_SHORT,
            Lang::Ru => &RU_SHORT,
        };
        lowercase(lang, keys).is_some_and(|text| words.contains(&text.as_str()))
    };
    keys.len() <= 2 && short(shown.other()) && !short(shown)
}

// Явный путь: модуль подключают и через `#[path]` из `examples/`.
#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;

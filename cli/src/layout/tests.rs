use super::*;

/// Размеченный набор: split, класс, раскладка на экране, набранное, fix/keep.
const CASES: &str = include_str!("../../tests/data/layout_cases.tsv");
/// Полнота на `test` при текущих порогах - 190 из 200; запас на пересборку словарей.
const MIN_CAUGHT: usize = 185;
/// Полнота `uk_wrong` на `test` - 110 из 115; запас на пересборку словаря.
const MIN_CAUGHT_UK: usize = 105;

const EN_RU: Pair = Pair::new(Lang::En, Lang::Ru);
const RU_EN: Pair = EN_RU.swapped();
const EN_UK: Pair = Pair::new(Lang::En, Lang::Uk);
const UK_EN: Pair = EN_UK.swapped();

/// Пара по колонке набора: раскладка на экране, `-uk` - вторая украинская.
fn pair_for(name: &str) -> Pair {
    match name {
        "en" => EN_RU,
        "ru" => RU_EN,
        "en-uk" => EN_UK,
        "uk" => UK_EN,
        _ => panic!("неизвестная пара {name}"),
    }
}

/// Нажатия, которые дают `text` в раскладке `lang`.
fn keys_for(lang: Lang, text: &str) -> Vec<(u16, bool)> {
    text.chars()
        .filter_map(|ch| {
            (2..=53)
                .flat_map(|code| [(code, false), (code, true)])
                .find(|&(code, shift)| key_char(lang, code, shift) == Some(ch))
        })
        .collect()
}

/// Решения детектора на части `test`: (класс, нужно ли исправлять, исправлено ли).
fn test_split_decisions() -> Vec<(&'static str, bool, bool)> {
    CASES
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields[0] == "test")
        .map(|fields| {
            let pair = pair_for(fields[2]);
            let keys = keys_for(pair.shown, fields[3]);
            assert_eq!(keys.len(), fields[3].chars().count(), "{}", fields[3]);
            (fields[1], fields[4] == "fix", wrong_layout(&keys, pair))
        })
        .collect()
}

#[test]
fn test_wrong_layout_keep_classes_never_switched() {
    let decisions = test_split_decisions();
    let false_fixes: Vec<_> = decisions
        .iter()
        .filter(|&&(_, should_fix, fixed)| !should_fix && fixed)
        .collect();
    assert!(decisions.iter().filter(|case| !case.1).count() >= 360);
    assert_eq!(false_fixes, Vec::<&(&str, bool, bool)>::new());
}

#[test]
fn test_wrong_layout_long_wrong_words_mostly_switched() {
    let caught = test_split_decisions()
        .iter()
        .filter(|&&(class, _, fixed)| (class == "ru_wrong" || class == "en_wrong") && fixed)
        .count();
    assert!(caught >= MIN_CAUGHT, "исправлено {caught}");
    let caught_uk = test_split_decisions()
        .iter()
        .filter(|&&(class, _, fixed)| class == "uk_wrong" && fixed)
        .count();
    assert!(caught_uk >= MIN_CAUGHT_UK, "исправлено {caught_uk}");
}

#[test]
fn test_key_char_ghbdtn_in_ru_gives_privet() {
    let word: String = keys_for(Lang::En, "ghbdtn")
        .iter()
        .filter_map(|&(code, shift)| key_char(Lang::Ru, code, shift))
        .collect();
    assert_eq!(word, "привет");
    assert_eq!(key_char(Lang::Ru, 1, false), None);
}

#[test]
fn test_wrong_layout_ghbdtn_on_en_switched_privet_on_ru_kept() {
    let keys = keys_for(Lang::En, "ghbdtn");
    assert!(wrong_layout(&keys, EN_RU));
    assert!(!wrong_layout(&keys, RU_EN));
}

#[test]
fn test_wrong_layout_short_words_by_list_single_letter_never() {
    // jy = «он», шы = «is»; «on»/«он» на своём месте и одна буква f/«а» - нет.
    assert!(wrong_layout(&keys_for(Lang::En, "jy"), EN_RU));
    assert!(wrong_layout(&keys_for(Lang::Ru, "шы"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::En, "on"), EN_RU));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "он"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::En, "F"), EN_RU));
    assert!(short_wrong(&keys_for(Lang::En, "F"), EN_RU));
    assert!(!short_wrong(&keys_for(Lang::En, "a"), EN_RU));
    assert!(short_wrong(&keys_for(Lang::En, "t`"), EN_RU));
}

#[test]
fn test_scores_digits_or_short_word_not_candidate() {
    assert!(scores(&keys_for(Lang::En, "ghb2"), EN_RU).is_none());
    assert!(scores(&keys_for(Lang::En, "yt"), EN_RU).is_none());
}

#[test]
fn test_known_dictionary_word_found_gibberish_not() {
    let letters = |lang, word: &str| -> Vec<usize> {
        word.chars()
            .filter_map(|ch| letter_index(lang, ch))
            .collect()
    };
    assert!(known(Lang::Ru, &letters(Lang::Ru, "привет")));
    assert!(known(Lang::En, &letters(Lang::En, "hello")));
    assert!(!known(Lang::Ru, &letters(Lang::Ru, "ршщзх")));
}

#[test]
fn test_wrong_layout_exception_word_not_switched() {
    // Живая речь и термины из exceptions.txt: на экране оставляем.
    assert!(!wrong_layout(&keys_for(Lang::Ru, "афк"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "ща"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "Ща"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "ща,"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "ру"), RU_EN));
    assert!(!wrong_layout(&keys_for(Lang::En, "tls"), EN_RU));
    assert!(!wrong_layout(&keys_for(Lang::En, "TLS"), EN_RU));
    assert!(!short_wrong(&keys_for(Lang::Ru, "ру"), RU_EN));
    assert!(!short_wrong(&keys_for(Lang::Ru, "ин"), RU_EN));
}

#[test]
fn test_wrong_layout_typo_still_switched_despite_exceptions() {
    // Настоящие опечатки раскладки исключениями не стали.
    assert!(wrong_layout(&keys_for(Lang::En, "ghbdtn"), EN_RU));
    assert!(wrong_layout(&keys_for(Lang::En, "nfr"), EN_RU));
    assert!(wrong_layout(&keys_for(Lang::En, "xnj"), EN_RU));
    assert!(wrong_layout(&keys_for(Lang::Ru, "пше"), RU_EN));
    assert!(wrong_layout(&keys_for(Lang::Ru, "пщ"), RU_EN));
    assert!(wrong_layout(&keys_for(Lang::En, "jy"), EN_RU));
    assert!(wrong_layout(&keys_for(Lang::En, "negb"), EN_RU));
}

#[test]
fn test_key_char_ukrainian_layout_differs_from_russian() {
    let word = |text: &str| -> String {
        keys_for(Lang::En, text)
            .iter()
            .filter_map(|&(code, shift)| key_char(Lang::Uk, code, shift))
            .collect()
    };
    assert_eq!(word("ghbdsn"), "привіт");
    assert_eq!(word("v`zcj"), "м'ясо");
    assert_eq!(word("]`;;"), "ї'жж");
    assert_eq!(word("\\fyjr'"), "ґанокє");
}

#[test]
fn test_known_ukrainian_dictionary_has_verbs_and_apostrophes() {
    let letters = |word: &str| -> Vec<usize> {
        word.chars()
            .filter_map(|ch| letter_index(Lang::Uk, ch))
            .collect()
    };
    for word in [
        "привіт",
        "дякую",
        "м'ясо",
        "з'їзд",
        "ґанок",
        "єдність",
        "їжак",
    ] {
        assert!(known(Lang::Uk, &letters(word)), "{word}");
    }
    assert!(!known(Lang::Uk, &letters("привет")));
    assert!(!known(Lang::Uk, &letters("ршщзх")));
}

#[test]
fn test_wrong_layout_ukrainian_pair_both_directions() {
    for typed in ["ghbdsn", "lzre.", "v`zcj", "Ldsxs", "\\fyjr"] {
        assert!(wrong_layout(&keys_for(Lang::En, typed), EN_UK), "{typed}");
    }
    for typed in ["руддщ", "цщкдв", "зкщпкфь", "ершт"] {
        assert!(wrong_layout(&keys_for(Lang::Uk, typed), UK_EN), "{typed}");
    }
    for kept in ["hello", "world", "don't", "git", "ls"] {
        assert!(!wrong_layout(&keys_for(Lang::En, kept), EN_UK), "{kept}");
    }
    for kept in ["привіт", "м'ясо", "дякую", "це", "що"] {
        assert!(!wrong_layout(&keys_for(Lang::Uk, kept), UK_EN), "{kept}");
    }
    // Короткие слова: wt = «це», oj = «що»; перед исправляемым словом.
    assert!(short_wrong(&keys_for(Lang::En, "wt"), EN_UK));
    assert!(short_wrong(&keys_for(Lang::En, "oj"), EN_UK));
    assert!(short_wrong(&keys_for(Lang::En, "s"), EN_UK));
    assert!(!short_wrong(&keys_for(Lang::En, "wt"), EN_RU));
    assert!(!short_wrong(&keys_for(Lang::En, "is"), EN_UK));
}

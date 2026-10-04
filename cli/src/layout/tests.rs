use super::*;

/// Размеченный набор: split, класс, раскладка на экране, набранное, fix/keep.
const CASES: &str = include_str!("../../tests/data/layout_cases.tsv");
/// Полнота на `test` при текущих порогах - 190 из 200; запас на пересборку словарей.
const MIN_CAUGHT: usize = 185;

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
            let lang = if fields[2] == "en" {
                Lang::En
            } else {
                Lang::Ru
            };
            let keys = keys_for(lang, fields[3]);
            assert_eq!(keys.len(), fields[3].chars().count(), "{}", fields[3]);
            (fields[1], fields[4] == "fix", wrong_layout(&keys, lang))
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
    assert!(wrong_layout(&keys, Lang::En));
    assert!(!wrong_layout(&keys, Lang::Ru));
}

#[test]
fn test_wrong_layout_short_words_by_list_single_letter_never() {
    // jy = «он», шы = «is»; «on»/«он» на своём месте и одна буква f/«а» - нет.
    assert!(wrong_layout(&keys_for(Lang::En, "jy"), Lang::En));
    assert!(wrong_layout(&keys_for(Lang::Ru, "шы"), Lang::Ru));
    assert!(!wrong_layout(&keys_for(Lang::En, "on"), Lang::En));
    assert!(!wrong_layout(&keys_for(Lang::Ru, "он"), Lang::Ru));
    assert!(!wrong_layout(&keys_for(Lang::En, "F"), Lang::En));
    assert!(short_wrong(&keys_for(Lang::En, "F"), Lang::En));
    assert!(!short_wrong(&keys_for(Lang::En, "a"), Lang::En));
    assert!(short_wrong(&keys_for(Lang::En, "t`"), Lang::En));
}

#[test]
fn test_scores_digits_or_short_word_not_candidate() {
    assert!(scores(&keys_for(Lang::En, "ghb2"), Lang::En).is_none());
    assert!(scores(&keys_for(Lang::En, "yt"), Lang::En).is_none());
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

//! Оценка детектора раскладки на размеченном наборе `tests/data/layout_cases.tsv`.
//!
//! `cargo run --example layout_eval -- <split>`: доля исправленных слов по классам
//! при текущих порогах, ошибки и сетка порогов (ложные срабатывания / полнота).
//! Пороги подбираются только на `dev`; `test` - итоговый замер.
// Утилита замера: отчёт печатается, сбой разбора набора прерывает её.
#![allow(clippy::expect_used, clippy::print_stdout)]

#[allow(dead_code)]
#[path = "../src/layout.rs"]
mod layout;

use layout::{Lang, MAX_ALT_COST, MIN_MARGIN, Scores, key_char, scores, wrong_layout};
use std::collections::BTreeMap;

struct Case {
    class: String,
    typed: String,
    should_fix: bool,
    scores: Option<Scores>,
    /// Решение `wrong_layout` целиком, со списком коротких слов.
    fixed: bool,
}

/// Нажатия, которые дают `text` в раскладке `lang`.
fn keys_for(lang: Lang, text: &str) -> Vec<(u16, bool)> {
    text.chars()
        .map(|ch| {
            (2..=53)
                .flat_map(|code| [(code, false), (code, true)])
                .find(|&(code, shift)| key_char(lang, code, shift) == Some(ch))
                .expect("символ вне раскладки")
        })
        .collect()
}

fn decide(case: &Case, margin: f64, max_alt: f64) -> bool {
    case.scores
        .is_some_and(|scores| scores.should_switch(margin, max_alt))
}

fn main() {
    let split = std::env::args().nth(1).unwrap_or_else(|| "dev".into());
    let cases: Vec<Case> = include_str!("../tests/data/layout_cases.tsv")
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields[0] == split)
        .map(|fields| {
            let lang = if fields[2] == "en" {
                Lang::En
            } else {
                Lang::Ru
            };
            let keys = keys_for(lang, fields[3]);
            Case {
                class: fields[1].into(),
                typed: fields[3].into(),
                should_fix: fields[4] == "fix",
                scores: scores(&keys, lang),
                fixed: wrong_layout(&keys, lang),
            }
        })
        .collect();
    let current = |case: &Case| case.fixed;
    let mut by_class: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for case in &cases {
        let entry = by_class.entry(&case.class).or_default();
        entry.0 += 1;
        entry.1 += usize::from(current(case));
    }
    println!("split={split} margin={MIN_MARGIN} max_alt={MAX_ALT_COST}");
    for (class, (total, fixed)) in &by_class {
        println!("  {class:<12} исправлено {fixed:>3} из {total:>3}");
    }
    println!("ошибки (класс, набрано, цена экран/другая, словарь экран/другая):");
    for case in &cases {
        // Одна буква сама не исправляется по замыслу: ошибкой не считается.
        let single = case.typed.chars().count() == 1;
        if case.should_fix != current(case) && !(case.class == "short_wrong" && single) {
            match case.scores {
                Some(s) => println!(
                    "  {:<12} {:<20} {:.2}/{:.2} {}/{}",
                    case.class, case.typed, s.shown_cost, s.alt_cost, s.shown_known, s.alt_known
                ),
                None => println!("  {:<12} {:<20} не кандидат", case.class, case.typed),
            }
        }
    }
    println!("сетка: margin max_alt -> ложные (keep) / полнота ru_wrong+en_wrong");
    let long_wrong: Vec<_> = cases
        .iter()
        .filter(|case| case.class == "ru_wrong" || case.class == "en_wrong")
        .collect();
    for margin in [0.0, 0.5, 1.0, 1.5, 2.0, 3.0] {
        for max_alt in [4.0, 5.0, 6.0, 7.0, 99.0] {
            let false_fixes = cases
                .iter()
                .filter(|case| !case.should_fix && decide(case, margin, max_alt))
                .count();
            let caught = long_wrong
                .iter()
                .filter(|case| decide(case, margin, max_alt))
                .count();
            println!(
                "  {margin:>3} {max_alt:>3} -> {false_fixes:>3} / {caught}/{}",
                long_wrong.len()
            );
        }
    }
}

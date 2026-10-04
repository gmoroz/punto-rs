//! Генератор триграммных таблиц `src/layout/{en,ru}.bin` из списков слов.
//!
//! Запуск: `cli/scripts/build_layout_model.sh` (готовит словари и вызывает пример).
//! Аргументы: `<en_words> <ru_words> <out_dir>`, по слову в строке, UTF-8.
// Утилита сборки данных: ошибка ввода-вывода прерывает генерацию целиком.
#![allow(clippy::expect_used, clippy::panic, clippy::print_stdout)]

#[allow(dead_code)]
#[path = "../src/layout.rs"]
mod layout;

use layout::{COST_SCALE, Lang, bloom_bits, letter_index, symbols, table_len, trigrams};
use std::collections::HashSet;

/// Сглаживание Лидстоуна: невиданная триграмма не бесконечно дорога.
const SMOOTHING: f64 = 0.1;
/// Бит фильтра на слово: при 7 хешах около 1% ложных совпадений.
const BLOOM_BITS_PER_WORD: usize = 10;

/// Фильтр Блума по уникальным словам (индексы букв).
fn bloom(words: &HashSet<Vec<usize>>) -> Vec<u8> {
    let mut filter = vec![0_u8; words.len() * BLOOM_BITS_PER_WORD / 8 + 1];
    let bits = u64::try_from(filter.len() * 8).expect("фильтр меньше 2^64 бит");
    for word in words {
        for bit in bloom_bits(word, bits) {
            filter[usize::try_from(bit / 8).expect("бит в пределах фильтра")] |= 1 << (bit % 8);
        }
    }
    filter
}

/// Таблица триграмм и фильтр Блума словаря языка.
fn train(lang: Lang, words: &str) -> (Vec<u8>, Vec<u8>) {
    let n = symbols(lang);
    let mut counts = vec![0_u32; table_len(lang)];
    let mut used = 0_u32;
    let mut unique = HashSet::new();
    for word in words.lines() {
        let word = word.trim().to_lowercase();
        let Some(letters) = word
            .chars()
            .map(|ch| letter_index(lang, ch))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if letters.is_empty() {
            continue;
        }
        used += 1;
        for index in trigrams(lang, &letters) {
            counts[index] += 1;
        }
        unique.insert(letters);
    }
    println!("{lang:?}: {used} слов, уникальных {}", unique.len());
    let table = counts
        .chunks(n)
        .flat_map(|context| {
            let total = f64::from(context.iter().sum::<u32>());
            context.iter().map(move |&count| {
                let probability = (f64::from(count) + SMOOTHING)
                    / (total + SMOOTHING * f64::from(u32::try_from(n).expect("алфавит мал")));
                let scaled = (-probability.log2() * COST_SCALE).round().clamp(0.0, 255.0);
                // Значение уже ограничено 0..=255.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let cost = scaled as u8;
                cost
            })
        })
        .collect();
    (table, bloom(&unique))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, en, ru, out] = args.as_slice() else {
        panic!("использование: train_layout_model <en_words> <ru_words> <out_dir>");
    };
    let out = std::path::Path::new(out);
    for (lang, path, name) in [(Lang::En, en, "en"), (Lang::Ru, ru, "ru")] {
        let words = std::fs::read_to_string(path).expect("список слов не прочитан");
        let (table, filter) = train(lang, &words);
        std::fs::write(out.join(format!("{name}.bin")), table).expect("таблица не записана");
        std::fs::write(out.join(format!("{name}.bloom")), filter).expect("фильтр не записан");
    }
}

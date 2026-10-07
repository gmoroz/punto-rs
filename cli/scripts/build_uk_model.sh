#!/usr/bin/env bash
# Пересобирает src/layout/uk.{bin,bloom} из словарей Debian/Ubuntu.
# Словоформы: hunspell-uk (MPL-1.1 / LGPL-2.1+ / GPL-2+), их разворачивает
# unmunch из hunspell-tools. unmunch теряет часть глагольных форм (`дякую`),
# поэтому добавляются слова частотного списка FrequencyWords (CC BY-SA 4.0),
# которые принимает сам hunspell: так отсекается и русский мусор субтитров.
# Пакеты скачиваются без установки, root не нужен.
set -euo pipefail

freq_url="https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/uk/uk_full.txt"
cli_dir="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export LC_ALL=C.UTF-8

cd "$work"
apt-get download -qq hunspell-uk hunspell hunspell-tools
for package in ./*.deb; do
    dpkg-deb -x "$package" root
done
dict=root/usr/share/hunspell/uk_UA
letters="абвгґдеєжзиіїйклмнопрстуфхцчшщьюя"
word_re="^[$letters]+('[$letters]+)*\$"

# Апостроф в источниках бывает ’ или ʼ; в раскладке ua он ASCII.
normalize() { sed -e "s/[’ʼ]/'/g"; }

# Имена собственные и аббревиатуры (с заглавной) не нужны.
root/usr/bin/unmunch "$dict.dic" "$dict.aff" 2>/dev/null |
    normalize | grep -E "$word_re" > forms.txt || true

curl -fsSL "$freq_url" | cut -d' ' -f1 | normalize | sed 's/.*/\L&/' |
    grep -E "$word_re" | sort -u > freq.txt
root/usr/bin/hunspell -i utf-8 -d "$dict" -l < freq.txt | sort -u > rejected.txt
comm -23 freq.txt rejected.txt > accepted.txt

sort -u forms.txt accepted.txt > uk_words.txt
echo "uk: форм $(wc -l < forms.txt), из частотного списка $(wc -l < accepted.txt)," \
    "всего $(wc -l < uk_words.txt)"

cd "$cli_dir"
cargo run --quiet --locked --release --example train_layout_model -- \
    src/layout uk="$work/uk_words.txt"

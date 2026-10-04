#!/usr/bin/env bash
# Пересобирает src/layout/{en,ru}.bin из словарей Fedora.
# EN: /usr/share/dict/words (пакет words, Public Domain).
# RU: hunspell-ru (BSD), словоформы разворачивает unmunch из hunspell-devel;
# оба пакета скачиваются без установки, root не нужен.
set -euo pipefail

cli_dir="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cd "$work"
dnf download --quiet hunspell-ru hunspell-devel.x86_64
for package in ./*.rpm; do
    rpm2cpio "$package" | cpio --quiet -idm
done
./usr/bin/unmunch usr/share/hunspell/ru_RU.dic usr/share/hunspell/ru_RU.aff 2>/dev/null |
    iconv -f koi8-r -t utf-8 > ru_words.txt
grep -E '^[a-z]+$' /usr/share/dict/words > en_words.txt

cd "$cli_dir"
cargo run --quiet --locked --release --example train_layout_model -- \
    "$work/en_words.txt" "$work/ru_words.txt" src/layout

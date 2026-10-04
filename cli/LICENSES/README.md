Static builds include libdbus through the `vendored` feature of
`libdbus-sys 0.2.7`. libdbus is used under the Academic Free License 2.1
option; its upstream authors list and complete license text are preserved
in `libdbus-AUTHORS` and `libdbus-COPYING`.

The exact vendored sources, including per-file copyright notices, are available
in https://crates.io/crates/libdbus-sys/0.2.7 (the `vendor/dbus` directory).
The Rust dependency versions are pinned in `Cargo.lock`.

The layout detector embeds a modified form of the Russian hunspell dictionary
(`hunspell-ru 0.99g5`, Copyright (c) 1997-2008, Alexander I. Lebedev): its word
forms are stored as a Bloom filter and a letter trigram model in
`src/layout/ru.{bloom,bin}`, rebuilt by `scripts/build_layout_model.sh`. Its
license is in `hunspell-ru-LICENSE`. The English word list is
`/usr/share/dict/words` (public domain).

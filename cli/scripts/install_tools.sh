#!/usr/bin/env bash
# Ставит закреплённые cargo-инструменты и проверяет Cargo.lock.
set -euo pipefail

RUST_CLI_DIR="${1:?каталог CLI}"
CARGO_DENY_VERSION="${2:?версия cargo-deny}"
CARGO_MACHETE_VERSION="${3:?версия cargo-machete}"
CARGO_NEXTEST_VERSION="${4:?версия cargo-nextest}"
CARGO_LLVM_COV_VERSION="${5:?версия cargo-llvm-cov}"

install_if_needed() {
	local package="$1"
	local expected="$2"
	local actual=""

	case "$package" in
		cargo-deny)
			if command -v cargo-deny >/dev/null; then
				actual="$(cargo deny --version)"
			fi
			if [[ "$actual" == "cargo-deny $expected" ]]; then
				printf '%s\n' "$actual"
				return 0
			fi
			;;
		cargo-machete)
			if command -v cargo-machete >/dev/null; then
				actual="$(cargo machete --version)"
			fi
			if [[ "$actual" == "$expected" ]]; then
				printf '%s\n' "$actual"
				return 0
			fi
			;;
		cargo-nextest)
			if command -v cargo-nextest >/dev/null; then
				actual="$(cargo nextest --version | head -n1)"
			fi
			if [[ "$actual" == cargo-nextest\ "$expected"* ]]; then
				printf '%s\n' "$actual"
				return 0
			fi
			;;
		cargo-llvm-cov)
			if command -v cargo-llvm-cov >/dev/null; then
				actual="$(cargo llvm-cov --version)"
			fi
			if [[ "$actual" == "cargo-llvm-cov $expected" ]]; then
				printf '%s\n' "$actual"
				return 0
			fi
			;;
		*)
			printf 'rust-cli-install: неизвестный пакет %s\n' "$package" >&2
			exit 2
			;;
	esac

	cargo install --locked --force "${package}@${expected}"
}

install_if_needed cargo-deny "$CARGO_DENY_VERSION"
install_if_needed cargo-machete "$CARGO_MACHETE_VERSION"
install_if_needed cargo-nextest "$CARGO_NEXTEST_VERSION"
install_if_needed cargo-llvm-cov "$CARGO_LLVM_COV_VERSION"

cd "$RUST_CLI_DIR"
# rust-toolchain.toml подтянет channel и components при первом cargo.
cargo fetch --locked
printf 'rust-cli-install: зависимости установлены по Cargo.lock\n'

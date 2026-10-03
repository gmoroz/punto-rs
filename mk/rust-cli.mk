RUST_CLI_DIR := cli
RUST_COVERAGE_LINES := 80
CARGO_DENY_VERSION := 0.20.2
CARGO_MACHETE_VERSION := 0.9.2
CARGO_NEXTEST_VERSION := 0.9.146
CARGO_LLVM_COV_VERSION := 0.9.1

INSTALL_TARGETS += rust-cli-install
FORMAT_TARGETS += rust-cli-format
LINT_TARGETS += rust-cli-lint
TEST_TARGETS += rust-cli-test
ROOT_ALLOWED += cli .copier-answers-rust-cli.yml

.PHONY: rust-cli-install rust-cli-format rust-cli-lint rust-cli-test

rust-cli-install: ## Установить Rust-инструменты и зависимости по Cargo.lock
	@bash "$(RUST_CLI_DIR)/scripts/install_tools.sh" \
		"$(RUST_CLI_DIR)" \
		"$(CARGO_DENY_VERSION)" \
		"$(CARGO_MACHETE_VERSION)" \
		"$(CARGO_NEXTEST_VERSION)" \
		"$(CARGO_LLVM_COV_VERSION)"

rust-cli-format: ## Отформатировать Rust CLI
	@cd "$(RUST_CLI_DIR)" && cargo fmt --all

rust-cli-lint: ## Проверить fmt, clippy, deny и machete
	@cd "$(RUST_CLI_DIR)" && cargo fmt --all --check
	@cd "$(RUST_CLI_DIR)" && cargo clippy --locked --all-targets -- -D warnings
	@cd "$(RUST_CLI_DIR)" && cargo deny --locked check advisories licenses bans
	@cd "$(RUST_CLI_DIR)" && cargo machete

# lint пишет target/debug, llvm-cov — target/llvm-cov-target. На CI (14 GB SSD)
# нельзя держать оба: сначала lint, перед coverage сносим debug, после — llvm-cov-target.
rust-cli-test: rust-cli-lint ## Запустить nextest и coverage
	@set -euo pipefail; \
	cd "$(RUST_CLI_DIR)"; \
	cargo nextest run --locked; \
	target_dir="$${CARGO_TARGET_DIR:-target}"; \
	if [ "$${CI:-}" = "true" ]; then \
		rm -rf "$$target_dir/debug"; \
	fi; \
	cargo llvm-cov nextest --locked --fail-under-lines "$(RUST_COVERAGE_LINES)"; \
	if [ "$${CI:-}" = "true" ]; then \
		rm -rf "$$target_dir/llvm-cov-target"; \
	fi

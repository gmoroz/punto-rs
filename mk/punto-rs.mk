# Проектные цели punto-rs поверх кубика rust-cli.
ROOT_ALLOWED += .github LICENSE
TEST_TARGETS += punto-rs-install-test

.PHONY: punto-rs-install-test deploy

punto-rs-install-test: ## Проверить установщик: повторная установка сохраняет конфиг
	@cd "$(RUST_CLI_DIR)" && cargo build --locked
	@sh "$(RUST_CLI_DIR)/scripts/test-install.sh"

deploy: ## Собрать release, установить для пользователя и перезапустить сервис
	@cd "$(RUST_CLI_DIR)" && cargo build --release --locked && sh scripts/install.sh target/release/punto-rs
	@systemctl --user daemon-reload
	@systemctl --user restart punto-rs
	@systemctl --user is-active punto-rs
	@journalctl --user -u punto-rs -n 5 --no-pager -o cat

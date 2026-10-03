# Проектные цели punto-rs поверх кубика rust-cli.
ROOT_ALLOWED += .github LICENSE
TEST_TARGETS += punto-rs-install-test

.PHONY: punto-rs-install-test

punto-rs-install-test: ## Проверить установщик: повторная установка сохраняет конфиг
	@cd "$(RUST_CLI_DIR)" && cargo build --locked
	@sh "$(RUST_CLI_DIR)/scripts/test-install.sh"

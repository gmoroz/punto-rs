.DEFAULT_GOAL := help
SHELL := /usr/bin/env bash

INSTALL_TARGETS :=
FORMAT_TARGETS :=
LINT_TARGETS := check-root check-file-lines check-no-skips check-no-markers check-agents check-jscpd
TEST_TARGETS :=
ROOT_ALLOWED := .copier-answers-core.yml
CODE_FILE_LIMIT := 400
JSCPD_VERSION := 5.3.3

-include mk/*.mk

.PHONY: install format lint test full-check push help e2e-test-complete \
	$(INSTALL_TARGETS) $(FORMAT_TARGETS) $(LINT_TARGETS) $(TEST_TARGETS)

help: ## Показать доступные команды
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\\n", $$1, $$2}'

install: $(INSTALL_TARGETS) ## Подключить git hooks
	@git config core.hooksPath .githooks
	@printf 'Git hooks подключены.\n'

format: $(FORMAT_TARGETS) ## Отформатировать код

lint: $(LINT_TARGETS) ## Выполнить быстрые проверки

test: $(TEST_TARGETS) ## Выполнить тесты

# Барьер пуст без кубика e2e; deploy-smoke ждёт завершения e2e, если он есть.
e2e-test-complete:

full-check: ## Выполнить lint и test, затем сохранить проверенный SHA
	@$(MAKE) --no-print-directory -j lint test
	@# Маркер только для чистого дерева: иначе проверенный код не совпадает с коммитом, который уйдёт в push.
	@if [ -n "$$(git status --porcelain)" ]; then \
		printf 'full-check: OK (дерево не чистое, маркер для push не записан)\n'; \
	else \
		git rev-parse HEAD > "$$(git rev-parse --git-dir)/full-check-ok"; \
		printf 'full-check: OK\n'; \
	fi

push: ## Проверить чистоту, выполнить full-check и отправить текущую ветку
	@if [ -n "$$(git status --porcelain)" ]; then printf 'push: рабочее дерево не чистое.\n' >&2; exit 1; fi
	@$(MAKE) --no-print-directory full-check
	@# ponytail: remote всегда origin; другой remote -> параметр, когда появится второй.
	@git push --set-upstream origin HEAD

check-root:
	@bash scripts/check_root.sh $(ROOT_ALLOWED)

check-file-lines:
	@bash scripts/check_file_lines.sh $(CODE_FILE_LIMIT) scripts/file-line-exceptions.txt

check-no-skips:
	@bash scripts/check_no_skips.sh

check-no-markers:
	@bash scripts/check_no_markers.sh

check-agents:
	@bash scripts/check_agents.sh

check-jscpd:
	@bash scripts/check_jscpd.sh $(JSCPD_VERSION)

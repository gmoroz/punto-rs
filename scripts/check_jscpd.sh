#!/usr/bin/env bash
set -euo pipefail

version="$1"
# jscpd по умолчанию уважает .gitignore; .git в ignore — он обычно не в .gitignore.
exec npx --yes "jscpd@$version" --min-lines 10 --min-tokens 50 --threshold 0 --fail-on-empty --format "bash,python,typescript,javascript,jsx,tsx,rust" --ignore "**/.git/**" .

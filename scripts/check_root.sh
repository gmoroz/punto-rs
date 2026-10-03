#!/usr/bin/env bash
set -euo pipefail

allowed=(
    AGENTS.md CLAUDE.md README.md Makefile .editorconfig .gitignore
    .githooks scripts mk .claude .git
)
allowed+=("$@")
failed=0
declare -A seen=()

is_allowed() {
    local entry="$1"
    local allowed_entry
    for allowed_entry in "${allowed[@]}"; do
        [ "$entry" = "$allowed_entry" ] && return 0
    done
    return 1
}

while IFS= read -r -d '' path; do
    entry="${path%%/*}"
    [ -n "${seen[$entry]:-}" ] && continue
    seen["$entry"]=1
    if ! is_allowed "$entry"; then
        printf 'check-root: неожиданный элемент в корне: %s\n' "$entry" >&2
        failed=1
    fi
done < <(git ls-files -z --cached --others --exclude-standard)

exit "$failed"

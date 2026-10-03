#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
failed=0

while IFS= read -r -d '' agents_file; do
    case "$agents_file" in
        AGENTS.md|*/AGENTS.md) ;;
        *) continue ;;
    esac
    lines="$(wc -l < "$agents_file")"
    if (( lines > 200 )); then
        printf 'check-agents: %s содержит %s строк, лимит 200.\n' "$agents_file" "$lines" >&2
        failed=1
    fi
    while IFS= read -r quoted_path; do
        path="${quoted_path:1:-1}"
        case "$path" in
            make|*' '*|*'('*|*'='*|*'<'*|*'>'*) continue ;;
        esac
        # Генерируемые пути в .gitignore отсутствуют в чистом checkout до генерации.
        if [ ! -e "$path" ] && ! git check-ignore -q "$path"; then
            printf 'check-agents: путь %s из %s не существует.\n' "$path" "$agents_file" >&2
            failed=1
        fi
    done < <(grep -oE '`[^`]+`' "$agents_file" || true)
done < <(bash "$script_dir/git_files.sh")

exit "$failed"

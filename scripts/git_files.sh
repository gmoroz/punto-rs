#!/usr/bin/env bash
# Печатает NUL-разделённые пути: отслеживаемые и новые неигнорируемые.
# Аргументы — расширения без точки; без аргументов печатает все пути.
set -euo pipefail

git ls-files -z --cached --others --exclude-standard | while IFS= read -r -d '' path; do
    if [ "$#" -eq 0 ]; then
        printf '%s\0' "$path"
        continue
    fi
    for ext in "$@"; do
        case "$path" in
            *."$ext")
                printf '%s\0' "$path"
                break
                ;;
        esac
    done
done

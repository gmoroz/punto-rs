#!/usr/bin/env bash
set -euo pipefail

limit="$1"
exceptions_file="$2"
script_dir="$(cd "$(dirname "$0")" && pwd)"
declare -A exceptions
failed=0

while IFS=' ' read -r path exception_limit; do
    [ -z "$path" ] && continue
    [[ "$path" == \#* ]] && continue
    if [ -z "$exception_limit" ] || ! [[ "$exception_limit" =~ ^[0-9]+$ ]] || (( exception_limit >= limit )); then
        printf 'check-file-lines: исключение %s должно быть меньше общего лимита %s.\n' "$path" "$limit" >&2
        exit 1
    fi
    exceptions["$path"]="$exception_limit"
done < "$exceptions_file"

while IFS= read -r -d '' file; do
    file_limit="${exceptions[$file]:-$limit}"
    lines="$(wc -l < "$file")"
    if (( lines > file_limit )); then
        printf 'check-file-lines: %s содержит %s строк, лимит %s.\n' "$file" "$lines" "$file_limit" >&2
        failed=1
    fi
done < <(bash "$script_dir/git_files.sh" py rs js jsx ts tsx sh)

exit "$failed"

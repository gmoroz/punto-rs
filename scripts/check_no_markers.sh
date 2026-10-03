#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
markers='TO'"DO|FI"'XME|HA'"CK|XX"'X'
failed=0

while IFS= read -r -d '' file; do
    matches="$(grep -nE "$markers" "$file" || true)"
    if [ -n "$matches" ]; then
        printf 'check-no-markers: запрещённая метка в %s\n%s\n' "$file" "$matches" >&2
        failed=1
    fi
done < <(bash "$script_dir/git_files.sh" py rs js jsx ts tsx sh)

exit "$failed"

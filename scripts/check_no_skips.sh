#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
pattern='pytest\.mark\.(skip|skipif|xfail)\b|pytest\.skip\(|#\[ignore\]|\b(describe|it|test|suite|context)\.(skip|only)\(|\bxit\(|\bxdescribe\('
failed=0

while IFS= read -r -d '' file; do
    if matches="$(grep -nE "$pattern" "$file" || true)"; then
        [ -z "$matches" ] || {
            printf 'check-no-skips: пропущенный тест в %s\n%s\n' "$file" "$matches" >&2
            failed=1
        }
    fi
done < <(bash "$script_dir/git_files.sh" py rs js jsx ts tsx)

exit "$failed"

#!/bin/bash
set -euo pipefail

PROJECT_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

check_top_level_test_target_duplicates() {
    local test_dir="${STYLE_TEST_DIR:-tests}"
    local mod_file="$PROJECT_ROOT/$test_dir/mod.rs"
    local failures=0
    local hit
    local line
    local module_name
    local target_file
    local rel_target

    [ -f "$mod_file" ] || return 0

    while IFS= read -r hit; do
        [ -n "$hit" ] || continue
        line="${hit%%:*}"
        module_name="${hit#*:}"
        target_file="$PROJECT_ROOT/$test_dir/$module_name.rs"
        [ -f "$target_file" ] || continue

        rel_target="$test_dir/$module_name.rs"
        printf "error: %s:%s: top-level integration test '%s' is already a Cargo test target; remove this mod declaration\n" \
            "$test_dir/mod.rs" \
            "$line" \
            "$rel_target"
        failures=$((failures + 1))
    done < <(
        awk '
            /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/ {
                line = $0
                sub(/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+/, "", line)
                sub(/[[:space:]]*;.*/, "", line)
                print FNR ":" line
            }
        ' "$mod_file"
    )

    if [ "$failures" -gt 0 ]; then
        echo "Rust project style checks failed with $failures duplicate test target issue(s)."
        exit 1
    fi
}

check_top_level_test_target_duplicates
exec env RS_CI_PROJECT_ROOT="$PROJECT_ROOT" "$PROJECT_ROOT/.rs-ci/style-check.sh" "$@"

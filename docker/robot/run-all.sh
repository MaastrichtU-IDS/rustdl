#!/usr/bin/env bash
# Walk every fixture in crates/owl-dl-bench/fixtures, ask the ROBOT
# oracle for its verdict, and compare against the manifest's expected
# value. Exits 0 if every fixture agrees, 1 otherwise.
#
# Usage:  docker/robot/run-all.sh [fixtures-dir]
#
# Output (one line per fixture):
#   01_atomic_sat.ofn          expected=sat  oracle=sat  OK
#   02_and_not_a_unsat.ofn     expected=unsat oracle=unsat OK
#   ...
#
# Summary printed at the end. Useful as a smoke test that all fixtures
# parse, that the Docker image is reachable, and that HermiT agrees
# with our pinned expectations *before* we hook rustdl in.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FIXTURES_DIR="${1:-$REPO_ROOT/crates/owl-dl-bench/fixtures}"
MANIFEST="$FIXTURES_DIR/manifest.toml"
ORACLE="$REPO_ROOT/docker/robot/oracle.sh"

if [[ ! -f "$MANIFEST" ]]; then
    echo "manifest not found: $MANIFEST" >&2
    exit 2
fi
if [[ ! -x "$ORACLE" ]]; then
    echo "oracle script not executable: $ORACLE" >&2
    exit 2
fi

pass=0
fail=0

# Parse the manifest line by line. A real Rust binary will replace this
# shell parser later; for now we read `file = "…"`, `test_class = "…"`,
# `expected = "…"` triplets in order.
current_file=""
current_iri=""
current_expected=""
flush() {
    if [[ -n "$current_file" && -n "$current_iri" && -n "$current_expected" ]]; then
        local oracle_verdict
        oracle_verdict="$("$ORACLE" "$FIXTURES_DIR/$current_file" "$current_iri" 2>/dev/null)"
        if [[ "$oracle_verdict" == "$current_expected" ]]; then
            printf '%-40s expected=%-5s oracle=%-5s OK\n' \
                "$current_file" "$current_expected" "$oracle_verdict"
            pass=$((pass + 1))
        else
            printf '%-40s expected=%-5s oracle=%-5s MISMATCH\n' \
                "$current_file" "$current_expected" "$oracle_verdict"
            fail=$((fail + 1))
        fi
    fi
    current_file=""
    current_iri=""
    current_expected=""
}

while IFS= read -r line; do
    case "$line" in
        "[[fixture]]")
            flush
            ;;
        *file*=*)
            current_file="$(echo "$line" | sed -E 's/^[^"]*"([^"]*)".*/\1/')"
            ;;
        *test_class*=*)
            current_iri="$(echo "$line" | sed -E 's/^[^"]*"([^"]*)".*/\1/')"
            ;;
        *expected*=*)
            current_expected="$(echo "$line" | sed -E 's/^[^"]*"([^"]*)".*/\1/')"
            ;;
    esac
done < "$MANIFEST"
flush

echo
echo "summary: $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]

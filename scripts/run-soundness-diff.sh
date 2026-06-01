#!/usr/bin/env bash
# Run the full closure-diff soundness net (all corpus + ORE fixtures).
# Requires fixtures already present under ontologies/{real,external}/.
# The #[ignore]d tests in crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
# each assert FP=0 individually; this script just invokes them all in release
# mode with --nocapture so the per-fixture harness lines are visible.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture

#!/usr/bin/env bash
# Run the full closure-diff soundness net (all corpus + ORE fixtures).
#
# This is rustdl's FP=0 gate: each test diffs the classification closure against a
# committed Konclude/HermiT oracle (`ontologies/**/*-classified.owx`) and asserts
# FP=0. The tests are `#[ignore]`d because the fixtures are large and gitignored;
# this script is the sanctioned entrypoint.
#
# RUN THIS LOCALLY BEFORE MERGING any change to the fragment gates
# (`is_pure_el` / `saturator_complete_fragment` in owl-dl-reasoner/src/classify.rs),
# to unsat derivation in owl-dl-saturation, or to conversion/normalization in
# owl-dl-core. The CI job of the same name is `workflow_dispatch`-only and its
# fixtures are never provisioned, so CI being green does NOT imply FP=0.
#
# Fixtures: `./scripts/fetch-real-ontologies.sh`. A missing REQUIRED fixture now
# FAILS with the path and that hint (it used to pass silently — of 22 fixture
# blocks, 9 could skip while still reporting `ok`).
#
# Coverage report: every test prints one `[fp0]` line saying what it verified, so
#   ./scripts/run-soundness-diff.sh 2>&1 | grep '^\[fp0\]'
# is a complete VERIFIED / NOT VERIFIED manifest for the run.
#
# `--test-threads=1` is deliberate: the `[fp0]` lines go to stderr, and parallel
# test threads interleave their writes mid-line, which corrupts exactly the report
# this script exists to produce. Serial costs ~2x wall (~2 min → ~4 min, `wine`
# alone is ~65 s) and is worth it for a gate run by hand.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- \
    --ignored --nocapture --test-threads=1

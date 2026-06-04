"""Soundness regression — Python bindings preserve FP=0 vs Konclude on alehif.

Mirrors crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
::alehif_closure_matches_konclude, but invokes through the Python
bindings. If this test fails, the bindings have introduced a data
corruption between Rust output and the Python return value — STOP
and investigate before releasing.
"""

import pathlib
import pytest
import rustdl

OWL_THING = "http://www.w3.org/2002/07/owl#Thing"
OWL_NOTHING = "http://www.w3.org/2002/07/owl#Nothing"


@pytest.mark.skipif(
    not (pathlib.Path(__file__).resolve().parents[4]
         / "ontologies" / "external" / "alehif-test.ofn").exists(),
    reason="ontologies corpus not fetched (run scripts/fetch-real-ontologies.sh)",
)
def test_alehif_closure_size_through_python(fixtures_dir):
    repo_root = pathlib.Path(__file__).resolve().parents[4]
    onto = repo_root / "ontologies" / "external" / "alehif-test.ofn"
    result = rustdl.classify(str(onto))

    # Count non-trivial subsumption pairs (no owl:Thing/Nothing, no reflexive).
    classes = [
        c for c in result.classes
        if c not in (OWL_THING, OWL_NOTHING)
    ]
    pair_count = sum(
        1
        for sub in classes
        for sup in classes
        if sub != sup and result.is_subclass(sub, sup)
    )

    # alehif's Konclude-confirmed closure = 247 pairs (per docs/perf-2026-06-04).
    # If this drifts, EITHER the Python bindings dropped data OR the
    # Rust-side closure shifted (the corpus closure-diff test guards that;
    # check it before assuming Python is at fault).
    assert pair_count == 247, (
        f"alehif closure through Python = {pair_count}; expected 247. "
        "Either bindings broken OR Rust closure drifted "
        "(check `cargo test -p owl-dl-reasoner --release --test "
        "konclude_closure_diff alehif`)."
    )

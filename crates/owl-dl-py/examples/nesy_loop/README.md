# nesy_loop — LLM-assisted OWL authoring demo

A minimal neurosymbolic demo on top of `rustdl`'s Python bindings: an LLM proposes
OWL axioms, `rustdl` gates each proposal with `classify`/`is_consistent` and
returns `justify`/`diagnose`/`repair` feedback on failure, and the LLM revises.
All non-LLM logic is deterministic and unit-tested; a real transcript is
produced by one run against the live Anthropic API.

This example is a front-end demo only — it does not modify any Rust crate.

## Requirements

- Python >= 3.10 (matches `crates/owl-dl-py/pyproject.toml`)
- A Rust toolchain able to run `cargo` (see the build note below)
- `ANTHROPIC_API_KEY` set in the environment, only if you want to run the loop
  against the real LLM (`AnthropicLLM`). Everything else — build, tests, the
  scripted/offline loop, replay — works without it.

## Build

The `rustdl` package is built from `crates/owl-dl-py` via
[maturin](https://www.maturin.rs/). Build note: the repo's pinned toolchain
(`rust-toolchain.toml`, currently 1.95.0) may be missing the `cargo` binary on
some hosts; if `maturin develop` fails with `cargo not applicable to 1.95.0`,
force the `stable` toolchain instead:

```bash
cd ~/code/rustdl/crates/owl-dl-py
python3 -m venv .venv && source .venv/bin/activate
pip install -U maturin pytest anthropic
# or: pip install -r examples/nesy_loop/requirements.txt maturin
RUSTUP_TOOLCHAIN=stable maturin develop --release
```

Expected: builds `rustdl._native` and ends with
`📦 Built ... 🛠 Installed rustdl`. The release build takes a few minutes —
that's expected.

Smoke-test:

```bash
python -c "import rustdl; print(sorted(n for n in dir(rustdl) if not n.startswith('_'))[:6])"
```

should list public API including `classify`, `diagnose`, `is_consistent`,
`justify`, `repair`.

## Run the tests

All tests use `ScriptedLLM` (no network, fully deterministic) and exercise the
real `rustdl` reasoner. From this directory, with the venv active:

```bash
cd ~/code/rustdl/crates/owl-dl-py/examples/nesy_loop
PYTHONPATH=.. pytest -v
```

## Run the real loop

With `ANTHROPIC_API_KEY` set and the venv active:

```bash
cd ~/code/rustdl/crates/owl-dl-py/examples/nesy_loop
PYTHONPATH=.. python -m nesy_loop.run --n-edits 8 --max-revisions 2 --out out
```

This prints a Markdown metrics table (edits proposed / edits with a clash /
fixed after repair / edits malformed / residual new-unsat) and writes
`out/transcript.jsonl` (one JSON object per turn) and `out/metrics.md`.

### Deterministic replay

A captured real run's axioms can be replayed offline (no API key, no
network) via `fixtures/recorded_replies.json` (a JSON list of the exact
axioms the LLM produced):

```bash
PYTHONPATH=.. python -m nesy_loop.run --scripted fixtures/recorded_replies.json --out out
```

This should reproduce the same metrics as the original captured run.

## Layout

- `gate.py` — apply an edit, classify with `rustdl`, format failure feedback.
- `llm.py` — `LLM` protocol, `ScriptedLLM` (tests/replay), `AnthropicLLM` (real run).
- `loop.py` — orchestrates propose -> gate -> revise, tracks metrics.
- `run.py` — CLI entry point (`python -m nesy_loop.run`), writes transcript + metrics.
- `fixtures/seed.ofn` — clean seed ontology (pizza toy domain).
- `tests/` — pytest suite (run with `PYTHONPATH=..`).
- `out/` — captured transcript + metrics from a real run (Task 7).

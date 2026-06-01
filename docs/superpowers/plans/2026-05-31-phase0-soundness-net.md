# Phase 0 — Soundness Net + Fragment Characterization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reproducible reference-classification pipeline and broaden the Konclude/HermiT closure-diff harness with inverse + cardinality + role-hierarchy ontologies, then write the fragment-completeness statement that justifies `trust_sat` being default-on.

**Architecture:** A new `docker/robot/classify-oracle.sh` turns any `.ofn` into a reference `*-classified.owx` using ROBOT's embedded HermiT (sound + complete). New ontologies are fetched, data-property-stripped, classified into `ontologies/external/`, and wired into the existing `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` harness as `#[ignore]`d FP=0 regression tests. A prose deliverable in `docs/` states the fragment on which the hyper engine is provably complete.

**Tech Stack:** Rust (edition 2024), `horned-owl` parser, ROBOT v1.9.6 + HermiT via Docker, bash.

---

## Background the executor needs

- The closure-diff harness is `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`. It reads a reference classification from an OWL/XML `*-classified.owx`, takes the transitive closure of its atomic `SubClassOf` / `EquivalentClasses` edges, classifies the same ontology with rustdl via `classify_top_down_with_timeout`, and reports `FP` (rustdl-only pairs — must be 0) and `MISSED` (reference-only pairs). The reusable helper is `diff_corpus_ontology(label, input, truth, per_pair_ms) -> (rustdl, konclude, fp, missed)` at `konclude_closure_diff.rs:214`.
- Each ontology gets a `#[test] #[ignore = "..."]` function that points at an `input.ofn` and a `truth.owx` under `../../ontologies/...`, calls `diff_corpus_ontology`, and asserts `fp == 0`. Existing examples: `galen_closure_matches_konclude` (`konclude_closure_diff.rs:284`), `notgalen_closure_matches_konclude` (`:310`).
- The fixture directories `ontologies/real/` and `ontologies/external/` are gitignored; fixtures are reproduced from upstream, never vendored. Existing reference files live in `ontologies/external/*-classified.owx`.
- `owl-dl-core` hard-rejects data-property/datatype axioms (`crates/owl-dl-core/src/convert.rs:297`). Real ontologies must be stripped with ROBOT first — the canonical recipe is in `docs/real-ontology-corpus.md` ("Caveat: data-property stripping").
- The in-tree oracle `docker/robot/oracle.sh` only emits a single `sat`/`unsat` verdict for one class; it does **not** emit a full classification. Task 1 fills that gap.
- Docker is available (`docker --version` → 29.x). The ROBOT image is `obolibrary/robot:v1.9.6`. Lab machines route HTTP(S) through `proxy.unimaas.nl:3128` (already in the environment for `curl`/`docker`).

---

## Task 1: Reference-classification oracle script

**Files:**
- Create: `docker/robot/classify-oracle.sh`
- Reference (do not modify): `docker/robot/oracle.sh`, `docs/real-ontology-corpus.md`

- [ ] **Step 1: Write the oracle script**

Create `docker/robot/classify-oracle.sh`:

```bash
#!/usr/bin/env bash
# Produce a reference classification of an OWL ontology using ROBOT's
# embedded HermiT (sound + complete), in the OWL/XML shape the closure-
# diff harness consumes (crates/owl-dl-reasoner/tests/konclude_closure_diff.rs).
#
# Usage: classify-oracle.sh <input.ofn> <output-classified.owx>
#
# The output contains the asserted axioms plus HermiT-inferred direct
# SubClassOf / EquivalentClasses axioms. The harness takes the transitive
# closure, so direct inferred edges are sufficient.
#
# Pinning: defaults to obolibrary/robot:v1.9.6; override with ROBOT_IMAGE.

set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <input.ofn> <output-classified.owx>" >&2
    exit 2
fi

INPUT="$1"
OUTPUT="$2"

if [[ ! -f "$INPUT" ]]; then
    echo "input not found: $INPUT" >&2
    exit 2
fi

ROBOT_IMAGE="${ROBOT_IMAGE:-obolibrary/robot:v1.9.6}"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cp "$INPUT" "$TMPDIR/in.ofn"

# `reason` runs HermiT and adds inferred axioms; we ask explicitly for
# the class hierarchy generators and emit OWL/XML so the harness's
# read_owx() can parse it. ROBOT may exit non-zero if the ontology is
# inconsistent — surface that rather than silently producing nothing.
docker run --rm \
    -v "$TMPDIR:/work" \
    -w /work \
    "$ROBOT_IMAGE" \
    robot reason \
        --reasoner hermit \
        --axiom-generators "subclass equivalent" \
        --input in.ofn \
        --output out.owx

cp "$TMPDIR/out.owx" "$OUTPUT"
echo "wrote $OUTPUT" >&2
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x docker/robot/classify-oracle.sh`
Expected: no output, exit 0.

- [ ] **Step 3: Validate the oracle reproduces an EXISTING reference**

This is the correctness gate: run the new oracle on an ontology that already has a checked-in reference, and confirm the harness reports FP=0 (i.e. our oracle agrees with whatever produced the original truth). ALEHIF+ is small (168 classes, ~2 s) and already at 100% in the corpus.

Run:
```bash
test -f ontologies/external/alehif-test.ofn || echo "MISSING alehif-test.ofn — fetch fixtures first"
docker/robot/classify-oracle.sh ontologies/external/alehif-test.ofn /tmp/alehif-ours.owx
```
Expected: stderr ends with `wrote /tmp/alehif-ours.owx`; the file exists and contains `SubClassOf` and/or `EquivalentClasses` elements (verify with `grep -c "SubClassOf\|EquivalentClasses" /tmp/alehif-ours.owx` → a positive count).

- [ ] **Step 4: Diff our reference against the checked-in reference**

Temporarily point the harness at our generated file by copying it over a scratch name and running an ad-hoc comparison. Run:
```bash
grep -oE 'IRI="[^"]+"' /tmp/alehif-ours.owx | sort -u | wc -l
grep -oE 'IRI="[^"]+"' ontologies/external/alehif-test-classified.owx | sort -u | wc -l
```
Expected: the two IRI counts are within a few of each other (HermiT and the original reasoner classify the same hierarchy). A large divergence means `--axiom-generators` is wrong — re-check Step 1.

- [ ] **Step 5: Commit**

```bash
git add docker/robot/classify-oracle.sh
git commit -m "feat(oracle): reference-classification script (ROBOT+HermiT -> classified.owx)"
```

---

## Task 2: Select inverse + cardinality + role-hierarchy candidate ontologies

**Files:**
- Create: `docs/phase0-corpus-candidates.md`
- Reference: `ontologies/external/ore2015_sample.zip` (contains `pool_sample/files/ore_ont_*.owl` + `pool_sample/dl/classification/metadata.csv`)

- [ ] **Step 1: List the ORE DL-classification ontologies and their expressivity**

The metadata CSV records each ontology's DL expressivity string. Run:
```bash
cd ontologies/external
unzip -o ore2015_sample.zip 'pool_sample/dl/classification/metadata.csv' -d /tmp/ore >/dev/null
head -1 /tmp/ore/pool_sample/dl/classification/metadata.csv
```
Expected: a CSV header naming columns (look for an expressivity/DL column and a filename column).

- [ ] **Step 2: Filter for inverse + cardinality + role-hierarchy profiles**

We want expressivity strings containing **I** (inverse), a cardinality marker (**Q** or **N**), and **H** or **R** (role hierarchy / complex roles) — the exact interaction that historically produced the SIO false positives. Run:
```bash
grep -iE 'S?[HR].*I.*[QN]|S?[HR].*[QN].*I' /tmp/ore/pool_sample/dl/classification/metadata.csv | head -20
```
Expected: a handful of rows. Note their ontology filenames (e.g. `ore_ont_NNNNN.owl`) and DL strings.

- [ ] **Step 3: Pick 2–3 small candidates and record them**

Prefer the smallest matching ontologies (cross-reference file sizes from `unzip -l ontologies/external/ore2015_sample.zip`) so each diff finishes in minutes, not hours. Create `docs/phase0-corpus-candidates.md` recording, for each chosen ontology: the ORE filename, its DL expressivity string, its approximate class count and file size, and one sentence on why it stresses the inverse+cardinality+role-hierarchy interaction. If fewer than 2 match, note that and fall back to the largest inverse+role-hierarchy (SHI/SRI) ontologies available.

- [ ] **Step 4: Commit**

```bash
git add docs/phase0-corpus-candidates.md
git commit -m "docs(phase0): select inverse+cardinality+role-hierarchy corpus candidates"
```

---

## Task 3: Fetch, strip, and classify the chosen ontologies

**Files:**
- Modify (extend, do not break existing entries): none required — work directly in `ontologies/external/`
- Reference: `docs/real-ontology-corpus.md` (stripping recipe), `docker/robot/classify-oracle.sh` (Task 1)

For each chosen ontology `<slug>` from Task 2 (repeat these steps per ontology — do not batch them, so a failure localizes):

- [ ] **Step 1: Extract the source from the ORE sample**

```bash
cd ontologies/external
unzip -o ore2015_sample.zip 'pool_sample/files/ore_ont_NNNNN.owl' -d /tmp/ore >/dev/null
cp /tmp/ore/pool_sample/files/ore_ont_NNNNN.owl ./<slug>.owl
```
(Replace `ore_ont_NNNNN.owl` and `<slug>` with the Task-2 values.)

- [ ] **Step 2: Strip data properties and convert to OFN**

```bash
docker run --rm -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
    robot remove --input <slug>.owl --select data-properties --signature true --trim true \
                 convert --format ofn --output <slug>.ofn
sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' <slug>.ofn
```
Expected: `<slug>.ofn` exists and is non-empty (`test -s <slug>.ofn`).

- [ ] **Step 3: Confirm rustdl can parse it**

```bash
cargo build -p owl-dl-cli --release 2>/dev/null
./target/release/rustdl classify --saturation-only ontologies/external/<slug>.ofn | head -5
```
Expected: it prints classification stats, not a parse error. A parse error means a residual datatype/data-property axiom survived stripping — re-run Step 2 inspecting which axiom shape remains.

- [ ] **Step 4: Generate the reference classification**

```bash
docker/robot/classify-oracle.sh ontologies/external/<slug>.ofn ontologies/external/<slug>-classified.owx
```
Expected: `wrote ontologies/external/<slug>-classified.owx`.

- [ ] **Step 5: Commit the doc note (fixtures themselves are gitignored)**

Append the new ontology to the inventory table in `docs/real-ontology-corpus.md` (slug, source = "ORE 2015 sample `ore_ont_NNNNN.owl`", expressivity, why). Then:
```bash
git add docs/real-ontology-corpus.md
git commit -m "docs(corpus): add <slug> (ORE inverse+cardinality+role-hierarchy fixture)"
```

---

## Task 4: Wire each new ontology into the closure-diff harness

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` (append new test functions after `notgalen_closure_matches_konclude`, before `corpus_closure_long_timeout`)

For each chosen `<slug>`:

- [ ] **Step 1: Add the regression test (write it before running)**

Append (substituting `<slug>` and a Rust-safe `<slug_ident>`, e.g. `ore_10009`):

```rust
#[test]
#[ignore = "needs ontologies/external/<slug>.ofn + <slug>-classified.owx; ORE inverse+cardinality+role-hierarchy soundness fixture"]
fn <slug_ident>_closure_matches_konclude() {
    let input = Path::new("../../ontologies/external/<slug>.ofn");
    let truth = Path::new("../../ontologies/external/<slug>-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing <slug> fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("<slug>", input, truth, 200);
    assert_eq!(fp, 0, "<slug> has FPs — soundness regression");
}
```

- [ ] **Step 2: Confirm it compiles**

Run: `cargo test -p owl-dl-reasoner --test konclude_closure_diff --no-run`
Expected: compiles with no errors (the test is `#[ignore]`d, so it won't execute yet).

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git commit -m "test(corpus): closure diff for <slug> (FP=0 soundness gate)"
```

---

## Task 5: Run the new diffs and record the result

**Files:**
- Create: `docs/phase0-soundness-results.md`

- [ ] **Step 1: Run every new diff with output captured**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture 2>&1 | tee /tmp/phase0-diff.log
```
Expected: each new `*_closure_matches_konclude` test prints a `--- <slug> (… s) --- rustdl_closure=… konclude_closure=… FP=… MISSED=…` line.

- [ ] **Step 2: Triage the FP column — this is the whole point**

- If **every new ontology reports FP=0**: the empirical soundness envelope is broadened — record it.
- If **any reports FP>0**: that is a genuine soundness bug discovered (a real Phase-0 win, exactly the SIO/dead-end-#12 pattern). Before anything else, localize the layer with the 30-second frame-change test from dead-end #12:
  ```bash
  ./target/release/rustdl classify --saturation-only ontologies/external/<slug>.ofn
  ```
  If the FP persists under `--saturation-only`, the bug is in `owl-dl-saturation`; otherwise it is in the tableau/wedge path. File it in the results doc with the offending `sub ⊑ sup` pair (printed by the harness) and which layer reproduces it. Do **not** attempt the fix in Phase 0 — Phase 0's job is the net, not the repair.

- [ ] **Step 3: Write the results doc**

Create `docs/phase0-soundness-results.md` with a table: ontology | expressivity | classes | wall | FP | MISSED | notes. State plainly whether the broadened corpus holds FP=0 or surfaced a bug (and which layer).

- [ ] **Step 4: Commit**

```bash
git add docs/phase0-soundness-results.md
git commit -m "docs(phase0): broadened-corpus soundness diff results"
```

---

## Task 6: Fragment-completeness statement

**Files:**
- Create: `docs/fragment-completeness.md`

- [ ] **Step 1: Gather the verified-vs-proven inventory**

Read `docs/hypertableau-summary.md` §2–§3 (what is "verified by composition" vs proven) and the `hyper_trust_sat_enabled` doc comment at `crates/owl-dl-reasoner/src/lib.rs:638`. The claim to formalize: **`trust_sat` is sound iff the hyper engine is complete on the workload.**

- [ ] **Step 2: Write the statement**

Create `docs/fragment-completeness.md` covering, with no placeholders:
- **Provably complete fragment:** Horn (DL-clauses with ≤1 head atom) + the supported EL constructs — cite the ELK result (`owl-dl-saturation/src/lib.rs` header) and the Horn determinism argument (`hyper.rs` header "Why Horn is deterministic").
- **Verified-by-composition, not proven:** the disjunctive / cardinality / nominal SROIQ constructs (HF3b/c, HF4b) — list each and note it is corpus-validated, not proof-backed.
- **The soundness implication:** since `trust_sat` trusts a `Sat` verdict, it is sound exactly on ontologies whose expressivity lies inside the provably-complete fragment OR is covered by the validated corpus. Outside both, `Sat`-trust is an empirical bet.
- **What would earn default-on generally:** a decision procedure that, given an ontology's expressivity profile, returns whether it falls inside the provably-complete fragment — the groundwork for the Phase 4 auto-gate.

- [ ] **Step 3: Cross-link**

Add a one-line pointer to `docs/fragment-completeness.md` from `docs/hypertableau-summary.md` §4 (item 1, the generalization work) and from the Phase 0 section of `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`.

- [ ] **Step 4: Commit**

```bash
git add docs/fragment-completeness.md docs/hypertableau-summary.md docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase0): fragment-completeness statement (earns the default-on)"
```

---

## Task 7: Make the broadened diff runnable on demand (CI)

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `scripts/run-soundness-diff.sh`

The corpus fixtures are gitignored and large, so the diff cannot be a push-gated test. It becomes a `workflow_dispatch` job that fetches/classifies/diffs, plus a local convenience script.

- [ ] **Step 1: Write the local convenience script**

Create `scripts/run-soundness-diff.sh`:

```bash
#!/usr/bin/env bash
# Run the full closure-diff soundness net (all corpus + ORE fixtures).
# Requires fixtures already present under ontologies/{real,external}/.
# Asserts FP=0 across the suite (the #[ignore]d tests assert per-ontology).
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture
```

Run: `chmod +x scripts/run-soundness-diff.sh`
Expected: exit 0.

- [ ] **Step 2: Add a workflow_dispatch CI job**

In `.github/workflows/ci.yml`, add a job gated on manual dispatch only (fixtures aren't in the repo, so it cannot run on PRs). Append under `jobs:`:

```yaml
  soundness-diff:
    name: closure-diff soundness net (manual)
    if: github.event_name == 'workflow_dispatch'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      # NOTE: fixtures must be provisioned by the runner (large, gitignored).
      # This job documents the entrypoint; provisioning is out of Phase 0 scope.
      - run: ./scripts/run-soundness-diff.sh
```

- [ ] **Step 3: Confirm the workflow still parses**

Run: `cargo fmt --all -- --check` (sanity that nothing else changed) and visually confirm the YAML indentation matches the existing jobs (2-space).
Expected: fmt passes; the new job is a sibling of `fmt`/`clippy`/`build-and-test`/`deny`.

- [ ] **Step 4: Commit**

```bash
git add scripts/run-soundness-diff.sh .github/workflows/ci.yml
git commit -m "ci(phase0): manual workflow + script for the closure-diff soundness net"
```

---

## Definition of done (Phase 0)

- `docker/robot/classify-oracle.sh` exists, is executable, and reproduces an existing reference within tolerance (Task 1).
- 2–3 inverse + cardinality + role-hierarchy ontologies are fetched, stripped, classified, and wired into `konclude_closure_diff.rs` as FP=0 regression tests (Tasks 2–4).
- The diff has been run and its FP/MISSED recorded in `docs/phase0-soundness-results.md`; any FP is filed with its responsible layer, **not** fixed here (Task 5).
- `docs/fragment-completeness.md` states the provably-complete fragment and the `trust_sat`-sound-iff-complete implication (Task 6).
- The net is runnable on demand via `scripts/run-soundness-diff.sh` and a `workflow_dispatch` CI job (Task 7).

This unblocks Phase 1 (selective trust-sat verification), whose every change is measured against this net.

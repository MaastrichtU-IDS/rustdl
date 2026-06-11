# classify-level concrete-domain verify (per-class) — design (2026-06-11)

## Goal

Make `classify` (the pairwise hierarchy build) detect classes that are
unsatisfiable **only** by a concrete-domain *counting* clash — e.g.
`C ⊑ DataMinCardinality(3 :p [0,1])` (capacity: 3 distinct integers
demanded, 2 exist) or `≥3 ⊓ ≤2` (min/max conflict). Today P3's
`concrete_domain_clash` (`card_sat`) catches these on the **main tableau**
(`is_class_satisfiable` / consistency), but `classify` misses them: its
per-class unsat probe trusts the **wedge's** `LabelOracle::Sat`, and the
wedge cannot run `card_sat` (and, per the wedge-hang fix `c4c61c2`,
deliberately does not even materialise DKey cardinality).

This is **Phase 1 of a measurement-gated escalation**. It ships the
low-risk classify utility (data-unsat *classes*) by reusing the tested
main-tableau clash. The higher-coverage, higher-risk in-wedge clash
(which would also catch named-class ⊑ named-class *counting subsumptions*)
is deferred until a real workload surfaces a case Phase 1 misses. See
"Deferred: in-wedge clash" below.

## Status (implemented 2026-06-11)

**Implementation commits:**
- Task 1 (`32495ae` + `c432516`): `data_counting_classes` builder + `PreparedOntology` field
- Tasks 2–3 (`d3d5aeb` + `442f31d`): failing classify canary + wire override into unsat probe
- Task 4 (`5c5b5d4`): remaining utility + FP-gate canaries
- Task 5 (`3bcbcdc`): D11b `∀+∃` membership probe

**D11b probe outcome:** PASSED on first run (wedge catches `∃p.DKey(v) ⊓ ∀p.DKey(r)` membership clashes in `classify`). The predicate stayed counting-only — no widening needed.

**Step 2 — 1M-cardinality DoS probe:** elapsed=0.012s (well under 1 s); `:C` reported unsatisfiable (rc=0). Hang fix confirmed still in effect.

**Step 3 — Corpus closure-diff (FP=0/MISSED=0 gate):**
- bibtex: rustdl=16, konclude=16 — FP=0 MISSED=0
- alehif: rustdl=247, konclude=247 — FP=0 MISSED=0
- shoiq-knowledge: rustdl=449, konclude=449 — FP=0 MISSED=0
- sio: rustdl=8904, konclude=8904 — FP=0 MISSED=0
- wine: rustdl=653, konclude=653 — FP=0 MISSED=0

All 5 fixtures passed; `test result: ok. 5 passed` in 106.55 s.

**Step 4 — sio perf spot-check:** sio.ofn classify wall = 20.8 s (normal range; `data_counting_classes` is empty for sio → override never fires, no extra main-tableau runs).

**Canaries:** 9 tests in `crates/owl-dl-reasoner/tests/classify_concrete_domain.rs` — 3 utility (capacity-unsat, min/max-conflict, inheritance), 5 FP-gate (satisfiable classes stay satisfiable), 1 D11b membership-in-classify probe — all passing.

**fmt note:** `cargo fmt --all -- --check` failed (rc=1) on the implementation code (line-length rewraps in `lib.rs` + `tests/classify_concrete_domain.rs`). `cargo fmt --all` was run as part of this task and the formatted files included in the commit.

## Measurement caveat (unchanged from the P3 spec)

Real-corpus utility of concrete-domain counting is ≈0 (the target
constructs are rare and produce no naturally-occurring verdict-changing
MISS). We build this for correctness/robustness completeness by explicit
user decision, verified by synthetic canaries. The win here is that
`classify` — the entry point people actually call — now reflects the
counting clashes that `is_class_satisfiable` already detects.

## Load-bearing invariant (soundness)

**The override only ever replaces a wedge `Sat` with a main-tableau
verdict.** The main tableau is the sound+complete path. So the change can
only *add* correctly-detected unsatisfiable classes; it can never produce
a false-positive subsumption (FP=0 preserved). This is strictly safer than
the in-wedge alternative, whose FP risk lives in the hottest, most
correctness-critical engine and is corpus-invisible.

## Architecture & data flow

The gap is at `classify.rs` (the per-class unsat probe, ~line 1071): for
each class it consults `label_cache` (the WEDGE, via `classify_labels` →
`LabelOracle`) and only falls through to the main tableau on `NoVerdict`.
A class unsatisfiable solely by a counting clash gets `LabelOracle::Sat`
→ reported satisfiable.

Fix — a targeted override:

1. **`PreparedOntology::from_internal`** (where `dkey_ranges` already
   lives) builds `data_counting_classes: HashSet<ClassId>` — the named
   classes carrying a *counting* DKey constraint (see predicate below).
2. **Unsat probe**: when the wedge verdict is `Sat` **and** the class ∈
   `data_counting_classes`, do not trust it — run
   `prepared.decide_with_deadline` (main tableau; already threads
   `dkey_ranges`, already runs `concrete_domain_clash`). All other classes
   keep the fast wedge path unchanged. A `NoVerdict`/deadline result is
   treated as satisfiable (sound under-approximation, mirroring the
   existing probe fallback).

Composition: the clausify hang-fix (`c4c61c2`) stays — the wedge still
safely ignores DKey cardinality during the label-cache build (no hang).
The main-tableau verify is suppression-guarded (`apply_min`/`apply_max`
skip DKey fillers) so a `≥10⁶` class clashes via `card_sat` before any
materialisation (no hang on the override path either).

## The `data_counting_classes` predicate

**Qualifies (narrow by design):** a class that carries a *counting* DKey
constraint — `DataMin/Max/ExactCardinality` over a recognised datatype
range, which P3 lowers to a `Min`/`Max` ConceptExpr over a DKey filler
(filler ∈ `dkey_ranges.keys()`). This is exactly what `card_sat` can
refute and the wedge cannot.

**Excluded (keeps the fast path for value-membership ontologies, e.g.
`sio`'s 8904 classes — no regression):** value-membership DKeys
(`∃p.DKey`, `∀p.DKey` from `DataSomeValuesFrom`/`DataHasValue`/
`DataAllValuesFrom`). The wedge already handles these — `∃` generates a
DKey successor, told `DKey⊑DKey` edges + `DisjointClasses(DKey,DKey)`
(D11b) propagate — so membership subsumptions/clashes are caught in the
wedge today (`sio` passes for this reason). They carry no counting demand,
so `card_sat` adds nothing.

**Construction (at `from_internal`):**
1. Scan the post-absorb IR for named classes with a `Min`/`Max`
   ConceptExpr whose filler ∈ `dkey_ranges.keys()` → the *direct* set.
2. Close downward via the told-subsumer table (already built): if
   `C ⊑* D` and `D` is direct, add `C` (inheritance — `C` is unsat by the
   same clash, and classify verifies each class independently).

**Perf gating:** if `data_counting_classes` is empty (every corpus
ontology except synthetic), the probe is byte-identical to today — zero
extra main-tableau runs. The override fires only for the literally
counting-constrained classes (corpus: 0; `shoiq`: ~1 satisfiable).

## Open verification (settled in implementation, not assumed)

Confirm the wedge genuinely catches a D11b `∀+∃` membership clash **in
classify** (not only in `is_class_satisfiable`). Probe: a class with
`∃p.DKey(v) ⊓ ∀p.DKey(r)`, `v ∉ r`, run through `classify`.
- If caught (expected — the wedge has `∃`-generation + `∀`-propagation +
  disjointness clauses): the predicate stays counting-only.
- If missed: widen the predicate to also include `∀`-over-DKey classes
  (still narrow; still excludes pure `∃` value-membership). Recorded as a
  test gate, not baked into the design either way.

## Testing

**Utility canaries (new, classify-level):** a class unsatisfiable only by
a counting clash must appear unsatisfiable via `classify`:
- `≥3 p.[0,1]` (capacity) → C unsatisfiable.
- `≥3 ⊓ ≤2` (min/max conflict) → C unsatisfiable.
- Inheritance: `D` carries `≥3 p.[0,1]`, `C ⊑ D` → both C and D
  unsatisfiable (exercises the told-subsumer downward closure).

**FP gate (negatives-first — must stay satisfiable via classify):**
`∃p.[0,10]`, `=2 p.[0,10]`, `≥2 p.[0,1]` (tight-but-feasible), `≤1 p`
alone, non-integer `≥3 p.{a,b}`.

**Non-regression:**
- Corpus closure-diff FP=0/MISSED=0 unchanged on the data-bearing
  fixtures (shoiq 449, sio 8904, wine 653, alehif 247, bibtex 16).
- Perf: `sio` classify wall unchanged (value-membership →
  `data_counting_classes` empty → no extra main-tableau runs); spot-check
  before/after.
- The D11b `∀+∃` membership-in-classify probe (above) — widens the
  predicate only if it fails.
- The 1M-cardinality DoS probe (challenge #1) still terminates fast (hang
  fix stays; override path is suppression-guarded).

## Deferred: in-wedge clash (Phase 2, measurement-gated)

If a real workload ever surfaces a named-class ⊑ named-class subsumption
entailed by data *counting* (where neither class is unsat alone, only
`C ⊓ ¬D` is — e.g. `C ⊑ ≥5 p.R` tested against `≥3 p.R`), Phase 1's
per-class verify misses it (it only checks each class alone). Closing that
needs the clash inside the wedge fixpoint: thread `dkey_ranges` into
`HyperEngine`, make `generate_at_least`/AtMost **record but not
materialise** for DKey fillers, add a `card_sat` clash hook with a
backjumping `DepSet`. This is surgery on the FP-critical wedge and reverts
the clausify hang-fix's drop in favour of record-but-not-materialise. Do
**not** model it on the pre-wedge clausify drop. Build only against a
measured miss.

# Hypertableau effort — capstone summary

Written 2026-05-27, at the close of the multi-phase hypertableau push.
This is the pick-up-cold document: what was built, what's verified,
where the boundaries are (and *why*), and the two honest forward
paths. Detail lives in the per-phase scoping docs cross-referenced
below and in the commit history.

## 1. The arc — what shipped

| phase | what | outcome |
|---|---|---|
| H0 | DL-clause representation + corpus shape stats | corpus ~96 % Horn |
| H1/H1b/H1c | Horn hyperresolution engine; structural clausifier | EL back-prop derived; RO/family/SIO deferred-counts slashed |
| H2 | disjunctive-head branching (`decide`), 3-valued result | complete decision procedure for Horn+disjunctive |
| H2b | wall probe (`hyper-sat`), instrumentation | SIO bare-sat moves: 16.3 s, branching real, vs default >135 s |
| H2c | pair-subsumption probe (`¬B` injection) | reaches pizza wall; sound, validated vs Konclude |
| H3a | antecedent DNF-distribution | pizza misses 114→77 |
| H3b | `¬sup` expansion + negative literals | 77→29 (antecedent-∀/¬ family) |
| multi-role | conjunctive (tree) body matching | 29→24 |
| H3c | `≤n` merge rule (union-find, branching) | 24→4 (cardinality) |
| nominals | nominal-as-atomic clausification | 4→0 — **pizza 100 %** |
| perf | clause indexing → semi-naive event eval | SIO 16.3 s → **0.45 s (~36×)** |
| H4 | sound-accelerator wedge (flag-gated, default off) | shipped; classify wall *not* moved (see §3) |

Every completeness step held **0 false positives**.

## 2. What's verified, and how

Methodology: clausify → run the engine per class (`hyper-sat`) or per
ordered pair (`hyper-classify-probe --dump-subsumptions`); diff the
result against the transitive closure of a reference reasoner's
classification (`cmp_generic.py` / `cmp_localname.py`). References:
**Konclude** (the goalpost) and **HermiT** (second opinion), both via
docker (see [[rustdl-konclude-input]]).

| ontology | expressivity | result |
|---|---|---|
| pizza | SHOIN | 695/695 subsumptions, **0 misses, 0 FP** |
| ro-stripped | SROIFV | 158/158, **100 %, 0 FP** |
| sulo-stripped | SRI | 51/51, **100 %, 0 FP**; HermiT agrees 100 % |
| sio-stripped | SRIQ | 1585 classes, 0 unsat (agrees Konclude), 0 FP |

Performance: SIO bare-sat **0.45 s vs Konclude 0.14 s (~3×)**, down
from ~116× at H2b. The wins came via clause indexing + semi-naive
*event* evaluation (a profile-first pivot that overturned the
intuitive trail-based fix; the node-granularity first cut was refuted
by measurement — 52M→57M — before the event model landed).

Incidental: HermiT **hangs > 9 min** on `ro-stripped` (SROIFV,
inverse+functional) where Konclude is 2 ms and this engine instant.

## 3. Boundaries — and their causes

The engine is **sound for `Unsat`** (subsumption-holds / unsat) on any
ontology: clausification only ever *weakens* the theory
(`Models(ontology) ⊆ Models(clauses)`), so a clause-set `Unsat` ⇒ an
ontology `Unsat`. Its `Sat` is **not** sound on the full theory, and
this is load-bearing:

- **ABox / consistency: out of scope.** The engine is TBox-only.
  `family-stripped` is ABox-inconsistent (1848 assertions) — Konclude
  rejects it; the engine has no opinion. Not a failure; a boundary.
- **`Sat` is unsound under the under-approximating clausifier.** Three
  causes, each independently fatal to "the `Sat` completion is a model
  of the full theory":
  1. *Cardinality* dropped from the clauses (e.g. `≥3 hasTopping`) —
     a completion needn't satisfy it.
  2. *Inverse roles* (pizza has 3): the engine uses **anywhere
     blocking**, which is **unsound with inverses** (those need
     pair-blocking). A `Sat` completion can correspond to no model of
     the inverse-bearing theory at all.
  3. *Nominals* clausified as plain classes — singleton-equality lost.
- **The classify wall is negative refutation, not positive proof.**
  Measured: `classify(pizza)` with the H4 wedge on = 4 m 38 s, 1119
  timed-out pairs, **0 hyper-proven**. The orchestrator already proves
  positive subsumptions via EL saturation (353/695) + transitive
  closure; the residual tableau pairs are *non-subsumptions* refuted
  by `sat(A⊓¬sup)` — model search on satisfiable instances. An
  `Unsat`-only accelerator has no work there, and trusting `Sat` is
  unsound (above). So the wedge is sound and shipped, but it does not
  move the classify wall.

**Net:** the engine's value is **probe-shaped** — single-query
*positive* subsumption (fast sound `yes` where EL misses) and
*per-class satisfiability* (SIO 0.45 s vs >135 s) — not full
classification of inverse/cardinality-rich ontologies.

## 4. The two forward paths (each a separately-scoped major effort)

1. **Full sound+complete hypertableau.** Fold cardinality and nominals
   into the calculus (not post-hoc), switch anywhere→pair blocking for
   inverse-role soundness, add `≥n` generation with the ≤-before-≥
   ordering for termination. This makes `Sat` sound → enables negative
   refutation → moves the classify wall. It is the Motik/Shearer/
   Horrocks 2009 algorithm in full; months, its own scoping.
2. **Model-validation (research).** Extract a *blocking-aware
   certificate* from a `Sat` completion and validate it against the
   dropped axioms. **Soundness obstacles (verified, do not re-misjudge
   as "medium-effort"):** cardinality interacts non-locally with
   blocking; anywhere blocking is unsound with inverses so the
   completion isn't a model to begin with; nominal "validation" is a
   merge *fixpoint*, not a check. Doing it correctly reproduces the
   complete calculus — i.e. collapses into path 1. A genuine research
   contribution lives here, not an implementation turn.

A pragmatic middle option, if neither is wanted: **promote the
probe-shaped wins to first-class reasoner APIs** (a public sound
single-query subsumption accelerator + per-class sat), banking the
measured value without the wall.

## 5. Pick-up-cold artifacts

- **Scoping/design docs:** [`hypertableau-scoping.md`](hypertableau-scoping.md)
  (master: H0–H2c, profiling, corpus agreement, HermiT cross-check),
  [`hypertableau-seminaive-scoping.md`](hypertableau-seminaive-scoping.md),
  [`hypertableau-cardinality-scoping.md`](hypertableau-cardinality-scoping.md),
  [`hypertableau-h4-scoping.md`](hypertableau-h4-scoping.md).
- **Code:** `owl-dl-core::clause` (clausifier), `owl-dl-tableau::hyper`
  (engine: `decide`, event worklist, `≤n` merge, blocking),
  `owl-dl-reasoner` (`hyper_subsumption_probe`, `hyper_sat_probe`,
  `HyperCache` wedge).
- **CLI probes:** `rustdl hyper-sat`, `rustdl hyper-classify-probe
  [--dump-subsumptions]`, `rustdl clause-stats`. Wedge opt-in:
  `RUSTDL_HYPERTABLEAU=1`.
- **Tests** encode the invariants: engine unit tests (`hyper::tests`),
  reasoner end-to-end + the H4 encoding-drift guard
  (`hyper_wedge_agrees_with_tableau`).
- **Memory:** the `rustdl-hypertableau-h2b` note carries the
  load-bearing findings (the convergent-vs-timeout distinction, the
  SIO-vs-pizza drop split, the profiling lesson, the classify-wall
  reframe).

**Process note worth keeping:** the dead-ends (node-granularity
semi-naive, the allocation fast-path, shared indexes as a standalone,
model-validation) were each killed by *measurement or grounding in the
actual target*, before sinking effort — examine the real deferred
constructs / profile before scoping, not after.

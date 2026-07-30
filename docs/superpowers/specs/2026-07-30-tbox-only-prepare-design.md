# TBox-only `PreparedOntology` construction (Lever A, extended to the build side)

**Date:** 2026-07-30
**Status:** Design — ready for implementation planning
**Evidence:** `docs/2026-07-29-memory-tail-localization.md` §8–§10
**Flag:** `RUSTDL_TBOX_ONLY_PREPARE`, landing **default-OFF**, flip criterion in § Rollout

## The problem, measured

`PreparedOntology::from_internal` allocates **+42.3 GB in a single call** on `ore_ont_9347` —
an **8.6 MB input with 114 classes**. In-process probes (`RUSTDL_TRACE_RSS`, merged):

| probe | hyper ON | hyper OFF | ABox stripped |
|---|---|---|---|
| `before_prepared` | 3.95 GB | 3.92 GB | 0.01 GB |
| **`after_prepared`** | **46.26 GB** | 28.41 GB | **0.01 GB** |
| outcome | 238 GB → OOM-killed | timeout | **completes, exit 0** |

It splits into ~17.9 GB `HyperCache::build` and ~24.5 GB absorb — `tbox-stats` reports
**`concept_rules: 49,571,087`**.

The driver is the ABox: `9347` carries ~55k assertions (13,356 `ClassAssertion`, 22,931
`ObjectPropertyAssertion`, 19,160 `DataPropertyAssertion`) and is **completely nominal-free**
(`ObjectOneOf`/`ObjectHasValue`/`SameIndividual`/`DifferentIndividuals`/
`NegativeObjectPropertyAssertion` all 0). Removing exactly those 55,446 assertions drops
`after_prepared` to **0.01 GB — 4600× — and the classification completes.**

**Lever A already computes that this ABox is irrelevant and does not act on it here.**
`abox_irrelevant_to_classify` (`lib.rs:4558`) is `classify_tbox_only_enabled() && has ABox &&
!nominals` — correct — but it is read at exactly two sites (`lib.rs:4865`, `:4992`), both
substituting an empty `Abox` when building **per-pair tableau seeds**. Everything above them in
`from_internal` — `saturate`, `build_told_tables`, `axioms.clone()`, `HyperCache::build`,
`ConsistencyCache::build`, `expand_role_characteristics`, `nnf_axioms`/absorb — runs on the
**full** `internal`. The lever drops the ABox at consumption and pays for it at construction.

## Why this is not a one-line hoist

`from_internal` is called from **eight modules**: `classify.rs` (×5), `realize.rs` (×5),
`abox_check.rs` (×2), `disjointness.rs`, `individuals.rs`, `property_values.rs`, and `lib.rs`
(numerous). Its own doc comment at `lib.rs:4280` already records the constraint:

> `abox` is still kept full for `realize`/`materialize`/consistency.

Filtering the ABox at the top of `from_internal` would therefore silently break every
ABox-dependent consumer. Three distinct needs coexist behind one constructor:

| consumer | ABox requirement |
|---|---|
| classify's subsumption oracle | irrelevant **iff** nominal-free **and** the KB is consistent |
| `abox_check` / `abox_verdict()` | **full** — an inconsistent ABox makes every class unsatisfiable |
| `ConsistencyCache::build`, realize, materialize, `individuals`, `property_values` | **full** by definition |

## The soundness contract (state it precisely — the whole design rests on it)

For a **nominal-free** ontology, no concept expression can refer to an individual, so ABox
assertions cannot participate in deriving `C ⊑ D` — **with one exception: an inconsistent ABox
entails everything, including every subsumption.**

So the safe statement is conditional, not absolute:

> If the ontology is nominal-free **and the KB is consistent**, class subsumption is determined
> by the TBox alone.

Therefore dropping the ABox from the *subsumption oracle* is sound **only while the
inconsistency check still runs on the full ABox.** That is exactly the split Lever A already
relies on: it empties the `Abox` inside `decide_classify` while `abox_check` runs separately on
the full input. This design must preserve that invariant, not weaken it.

Direction of risk if the contract is honoured: dropping axioms weakens the KB, so the failure
mode is a MISS, never a false positive. Lever A shipped with a 271-ontology on-vs-off validation
showing **0 answer changes**.

## Two candidate shapes

### Option A — a classify-specific constructor (recommended)

Add `PreparedOntology::from_internal_tbox_only(internal)`, which filters ABox assertion axioms
out of `internal` **before** any construction, then delegates to the existing body. Route only
the classify call sites to it, and only when `abox_irrelevant_to_classify` would be true.

- **Zero risk to the seven non-classify consumers** — their code path is byte-identical.
- The existing `abox_irrelevant_to_classify` field becomes redundant on this path (the `Abox` is
  empty by construction) but must be **left in place**, because the flag-OFF path and the
  non-classify consumers still use it.
- `abox_check` must be called on the **full** ontology *before* the TBox-only prepare, or from
  its own `from_internal`, so the inconsistency verdict is not lost. `classify.rs:786` already
  builds a separate `PreparedOntology` for exactly this — that structure is what makes Option A
  cheap.
- Reuse `is_abox_axiom` (`classify.rs`) as the filter predicate; it already enumerates the five
  assertion forms and is documented as kept in sync with `has_abox_axioms`.

### Option B — two-phase build inside `from_internal`

Keep one constructor; run the ABox-dependent work (`saturate` for `abox_check`,
`ConsistencyCache::build`) on the full input, then filter before the TBox-side work
(`HyperCache::build`, `expand_role_characteristics`, `nnf_axioms`/absorb).

- No API addition, and every consumer benefits.
- But it interleaves two ontology views inside one function whose consumers have opposite
  requirements, and a future edit that moves a line across the filter point silently changes
  semantics for eight callers. It also cannot express "this caller wants the ABox" — the
  distinction is per-caller, not per-phase.

**Recommendation: Option A.** The distinction being encoded is *which query is being asked*, and
that is caller knowledge. Option B pushes caller knowledge into a shared function, which is how
the gate/engine mismatches fixed earlier this month arose — two sites disagreeing about what the
other handles.

## Scope

**In scope.** The new constructor, routing the classify call sites to it, the flag, tests, and
the corpus + oracle gate.

**Out of scope.** Any change to realize/materialize/consistency/`disjointness`/`individuals`/
`property_values`. Widening the nominal-free condition. `ore_ont_11085`'s memory (a **second,
distinct** mechanism — 33.7 GB with only 20,758 concept rules, so absorb is not its problem;
`HyperCache::build` is the candidate given its `resid_or: 1341`). Explaining *why* absorb emits
49.6M rules — the fix does not depend on it, and the multiplicative ABox × disjunctive hypothesis
is **refuted** (`5368`: 18.6M rules at `resid_or: 34`; `11085`: 20,758 rules at `resid_or: 1341`).

## Expected payoff — five ontologies, and be honest about it

Selected by `concept_rules` (which separates the known points 346×; **ABox size does not
predict the blowup** — `9347` has 55k assertions at 46 GB, `ore_ont_10125` has 748k at 1.7 GB):

| ontology | `concept_rules` | ABox | nominals |
|---|---|---|---|
| `ore_ont_9347` | 49,571,087 | 55,446 | 0 |
| `ore_ont_5368` | 18,620,251 | 18,301 | 0 |
| `ore_ont_1833` | 14,030,936 | 42,429 | 0 |
| `ore_ont_7607` | 11,640,553 | 85,639 | 0 |
| `ore_ont_1685` | 11,506,431 | 51,637 | 0 |

`5368` was independently corroborated (`after_prepared` 17.73 GB). `ore_ont_9694`/`16542` have
the same shape but 199 nominals, so Lever A's premise excludes them.

**Do not claim more.** An earlier estimate of 83 ontologies ("ABox-bearing AND nominal-free")
collapsed on testing: of its four largest members, **0 of 4** were rescued by ABox stripping and
none was memory-bound (1.1–2.7 GB). Report gate levers by *measured recoveries*, never by
feature-presence counts.

**The stronger case is wasted work, not DNF recovery.** Building 49.6M concept rules and an
~18 GB clause set from axioms the query provably ignores is waste on *every* ABox-bearing
nominal-free classify, including the ones that already succeed. Five DNF recoveries is the
headline; the avoided work is the argument.

## Gates

1. **FP=0 / MISSED=0.** `./scripts/run-soundness-diff.sh` — 16 tests, the reference closures
   (galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247,
   ro 158, ore-15672 142, sulo 51, bibtex 16). Mandatory: **CI does not run this** (the job is a
   `workflow_dispatch` stub with unprovisioned fixtures), so a local run is the only FP=0
   evidence this change will ever get.
2. **Flag ON-vs-OFF byte-identity** on the curated corpus. Any diff is a bug — the rewrite is
   supposed to be entailment-preserving.
3. **Inconsistency preserved.** An ABox-inconsistent, nominal-free ontology must still report
   every class unsatisfiable with the flag ON. This is the contract above; without this test the
   change is unsound rather than merely unproven.
4. **Recovery.** The five ontologies above: measure `after_prepared` (via `RUSTDL_TRACE_RSS=1`)
   and whether classify now completes. Report per-ontology, not as a total.
5. **Non-classify consumers untouched.** `realize`, `materialize_*`, `consistent`,
   `disjoint_classes`, `same_individuals`, `inferred_*_property_values` byte-identical ON vs OFF.
   Under Option A this is true by construction; the test exists to pin it.

## Testing

- **Negatives-first, and the load-bearing one:** ABox-inconsistent + nominal-free ⟹ all classes
  unsatisfiable with the flag ON (gate 3). Write this **first**; if it fails, the design is wrong.
- A **nominal-bearing** ABox ontology must NOT take the TBox-only path — the flag must not
  override the nominal condition.
- A nominal-free ABox ontology where the ABox is irrelevant: classification identical ON vs OFF,
  and `after_prepared` measurably lower.
- An ABox-free ontology: entirely inert, byte-identical, no new allocation path taken.
- `realize` on a nominal-free ABox ontology: identical ON vs OFF (proves the constructor split
  did not leak into the ABox-dependent consumers).

## Rollout

Land **default-OFF**. Flip to default-ON with a `=0` revert — matching
`RUSTDL_CLASSIFY_TBOX_FRAGMENT` and `RUSTDL_NEG_TO_BOT_GCI` — only once gates 1–5 are green
*and* gate 4 shows recovery on at least one of the five ontologies. If gate 4 shows zero
recoveries, keep it OFF and record it as a measured non-lever; the wasted-work argument alone
does not justify a default-ON change to a shared constructor on an FP-relevant path.

## What this does not claim

- It does not explain the 49.6M concept rules, and does not need to.
- It does not address `ore_ont_11085` or the ~139 ABox-free DNF ontologies.
- It does not make the memory tail tractable in general — after this, the tail still contains at
  least one further distinct mechanism, and `docs/2026-07-29-memory-tail-localization.md` §5
  records that the benchmark's RSS column is timeout-truncated, so the tail's true size is
  uncertain in both directions.

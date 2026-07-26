# Design: pseudo-model realize shortcut (salvaged from PR #23)

**Date:** 2026-07-26
**Status:** design approved (brainstorming); pending spec review → implementation plan.
**Author:** Claude + Michel.
**Origin:** PR #23 (@rcelebi, `feat/repr-cache`) is 457 commits stale and largely
superseded — its realize-hang fix (saturation fast path + #35-v4 safety net) and
incremental-fixpoint (`RUSTDL_HYPER_INCREMENTAL_FIXPOINT`) are already on `main`.
The **one genuinely-novel, non-superseded idea** is the *pseudo-model shortcut*;
this spec re-extracts it against current `main`. Recommend closing #23 once this
lands (crediting the author).

## 1. Goal

Speed up **off-fragment (tableau) realization** — the slow path where `realize`
probes `{a} ⊓ ¬C` for each `(individual, class)` pair (bounded by
`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, and yielding a sound MISS on pairs that time
out). On nominal ABoxes this is O(individuals × classes) probes, each up to the
per-pair budget — minutes on real ontologies (MIE breast-cancer: was a hang; #23
measured ~110–630× with this shortcut).

The saturation fast path (`RUSTDL_REALIZE_SATURATION`, complete on the EL/Horn
fragment) is **untouched** — it already skips per-pair probes. This targets only
the off-fragment tableau path.

## 2. Mechanism — one witness model prunes most non-membership probes

Run the ABox-seeded consistency **wedge once**; on `Sat` it is a clash-free
completion (a *witness model* of the KB). Read each named individual's **full**
class-label set in that completion → `base_model_types: Vec<HashSet<ClassId>>`
(index `i` = individual `i`'s types in the witness).

In the per-pair check, **before** the `{a} ⊓ ¬C` tableau probe:
> if `class ∉ base_model_types[individual]` ⇒ return **not a member** (no probe).

The witness model places that individual *outside* the class, so it is a genuine
counter-model to membership: `KB ⊭ class(a)`. One model computation refutes the
vast majority of `(individual, class)` pairs; only classes actually present in
the witness need the real probe.

## 3. Soundness (the load-bearing argument)

- **Completeness-preserving, never a false positive.** A genuinely-entailed type
  holds in *every* model, hence in this witness, hence is present in
  `base_model_types[i]` — so it is **never pruned**. The prune only ever returns
  "not a member," and only when a real counter-model exists.
- **This is the *sound* membership direction** — distinct from the snapshot-cache
  subsumption reuse trap (`sup ∈ model ≠ sub ⊑ sup`, FP-unsound, defaulted OFF).
  Here `class ∉ individual's-model-types ⇒ not entailed member` is valid.
- **The one real risk is INCOMPLETENESS (a MISS), never unsoundness:** it holds
  *only if the label read is the individual's COMPLETE completed type set*. If the
  reader under-reports (returns a partial label set), a genuine member could be
  pruned → a MISS. So the label primitive must read the full completion labels,
  and the assessment (§6) is an oracle-backed completeness gate, not just a wall
  check. If the witness wedge returns `Unsat`/`Stalled` (no usable model),
  `base_model_types` is `None` → the shortcut is skipped entirely (every pair
  takes the normal probe path — no change).

## 4. Implementation

Reusing `main`'s existing plumbing (`ConsistencyCache` already holds
`clauses` + `seed: AboxSeed` + `sub_roles` + `num_classes`/`num_individuals`, and
`ConsistencyCache::decide` already builds `HyperEngine::new_seeded(&clauses,
&seed).with_nominals(…)`):

- **`owl-dl-tableau` (`hyper.rs`) — the one new primitive (if not already present):**
  `HyperEngine::seeded_individual_labels(individual_idx) -> Vec<ClassId>` (or a
  batch variant) — read node `individual_idx`'s **complete** atomic-class label
  set from the completed graph after a `Sat` verdict. First check whether the
  existing seeded/per-individual reader (`ConsistencyCache` uses a per-individual
  path around lib.rs:3213/`decide`) already exposes full labels; reuse it if so.
- **`owl-dl-reasoner` (`lib.rs`) — `ConsistencyCache::base_model_types(deadline)
  -> Option<Vec<HashSet<ClassId>>>`:** a sibling of `decide` — build the same
  seeded nominal wedge, `decide_with_deadline` once, and on `Sat` collect each
  individual's `seeded_individual_labels`. `Unsat`/`Stalled` ⇒ `None`. Exposed to
  realize via `PreparedOntology::realize_base_model_types(deadline)`.
- **`owl-dl-reasoner` (`realize.rs`) — wiring:** in `realize_internal`'s
  off-fragment path, compute `base_model` once (gated by the flag), and thread the
  per-individual `base_types` into `instance_check_with_closure`; add the
  `class ∉ base_types ⇒ Ok(false)` short-circuit *after* the told-closure fast
  path and *before* the `{a} ⊓ ¬C` probe (the `pool.and([nom, ¬cls])` site,
  realize.rs:210).
- **Optional companion (defer unless cheap):** #23's `realize_candidate_classes`
  (restrict probes to classes derivable in some clause head) — a second sound
  prune. Not required for the core win; note as a follow-up.

`convert_back`/API surface unchanged; `Realization` result unchanged.

## 5. Gating

Ship behind **`RUSTDL_PSEUDO_MODEL`**. Its default is decided by the §6 assessment:
- **Default ON** iff the assessment passes clean (verdict-identical + oracle-sound +
  a wall win).
- Otherwise **default OFF** (opt-in), shipped as documented scaffolding.

`=0` (or empty) disables; the flag-off path is byte-identical to today.

## 6. The default-ON assessment (gate)

Before flipping the default, run a **bake-off, prune ON vs OFF**:
1. **Verdict-identical (completeness-preserving) on ORE-tier + curated fixtures:**
   `realize` output (per-individual entailed types + most-specific types)
   **byte-identical** ON vs OFF across the ABox-bearing corpus (the ORE ABox
   ontologies + curated fixtures with individuals — e.g. `sulo`, `mie`-style,
   `family`-adjacent). Any divergence = a completeness regression = blocks
   default-ON (and is a bug in the label-completeness of the primitive).
2. **Oracle soundness on a nominal ABox:** a HermiT/Konclude oracle check on a
   custom nominal-ABox fixture (MIE-style: nominals + defined class + property
   domain + assertions) — rustdl's realized types with the prune == the oracle's
   (FP=0 and no new MISS vs OFF).
3. **Wall win:** a measurable realize speedup on that nominal ABox (the whole
   point).

If 1–3 pass → default ON. Record the assessment in the results doc. (Corpus/ORE
fetch may need a Linux box; if unrunnable in-sandbox, ship **opt-in** and flag the
assessment as the pre-default-ON gate, mirroring how the #40 corpus bake-off was
handled.)

## 7. Testing

- **Unit canaries:** the prune refutes a true non-member (class absent from the
  witness) without a probe; it does **not** prune a true member (present in the
  witness); `Unsat`/`Stalled` witness ⇒ `base_model_types == None` ⇒ graceful
  fallback (every pair probed normally); flag-off ⇒ identical to today.
- **`seeded_individual_labels` completeness test:** on a fixture where an
  individual's completion type set includes a *derived* (not asserted) class, the
  primitive reports it (guards the incompleteness landmine of §3).
- **Verdict-identity gate:** realize ON-vs-OFF byte-identical on the fixtures that
  have individuals (a focused, in-repo subset of the §6 assessment).

## 8. Non-goals

- Not the `instance_check_wedge` half of #23 (realize *termination*) — `main`
  already terminates via the saturation fast path + `RUSTDL_MAX_NODES`/pair-timeout
  safety net. This is purely the *pruning* speedup.
- Not #23's speculative caches (`RUSTDL_REPR_CACHE`/`REPR_BLOCK`/`STRUCT_MEASURE`)
  — no measured win, reuse-cache-trap-adjacent.
- No change to classify/consistency, the saturation fast path, or the public API
  shape.

## 9. Risks / open items for the plan

- **Does `main` already expose a full per-individual label reader** (the
  `ConsistencyCache` per-individual path near lib.rs:3213), or must
  `seeded_individual_labels` be added to `hyper.rs`? Plan confirms and picks the
  minimal surface. This is the primary mechanical unknown.
- **Label completeness** (§3) — the reader must return the *completed* label set,
  not just seeded/told labels. Pin with the §7 completeness test + the §6 gate.
- **Witness cost** — one extra seeded-wedge `decide` per `realize`. Cheap relative
  to the O(n²) probes it saves, but note it; `Stalled` witness (deep ABox) ⇒
  `None` ⇒ no regression (just no speedup).

# SP2 — Node-local UNSAT no-good viability + sound prune (design)

**Status:** advisor-reviewed (2026-07-14) — for user review
**Parent roadmap:** `docs/superpowers/specs/2026-07-13-dense-sroiq-tractability-roadmap.md` (SP2)
**Goal:** decide — cheaply and honestly — whether a **sound node-local UNSAT
no-good** layer in the hypertableau wedge helps `classify ore_ont_10019` decide
within budget, and if so ship it. This is roadmap lever **(2b)**; lever (2a)
"stronger blocking" was down-weighted by SP0 (blocking already fires ~76%; the
stall is disjunctive-**depth**-bound, which node-blocking does not cap).

## What decided the direction (SP0 + SP1 recap)

- **SP0:** `ore_ont_10019` has **no inverse roles, no nominals**, but **has
  `=n` cardinality**; the stalled classes branch 100% on disjunction
  (`merge=0`); blocking fires ~76% and node count is capped (~1099). ⇒ pure
  label-set no-goods are *admissible in principle* (no inverse/nominal), and the
  cost is not a blocking failure.
- **SP1:** incremental `horn_fixpoint` cut per-branch re-derivation ~56× and the
  33 classes **still** don't decide — they reach the depth cap with cheap
  branches. The residual stall is **depth-bound, not per-branch-cost-bound.**

So 2b is the indicated lever *and* the one carrying the heaviest proof burden:
the prior **sound node-local CDBL** (in `search.rs`) "hit 19×, moved wall **0%**,
and was reverted" (`docs/cdbl-plan.md`). 2b must show the wedge regime differs.

## The advisor reframe (why a naive repeat-rate probe misleads)

A read-only "how often does the same clash label-set recur" probe — the first
proposal — is rejected. Three load-bearing reasons:

1. **Backjumping confound (make-or-break).** `solve` already does
   dependency-directed backjumping on `clash_deps`. A *low* observed repeat-rate
   cannot distinguish "no reuse exists" from "backjumping already captured the
   reuse," and a *high* rate can be forced re-derivations backjumping legitimately
   re-takes. Raw repeat-rate maps to neither wall nor viability. This confound
   (plus low hit volume + a timeout-bound regime) is very likely **why the prior
   CDBL moved 0%.** The honest observable is **net-new prunes beyond
   backjumping**, weighted by pruned-subtree cost.
2. **Wrong key.** The full-label-set key already exists (`clash_label_key`) and
   structurally *undercounts*: generated bond-successors rarely share byte-
   identical full label-sets but routinely share a small UNSAT **core**. Worse, a
   *syntactic* node-local clash (`Lᵢ⊓Lⱼ→⊥`) is already caught eagerly by the TBox
   clause — caching it prunes nothing. Only **non-syntactic** cores (multi-step
   antecedents, e.g. `{L₁,L₃}` forcing a clash via `L₁→L₂`, `L₂⊓L₃→⊥`) have 2b
   value. The honest key is the **minimal antecedent / decision-label core**,
   matched by **subsumption** (`core ⊆ candidate label-set`), never equality.
3. **Depth-locality is decisive and was omitted.** SP1 proved the residual is
   depth-bound; no-goods prune re-derivation, not the depth needed to refute. Only
   repeats that cut the **deep tail** can change the outcome; shallow-concentrated
   repeats are what backjumping already handles. `ClashRecord.branch_depth`
   records this for free.

Also: **SP1 already ate most of 2b's upside** (re-derivation is ~56× cheaper) and
per-solve scoping (required by `reuse-trap-A1` to avoid cross-pair FP) forbids the
big cross-pair win. **2b's realistic ceiling is low; the design is built to reject
cheaply.**

## Infrastructure inventory (grounded in the current tree)

**Already exists (reused, not rebuilt):**
- `RUSTDL_SHADOW_DEP_PROBE` / `HyperEngine::with_shadow_dep_probe` is **already
  wired into the live classify path** (`reasoner/src/lib.rs:2608`
  `decide_with_stats`, `:2671` `classify_labels`), populating
  `SearchStats.clash_records`.
- Each `ClashRecord` already carries `clash_label_key` (full-set hash),
  `branch_depth`, and the real/shadow `DepSetSnapshot`s.
- `owl-dl-tableau/src/shadow_measures.rs` — read-only measures over clash records
  (`Histogram`, etc.).
- Merge-taint flags on `HyperNode`: `at_most_tainted`, `nn_tainted` (live-path,
  set during search) — a conservative derivation-local exclusion. (NOT
  `shadow_merge_cause`: it is written only under the shadow probe, so it is empty
  under `RUSTDL_WEDGE_NOGOOD` and unusable here.)

**Must be built (Stage 1) — NOTE (advisor rework):** the tableau's
`saturate::verify_node_local_clash(pool, tbox, hierarchy, &[ConceptId], …)` is
**NOT reusable** as the soundness oracle: the wedge (`HyperEngine`) holds no
`ConceptPool`/`AbsorbedTBox`, reasons over clausal `DlClause`, and its node
labels are `ClassId` (a distinct newtype from `ConceptId`, whose space includes
preimage-less Tseitin names). Stage 1 must build a **wedge-native** node-local
UNSAT oracle over `self.clauses` (node-local clauses = all-`Class`-atom bodies on
one variable; plus `disjoint_pairs`). That oracle — not merge-taint — is the
load-bearing soundness guarantee (a genuinely node-local core is TBox-global,
sound regardless of `≤n`-merge provenance, on the no-inverse/no-nominal fragment).
- `DepSet` + `clash_deps` at the clash site; `record_clash`.

**Must be built (Stage 1):**
- **Core extraction.** `clash_decision_labels_at` does **not** exist — Stage 1
  builds the map from a clash's `clash_deps` (+ clashing node label-set) to the
  minimal antecedent / decision-label **core** that is the no-good key.

## Stage 0 — depth-binned repeat-rate kill-check (near-zero engine code)

**Purpose:** a cheap, decisive early gate. Reuse the already-wired shadow probe;
the *only* new code is a measure + a way to emit it.

- Run `classify ore_ont_10019` (live path) with `RUSTDL_SHADOW_DEP_PROBE=1`,
  collecting `clash_records`.
- Add to `shadow_measures.rs` a **depth-binned full-set repeat-rate** measure:
  per class-solve, group clash records, count how often a `clash_label_key`
  recurs, and cross-tabulate the repeat-rate by `branch_depth` (shallow vs deep-
  tail bins). Emit per-class + aggregate (median stalled class), plus the
  deep-tail share of repeats.
- Provide a way to surface it on the real run — a small `hyper-sat`/CLI dump of
  the classify-path `clash_records` measure, or a gated test harness that runs
  classify with the probe and prints the measure. (Exact surface is a plan
  detail; it must run the **live classify config**, not a probe-only path — that
  was SP1's retracted-measurement error.)
- **Kill gate (hard-zero only, per user "commit through Stage 1"):** if full-set
  repeats are **≈0** OR concentrate **entirely shallow**, that is a definitive
  "2b DEAD" — record it and stop before Stage 1. Otherwise (any non-trivial
  deep-tail repetition, including borderline) proceed to Stage 1 for the direct
  measurement. Full-set repeat-rate is only a *lower-bound kill-check* here — a
  non-zero result does **not** prove viability (see reframe reason 2); only
  Stage 1's core-keyed net-new prune does.
- Deliverable: findings note section; ≤ ~1 measure function of new code.

**Bias controls:** run Stage 0 both **in-budget** (default deadline +
`adaptive_budget` ON) and **asymptotic** (`RUSTDL_ADAPTIVE_BUDGET=0`, large/no
deadline, depth-cap-bounded) and report both — the in-budget clash sample is
truncated by the deadline and the `DIV_WINDOW=500` divergence cut.

## Stage 1 — sound node-local core-keyed UNSAT prune (built behind a flag)

Only reached if Stage 0 is not a hard-zero kill. Because a node-local UNSAT
no-good is **sound**, the cheapest honest option is to *build the prune* behind a
default-OFF flag and measure the target metric directly — the build is ~the same
effort as a rigorous read-only probe and answers the real question (wall /
stalled-flips), not a proxy.

**Mechanism (wedge, `hyper.rs`), gated `RUSTDL_WEDGE_NOGOOD` (default OFF):**
- At a clash, extract the **core** = minimal antecedent/decision-label subset that
  forces the clash (built from `clash_deps` + the clashing node's labels).
- Store cores in a **per-solve** (per-class `decide`) store — *never* classify-
  global (avoids the `reuse-trap-A1` cross-context FP surface).
- At each branch decision (before descending), **subsumption-check** the
  candidate node's label-set against stored cores; if a stored core ⊆ the label-
  set, the branch is provably UNSAT → prune it (report the clash as if derived).

**Soundness (airtight-or-FP — this is the whole risk):**
- Only **node-local** clashes are eligible: the clash clause has no `Atom::Role`
  and no variable other than `X` (static, O(clause-length) at the empty-head
  fire site). Cardinality pre-check + merge clashes are edge/successor-dependent
  and excluded by construction.
- **Derivation-local, not just clause-local:** exclude any clash whose resolved
  clash node — or any label in the core — carries **merge taint**
  (`at_most_tainted` / `nn_tainted` / `shadow_merge_cause`). A label placed via a
  `≤n` merge makes a "node-local" clause no-good unsound to generalize.
- **Precondition (explicit, ontology-scoped):** soundness of generalizing a
  node-local no-good across nodes holds **only given no inverse roles and no
  nominals** (true for `ore_ont_10019`; the merge-taint exclusion then covers the
  sole remaining `≤n` contamination path). The flag therefore must not be trusted
  on inverse/nominal ontologies — enforced by the corpus FP gate below, and the
  design does not claim general SROIQ(D) soundness.
- Each stored core is validated by the **wedge-native node-local oracle** (a
  read-only node-local Horn re-derivation over `self.clauses`) before it is
  allowed to prune, so a mis-extracted core can never cause a false prune (FP) —
  only a MISS, caught by the closure gate.

**Measurement (the actual verdict):** with the flag ON, on `ore_ont_10019`
classify (adaptive-budget OFF and ON), report:
- **stalled-count delta** and **any class newly decided within budget** (the
  headline);
- wall delta;
- **net-new vs backjumping-redundant** prune split — for each prune taken,
  classify whether the pruned branch's decision-dep set is one backjumping would
  have unwound anyway (redundant) vs net-new; only net-new deep-tail prunes are
  2b's real contribution.

## Decision criterion (replaces the naive repeat-rate threshold)

- **2b VIABLE** iff, on the live classify path, the sound prune **flips ≥1
  currently-stalled class to decided within the existing budget** (or a materially
  lower stalled-count with a credible path to full decision), **driven by net-new
  (non-backjumping-redundant) deep-tail prunes.**
- **2b DEAD** iff repeats are shallow-only, net-new prunes are negligible, or the
  residual is confirmed depth-bound (no re-derivation to cut — the SP1 evidence
  already leans here). On DEAD, pivot per roadmap: (2a) stronger blocking, or
  bound `search.rs` as the honest tail backstop. Record the pivot rationale; do
  not ship the flag.

## Soundness invariant & cross-cutting gates

- **FP = 0 must never regress.** Because the curated corpus is mostly EL/Horn
  (disjunction-FP cannot manifest), the FP gate **must** include the **non-Horn
  adversarial oracle** (`ore_ont_13723` vs Konclude) — the exact regressor class
  that killed SnapshotCache. Curated FP=0 alone is insufficient.
- **MISSED = 0 / byte-identical curated closures.** The prune is sound, so a bug
  manifests as a MISS (dropped subsumption), not an FP — the differential/closure
  gate is load-bearing.
- Ship path (only if VIABLE): default-OFF flag → validate FP=0 **and** MISSED=0
  on curated **and** the non-Horn oracle + no curated wall regression → flip
  default-ON in a **separate reviewed commit**. Same discipline as SP1.

## Scope / YAGNI / out of scope

- Stage 0 reuses shipped infra; the only Stage-0 code is one measure + its emit.
- Stage 1 is **node-local only**, **per-solve only**, **`ore_ont_10019`-shaped
  (no inverse/nominal)**. No cross-pair cache, no edge-qualified no-goods, no SAT
  caching (anti-monotone, dropped by the roadmap).
- Out of scope: inverse/nominal-safe no-goods; wedge `≥n`/inverse-`≥n`/`≈` holes
  (merge=0 here); replacing `search.rs`; the `search.rs` CDBL (separate engine,
  lookup disabled — not revived).

## Risks

- **Low ceiling (advisor):** SP1 made re-derivation ~56× cheaper and per-solve
  scoping forbids the cross-pair win; 2b may replicate the prior CDBL's 0% wall.
  Mitigated by the Stage-0 hard-zero kill and by measuring the target metric
  (stalled-flips) directly rather than a proxy.
- **Depth-bound residual:** if the stall is purely depth-bound, no-goods cannot
  converge these classes regardless of hit-rate — Stage 0's deep-tail bin is
  designed to catch this before Stage-1 effort.
- **Core-extraction correctness:** a mis-extracted core is caught by the
  wedge-native node-local oracle gate (no FP) but could over-prune → MISS; the
  MISSED=0 closure gate catches it.

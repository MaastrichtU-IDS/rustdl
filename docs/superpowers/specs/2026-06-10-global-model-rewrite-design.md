# Spec: super-efficient global-model classification (2026-06-10)

Branch: `spec/global-model-rewrite`. Status: DESIGN (no code yet).

## 0. Problem & goal

Today's classifier is **per-pair**: a top-down walk issues a wedge
satisfiability probe per candidate `(sub, sup)`. On alehif (247 classes) that is
**16 048 wedge `decide` calls**, each rebuilding an engine over the full clause
DB; the models themselves are trivial (≤19 nodes — see
`tableau-memory-investigation-2026-06-10.md`). So the inefficiency is **probe
count × per-probe redundancy**, not model size or blocking. Konclude/HermiT
avoid this by building **per-class (pseudo-)models once** and deciding the
hierarchy by **model merging**, running the full tableau only on a small
residual.

**Goal:** replace the n²-ish probe loop with a global-model architecture that
- builds O(n) per-class models once (not O(n²) probes),
- decides the vast majority of pairs from those models in ~O(1),
- runs the expensive engine only on a small ambiguous residual,
- **preserves FP=0 (the cardinal invariant) and ≥ current MISSED** (verified by
  a differential equivalence gate against the current classifier).

Non-goal: a from-scratch consequence-based SROIQ engine (§6, the north star) —
this spec is the pragmatic rewrite that leverages existing machinery.

## 1. What already exists (the rewrite is 60% built)

- **EL/Horn saturator** (`owl-dl-saturation`): a *global* consequence-based
  engine — one fixpoint computes the COMPLETE subsumer closure for the
  saturator-complete fragment. This IS global-model classification for
  EL+functional (GALEN: 27 997 pairs in 0.5 s, MISSED=0). The Horn-shortcircuit
  (gated on `saturator_complete_fragment`) already dispatches the whole
  classification to it. **The rewrite only needs to handle the out-of-fragment
  (SROIQ) residual.**
- **Per-class pseudo-models** (Phase-7 label oracle): `classify_labels(C)` runs
  the wedge on `q⊑C` satisfiability and returns the seed node's label set
  `L(C)` (`HyperCache::classify_labels` → `satisfiability_labels`). This is
  exactly a pseudo-model in the merging sense.
- **Sound refutation already wired**: classify prunes `C ⊑ D` when `D ∉ L(C)`
  (`label_cache_pruned`) — a counterexample model, sound. Prune rates are
  96–100% (Phase 7 doc).

So the missing piece is: stop issuing a per-pair wedge probe for the un-pruned
candidates, and instead decide them from a proper **model-merge + sound-confirm
+ minimal-residual** pipeline.

## 2. THE soundness boundary (non-negotiable — the reuse trap)

A pseudo-model `L(C)` is **sound for REFUTATION, unsound for CONFIRMATION**:
- `D ∉ L(C)` ⟹ there is a model of `C` whose representative is not a `D` ⟹
  `C ⋢ D`. **Sound** (counterexample).
- `D ∈ L(C)` does NOT imply `C ⊑ D` — `D` may be in *this* model by coincidence
  (a non-deterministic choice), while another model omits it. **Confirming a
  subsumption from `D ∈ L(C)` is UNSOUND on the non-Horn fragment** — this is
  exactly the snapshot-cache FP bug (`snapshot-cache-fp-soundness-fix`,
  `reuse-trap-A1-scoping-2026-06-08`): replay trusted one satisfying model and
  emitted spurious subsumptions on disjunctive ontologies (30+ FP/ont on ORE).

**Therefore every architecture here REFUTES from models but CONFIRMS only via a
sound channel:**
1. told-subsumer closure (transitively closed; always sound), and
2. the EL/Horn saturator closure on the saturator-complete fragment (sound by
   construction), and
3. a tableau/wedge `Unsat` proof of `C ⊓ ¬D` for anything else (sound for any
   ontology — the wedge's `Unsat` is the trusted direction).

The win is that (1)+(2) confirm most real subsumptions for free, models refute
most non-subsumptions for free, and only the residual `{(C,D) : D∈L(C),
not told/saturator-confirmed}` needs a tableau `Unsat` proof. FP=0 holds because
no subsumption is ever asserted from a model's mere membership.

## 3. Architecture A — amortized pipeline + (optional) model merging (RECOMMENDED)

Two distinct things, often conflated — keep them separate (see §5):
**Phases 1–3 are DE-REDUNDANCY of already-trusted channels** (not a new
architecture — just deciding each pair from cached `L(C)` + closure before
falling to a probe). **Pseudo-model merging (§4) is the only genuinely new
model-based machinery** and is the part that earns the word "rewrite."
Pipeline, all over the out-of-fragment class set (the in-fragment part is the
saturator's job, already complete):

**Phase 1 — global base, built once.** Clausify once; build the shared
`HyperBase` (clause indexes + disjoint pairs) once; run the saturator to get the
told+EL closure. (Shared immutably across all parallel work.)

**Phase 2 — per-class pseudo-models, O(n), parallel.** For each class `C`,
`classify_labels(C)` → `L(C)` (seed-node label set) + an `Unsat(C)` flag
(unsatisfiable classes). Reuse the existing wedge; this is the existing label
cache, promoted from a heuristic to the classifier's backbone. n model builds,
not n². (Pseudo-model enrichment: also capture the seed node's *successor*
pseudo-model summary — role → filler-label-set — to strengthen merge tests; see
§4.)

**Phase 3 — decide each pair from models + sound confirm.** For candidate
`(C, D)` (drive with the top-down walk to skip transitively-implied pairs):
  - `D ∉ L(C)` → **not subsumed** (sound refute). [most pairs]
  - `D` is a told/saturator subsumer of `C` → **subsumed** (sound confirm). [most
    real subsumptions]
  - else (`D ∈ L(C)`, unconfirmed) → **residual** — queue for Phase 4.

**Phase 4 — residual tableau, minimized.** Only residual pairs get a wedge
`decide(C ⊓ ¬D)` (the current per-pair probe). Target: |residual| ≪ n²
(empirically the un-pruned, un-told set is tiny — Phase 7 prune rates imply a
few % of pairs). This is where the 16 048 probes should collapse to hundreds.

**Efficiency levers layered on top:**
- **Model-merge refutation beyond labels (§4):** before a residual tableau call,
  attempt a sound *pseudo-model merge* of `C` and `¬D` — if their pseudo-models
  are obviously compatible (no shared disjointness/clash on the merged
  label+successor summary), refute without the tableau. (Konclude's core trick.)
  Sound for refutation only.
- **Shared engine base:** residual probes use `HyperEngine::with_base` (the
  hoist from `worktree-agent-a92e8f2fc7cea09ac` — verdict-equivalent, currently
  no standalone win, but it removes per-probe clause re-clone which matters once
  probe count is the only cost). Re-evaluate its benefit when probe count is
  already minimized.
- **Top-down + bottom-up traversal** to prune transitively-entailed pairs from
  the candidate set before Phase 3 (already partly present).

## 4. Pseudo-model merging — the precise sound test

A pseudo-model of `C` is `(L(C), succ(C))` where `succ(C)` maps each role `R`
to the multiset of filler pseudo-model summaries demanded by `C`'s `∃R`/`≥R`
labels. Two pseudo-models *merge* (are jointly satisfiable) unless a clash is
forced: a shared class and its negation/disjoint partner co-occur, or a
functional/`≤n` role forces a merge of disjoint fillers. To decide `C ⊑ D`:
- build the pseudo-model of `C` and of `¬D` (or inject `¬D` at C's root);
- if the merge is **clash-free** → `C ⊓ ¬D` satisfiable → **not subsumed**
  (SOUND refute, because a clash-free pseudo-merge witnesses a real model under
  the standard pseudo-model correctness conditions);
- if the merge **clashes** → INDETERMINATE (the clash may be reparable by
  backtracking) → fall to the tableau. **Never confirm from a clash.**

This keeps the confirm-channel sound (§2) while letting merges do most of the
refutation work the full tableau currently does. The exact clash conditions are
the Konclude/HermiT "pseudo-model merging" rules (Haarslev–Möller 2001; Glimm
et al. for HermiT) — adapted to the wedge's clause model.

**Soundness subtlety — §4 risks a false REFUTATION (MISSED), not an FP.** §2's
asymmetry protects FP (we never confirm from a model). It does NOT protect §4:
"clash-free pseudo-merge ⟹ real model ⟹ not-subsumed" is sound *only* under the
FULL pseudo-model correctness conditions, which for SROIQ with **inverse roles**
and **`≤n`/number restrictions** are notoriously subtle (Haarslev–Möller's
conditions needed patching; a flat label+successor summary cannot see an inverse
role propagating a label back to the merged predecessor, nor a `≤n` forcing a
merge of fillers). A naive "no shared clash" merge can therefore declare
clash-free when the real models do clash → it wrongly **refutes a true
subsumption → MISSED** (a silent C2 violation — the very thing the calibrated-
incompleteness story forbids). So the merge must be conservative in the REFUTE
direction too: **clash-free ⟹ refute ONLY when the pseudo-model conditions
provably hold** (no inverse/`≤n` interaction on the merged core); on ANY doubt,
fall to the tableau. This is exactly analogous to D11's `definitely_disjoint`
but guarding MISSED rather than FP — exhaustive negatives-first canaries on the
clash conditions are the only safety net (the corpus won't exercise them).

## 5. Phasing & verification

- **P0 — THE GATE (do this first; it decides whether the project exists).**
  A ~20-line instrumentation patch: count, per ontology, pairs *refuted by
  labels*, *confirmed by told/saturator closure*, and *residual sent to the
  tableau*. Run on alehif / sio / ore-10908 / ore-15672 / wine. **Everything
  below is conditional on the residual being large.** There is an unresolved
  tension P0 must explain: the 16 048 alehif probes were measured *with the
  label heuristic already active*, yet Phase 7 reports 96–100 % prune rates —
  so either pruning is NOT hitting on alehif (then P1 helps), or probe count is
  not actually the wall (then there is no project and the residual work is
  elsewhere — likely the allocator churn already characterized). **If the
  residual is already small, STOP — P1/P2 buy nothing.** (This session twice
  mis-predicted a perf cause; P0 is the go/no-go, not a warm-up.)
- **P1 — DE-REDUNDANCY (not a new architecture).** *If P0 shows a large
  residual:* reorder the classify pipeline so each pair is decided from the
  ALREADY-SOUND channels in cost order — refute from cached `L(C)`, confirm from
  told+saturator closure, and issue a tableau probe ONLY for the residual —
  instead of re-deriving from scratch per pair. This is amortization of existing
  trusted channels, **zero new soundness surface**, not "global model." Should
  cut probe count with no calculus change. **Differential gate: byte-identical
  hierarchy vs current classifier** (flag-gated A/B), FP=0/MISSED=0.
- **P2 — pseudo-model merging (§4): the only genuinely new model-based
  machinery (the actual "rewrite").** Shrinks the residual further. Adds the one
  new FP-AND-MISSED-critical component (the merge clash conditions, sound in
  BOTH directions per §4) — own flag, conservative (fall to tableau on doubt),
  differential + FP=0/MISSED=0 gate, and an exhaustive merge-condition unit
  suite (D11 `disjoint()`-style negatives-first canaries, guarding both spurious
  confirm AND spurious refute).
- **P3 — successor-summary enrichment + traversal pruning** for the long tail.

Verification gates (every phase): env-flag A/B; differential equivalence
(hierarchy identical to current, not just both-FP=0); full FP=0/MISSED=0 corpus
gate; perf re-measure (probe count, wall, RSS) on alehif/sio/ore-10908/
ore-15672/wine/GALEN. Ship a phase only when the differential is clean.

## 6. Architecture B — consequence-based SROIQ (north star, NOT this spec)

The textbook "global model": one saturation computing the full hierarchy with
no per-pair anything, extending the EL saturator's calculus to SROIQ
(Sequoia-style consequence-based SROIQ, Bate et al. 2016). Eliminates the
tableau entirely. Far larger (a new calculus + completeness proof), higher
risk. Recommended only if A's residual stays large after P2. Out of scope here.

## 7. Risks & non-starters (learned this session)

- **The reuse trap (§2)** is the #1 risk; the snapshot cache died on it. Mitigated
  by the refute/confirm asymmetry — confirmations never come from a model.
- **Model-merge clash conditions** (§4) are FP-critical; treat like
  `definitely_disjoint` (conservative, exhaustive canaries, fall-to-tableau on
  doubt).
- **Don't re-propose** sub-tableau/snapshot caching as the mechanism
  (`sub-tableau-caching-already-shipped`, `snapshot-gate-loosening-dead-end`):
  that is model REUSE for confirmation, which is the unsound direction. This
  spec reuses models only for refutation.
- Scope honesty: this is SROIQ-path work, **outside the EL/Horn embeddable
  niche** (which is already global-model-complete via the saturator). Pursue
  only if SROIQ classification performance becomes a goal.

## P0 RESULTS (2026-06-10) — MIXED; conditional go, not a clear win

Ran P0 (wall-breakdown + wedge-cost histogram, which is more informative than
the planned refute/confirm counters — it reveals WHERE the wall goes). The wall
is `tier_walk`-dominated everywhere (`tier_walk = total − setup`, and it
*includes* the probe calls), but the split between walk-overhead and probe-time
is **ontology-dependent**:

| fixture | wall | tier_walk | probes | probe-cost split | bound by |
|---|---|---|---|---|---|
| alehif | 6.5s | 6305 ms | 16 048 | 15 859 @ 0ms (≈free) | **walk-overhead** |
| ore-10908 | 23s | 22 901 ms | 6 881 | **645 @ 20–49 ms ≈ 19 s** | **probe-time** |
| ore-15672 | 140s | 139 253 ms | 1 969 | (likely probe-time: 1969/139s) | probe-time (inferred) |

**The go/no-go is split:**
- **alehif (the ontology that motivated this whole thread) is WALK-OVERHEAD-bound
  — probes are ~free.** The global-model rewrite (reduce probe count) would NOT
  have helped it. Its 6.1 s is the top-down traversal itself
  (`find_direct_parents_top_down`: candidate enumeration, label-oracle lookups,
  closure checks), and its 1.6 GB is glibc arena churn — neither is probes.
- **ore-10908 IS probe-bound** — 645 medium-cost probes (~30 ms) drive ~19 s of
  23 s. Reducing probe count would help **IFF those 645 are model-REFUTABLE**
  (the label cache already pruned 26 140 cheaply; the 645 are the ones that
  passed `D∈L(C)` and needed a full probe). Whether they refute-after-probe
  (rewrite helps) or confirm as real subsumptions (rewrite can't help — they
  still need the tableau) is **UNMEASURED** — that is the missing P0 datum.

**Verdict: do NOT commit to the rewrite yet.** It is a *partial, conditional*
win (helps probe-bound SROIQ like ore-10908 only if the expensive probes are
model-refutable) and does nothing for the walk-overhead-bound headline case
(alehif). Two cheaper, targeted measurements gate any build:
1. **Refute/confirm split of ore-10908's 645 expensive probes** — instrument
   their verdicts (Unsat=confirm vs Sat=refute). If most are Sat (refutable),
   P2's pseudo-model merging is justified; if most are Unsat (real
   subsumptions), the rewrite cannot help and the tableau is doing necessary
   work.
2. **Profile alehif's tier-walk overhead** (`find_direct_parents_top_down`) —
   this is a *separate, possibly broader* lever (it's pure walk cost, present on
   every hybrid ontology) and may beat the rewrite for less effort.

Net: P0 reframes the target. The clear, ontology-independent lever is the
**tier-walk traversal overhead**, not per-pair probing (which is free on alehif
and only conditionally reducible on ore-10908). The rewrite stays specced but
**parked pending measurement #1**.

## 8. Expected payoff

alehif: 16 048 probes → a residual of (hopefully) hundreds; the EL-saturator
already handles GALEN-class. The win is wall + the parallel-alloc churn behind
the memory artifact (fewer probes → less arena churn). To be *measured* in P0,
not assumed — this session twice mis-predicted a perf cause; P0's residual count
is the go/no-go signal for P1+.

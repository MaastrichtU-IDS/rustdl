# 1-UIP conflict-learning for the merge-free disjunctive fragment — design

**Goal:** make the complete wedge search tractable on the **merge-free
non-nominal disjunctive** DL fragment via 1-UIP clause learning, closing the ORE
DL completeness tail (273 silent-MISSED pairs, ~145 search-hard) and speeding
SROIQ classification broadly. **Pure ROI: wall-only, FP=0** (completeness/perf,
not soundness).

This supersedes/refocuses the 2026-06-06 kickoff
(`docs/1uip-cdcl-kickoff-2026-06-06.md`, on branch `docs/1uip-cdcl-kickoff`): same
1-UIP design, **different target** — and the new target is the fragment where the
kickoff's load-bearing "merge-free" assumption *actually holds*.

## 1. Why this is a GO now when 1-UIP was NO-GO before

The prior 1-UIP spike NO-GO'd on **wine** (`docs/...wine-perf-attribution-and-gonogo`,
`diag/wedge-stall-1uip-nogo`). Root cause: **nominal-merge** (`merge_with_cause`
folds the merge *causation* into `birth_deps`) makes every clash depend on the
full ancestor context — bjgap≈1 — so learned clauses are leaf-bound and prune
nothing. Crucially, the kickoff's §5b "GO" measurement (`popcount(clash_deps.bits)`
= 2–3 on wine) was **misleading**: the bits undercount the merge-folded deps. The
spike exposed the true bjgap≈1.

**The non-nominal merge-free fragment has no such folding.** Profiled (2026-06-20,
`trust_sat=0`, `RUSTDL_TRACE`):
- **ore_ont_778** (Animals; ∀ + complement + disjoint; **no cardinality, no
  functional, no nominals** — genuinely merge-free): clashes at depth **248–255**
  with **deps=1**, **14,588 clashes/40s**, tiny **446-node** graph, hits the
  depth-256 cap. A deps=1 clash at depth 255 means a learned nogood backjumps
  ~254 levels; the search re-derives the same deps=1 clash ~14k× purely because
  the wedge **backjumps but does not learn**. Textbook CDCL pathology.
- **ore_ont_9786** (SIO): deps=2–3 at depth 200–237, 41k-node graph — BUT it has
  46 ExactCard + 34 Min + 42 Functional ⟹ **≤n merges present** ⟹ its deps may be
  merge-folded like wine. **9786 is NOT a clean target**; re-verify its true deps
  before counting it in scope.

So: on the merge-free fragment the deps=1 are *true* (no merge confound), bjgap is
huge, and 1-UIP is the standard, sound, terminating CDCL win the wedge currently
lacks. The wine NO-GO does not transfer.

## 2. Where the lever goes (architecture)

The explosion is in the **wedge** (`crates/owl-dl-tableau/src/hyper.rs`; depth cap
`HYPER_WEDGE_DEPTH=256`). The wedge has dependency-directed **backjumping**
(`DepSet`: a `u128` of decision levels + overflow flag) but **no learning**. The
PR #19 conflict-learning foundation (`learned_nogoods`) is in the **main tableau**
(`search.rs`/`TableauContext`), NOT the wedge — so it is *not* directly reusable;
the learning infrastructure must be built in the wedge.

## 3. The change (reuse the kickoff design)

Per `docs/1uip-cdcl-kickoff-2026-06-06.md` §3–§5:
- **Antecedent records.** Per derived label, `Antecedent { clause_id, body:
  [LabelRef] }`; `LabelRef` keyed by **canonical (provenance) node id** (merge-
  /≥n-stable), resolved through union-find. Decision labels (the asserted
  disjunct) are roots tagged with their decision level. Threaded through the
  wedge's horn fixpoint; survives `save`/`restore` (per-label state like
  `label_deps`).
- **1-UIP analysis.** At a clash, resolve backward over the implication graph
  (pop the most-recent current-level label, replace by its antecedents) until one
  current-level label remains (the UIP). Learned clause = ¬UIP ∨ (other-level cut
  negations). Backjump to the 2nd-highest level, assert the flipped UIP with the
  learned clause as antecedent. (The far backjump-and-assert is the subtree
  pruning simple dep-set nogoods structurally cannot do — why PR #19 got only 13%.)
- **Merge-free restriction (the soundness+tractability gate).** Require the
  conflict cut to be **merge-free / non-overflow** (merge-`≠`-NN contributions
  already carry `DepSet::ALL`/overflow — exclude them, fall back to current
  backjumping). On the 778-class fragment this excludes nothing; on cardinality/
  merge onts it degrades gracefully to today's behavior. This is exactly the
  kickoff's "first cut" — now aimed at the fragment where it is lossless.

## 4. Secondary fix folded in: DepSet depth > 128

`DepSet` is `u128` (levels 0–127); `singleton(level≥128) → overflow → DepSet::ALL
→ chronological`. The search reaches depth **256**, so backjumping silently
degrades in the deep half. Extend decision-level tracking past 128 (e.g. a small
`Vec<u64>` bitset or a level→slot remap) so deep clashes keep precise deps — or
the 1-UIP cut overflows exactly where the action is. Gate identically (FP=0,
byte-identical OFF).

## 5. Plan — SPIKE FIRST on 778 (the clean target), not wine

The kickoff's spike plan, retargeted to the merge-free fragment:
- **Milestone A — antecedent recording only (no learning).** Record the
  implication graph through the wedge fixpoint, surviving save/restore. **Gate:
  corpus closure-diff byte-identical** (pure bookkeeping, FP=0/MISSED=0). If it
  doesn't come together cleanly on merge-free 778, **stop** — cheap failure.
- **Milestone B — restricted 1-UIP on 778.** 1-UIP on merge-free conflicts,
  flag-gated. **Go/no-go:** does 778's `branches` fall **super-linearly** (subtree
  pruning beyond PR #19's 13% leaf pruning) and the hard pairs resolve within the
  classify budget? 778's 14k deps=1 re-derivations should collapse. If yes →
  commit to the build; if no → 1-UIP is a genuine architectural limit even
  merge-free, NO-GO, bank.
- **Build (multi-week, only if B is GO).** Full antecedent graph + save/restore;
  non-chronological backjump + learned-clause assertion; watched-literal
  propagation for learned clauses; the DepSet>128 fix; the full FP gate.

## 6. Validation gates (every milestone)

- **FP=0 corpus-wide** + **verdicts byte-identical** flag-OFF (the sacred gate).
- **Differential fuzz**: learning-ON ≡ OFF on random SROIQ.
- **ORE pilot**: the merge-free DL-tail onts (778-class) MISSED↓, FP_strict=0/197.
- **Flag default-OFF** until the bake-off; wall non-regression on the EL/Horn corpus.

## 7. Scope / honesty

- **In scope:** the merge-free non-nominal disjunctive fragment (778-class).
  Reward bounded by how many of the 273-pair tail are merge-free — **MEASURE THIS
  FIRST** (partition the DL-tail onts by cardinality/nominal presence; 9786-class
  is out until its true deps are re-verified).
- **Out of scope:** nominal/merge-heavy (wine; 9786 pending re-check) — stays
  backjump-only; the merge-free restriction degrades to today's behavior there.
- **Honest framing:** genuine multi-week research-grade work (1-UIP over a
  partially-mutating graph). The spike is the decision point. But unlike the wine
  attempt, the target fragment's merge-freeness is *verified*, so the §5b
  confound that sank wine is structurally absent here.

## 8. First concrete step
Partition the 273-pair DL tail by merge-freeness (construct scan: no
cardinality/functional/nominal ⟹ merge-free) to size the in-scope reward, then
Milestone A on 778.

## 9. Scoping outcome (2026-06-20): viable but ROI-NO-GO — DO NOT BUILD (yet)

Ran §8's partition. DL tail = 273 pairs: **merge-free 70 / merge-cardinality 203**.
Of the SEARCH-HARD pairs (the only ones 1-UIP addresses — the rest are fast-tableau
pairs masked by label/trust_sat, a different problem): **merge-free = 59, all in ONE
ontology (ore_ont_778)**; merge/cardinality search-hard = 86 (9786/7339/12698/7532/
12723 — the wine-confound fragment, deps likely merge-folded ⟹ 1-UIP preconditions
suspect, expected NO-GO like wine).

**Conclusion: 1-UIP is technically GO on the merge-free fragment (contra wine) but
the clean reward is 59 pairs in ONE ontology — a multi-week research-grade build for
one ontology, unable to touch the 86-pair merge-bulk. ROI-NO-GO.** The DL tail is
fundamentally the wine-class cardinality/merge architectural limit (203/273 pairs).
Bank. Revisit only if a real workload surfaces a LARGE merge-free disjunctive
fragment (then 778 + Milestone-A spike is the entry point). Measure-first (the
partition) prevented the multi-week build.

# Depth-reducibility probe — `ore_ont_10019` (2026-07-16)

**Why:** the "remaining closer = whole-model caching / CDCL" claim was circling — that
family is already measured out (SP2 node-local no-goods DEAD/all-net-new; snapshot
replay reuse-trap FP; simple nogoods 13%; 1-UIP NO-GO). SP2's own pivot said the stall
is **depth-bound, not re-derivation-bound**, and named the correctly-targeted lever:
reduce the disjunctive-decision-stack DEPTH via **blocking**, not a cache. This probe
tests whether that depth is reducible — *before* building anything.

**Method:** gated (`SB_DEPTH_PROBE`) instrumentation at the wedge `⊔` decision recording,
per decision: level `d`, `succ` (`parent.is_some()` — a generated successor, blockable
in principle, vs a root/named node), `blk` (currently blocked), and `anyblk` (does some
earlier canonical node carry a **superset** of this node's labels — the anywhere-blocking
precondition). Run on `ore_ont_10019` via `hyper-sat --per-class-timeout-ms 500`,
`RUSTDL_ADAPTIVE_BUDGET=0`, ~52 k decisions.

## Results (by depth bucket)

| level | decisions | on generated successor | currently blocked | **anywhere-blockable** |
|---|---|---|---|---|
| 0–31 | 1 112 | 77 % | 0 | **0 %** |
| 32–63 | 8 020 | 100 % | 0 | **0 %** |
| 64–95 | 31 367 | 100 % | 0 | **0 %** |
| 96–127 | 11 745 | 100 % | 0 | **0 %** |

## Findings

- **Q1 — where the deep decisions sit:** ~100 % on **generated ∃-successors**, not
  root/named nodes. So the stall is NOT the "unblockable root" dead-end; the decisions
  are on nodes where blocking *could* apply in principle.
- **Q2 — is the depth reducible by blocking:** **NO.** `anyblk = 0 %` everywhere — no
  earlier canonical node is a label-superset of a ⊔-decision node. The generated
  successors have **non-recurring** label structure, so *no* blocking condition (ancestor,
  double, or anywhere) has anything to block against. Blocking is **dead** here — for a
  different reason than caching: not "unblockable position," but "nothing recurs to block."

## What this settles vs. leaves open

**Settled (each eliminated with data, not caution):**
- Caching / CDCL / no-goods family — DEAD (depth-bound, not re-derivation-bound; SP2 sweep).
- Blocking / depth-reduction (roadmap 2a) — DEAD (0 % anywhere-blockable; nothing recurs).
- Decision ordering — the stack is a dependent generative ∃-chain of growing-label
  successors; ordering can't shallow a forced generative chain.

**The one genuinely-untried lever the measurements point at — and its catch.** SP2's
Phase-A measured `revisit_frac ≈ 1.0` on the **label-set-hash** (the same exact whole-node
state recurs ~1.0 over the search), yet this probe shows `anyblk = 0 %` (no coexisting
superset). Those are consistent: the recurrence is a **search-path** phenomenon (the DFS
regenerates the same states via different ⊔ orders), NOT a **graph-structure** one
(coexisting superset). Blocking exploits graph-structure recurrence → dead. A **within-
search transposition memo keyed on the whole-node label-set-hash** exploits search-path
recurrence → matches the measured `revisit_frac ≈ 1.0`. SP2 never built this: its Phase-B
cache keyed on the **clash core** (distinct → all-net-new), not the label-set-hash (which
recurs). This is distinct from the reuse-trap (cross-query SAT-model reuse) and from
node-local no-goods.

**Its two unresolved risks (why it's a spike, not a build):**
1. **FP surface (reuse-trap family).** A within-search verdict memo is sound only if the
   key fully determines the subtree AND the memoized `Unsat` dep-set is handled soundly
   (a cached `Unsat`'s backjump dep-set is path-dependent). Getting this wrong is a false
   `Unsat` = FP subsumption — the exact hazard that bit Layer B and the snapshot cache.
2. **SP2's "depth-bound" caveat.** SP2 argued a memo "cannot convert a deadline-bound
   stall into decided classes" because it doesn't shorten the *first* descent's depth.
   BUT if `revisit_frac ≈ 1.0`, most descents are *re*-descents → memoizing them cuts
   total work by ~the revisit factor → could beat the deadline. This tension is
   **genuinely unresolved** and is the spike's go/no-go.

## Recommended next step (measure-first spike, NOT a commitment)

A minimal within-search transposition memo: at `solve` entry, hash the resolved node's
label-set (reuse the existing 64-bit `label_sig`); if the same (node-signature, remaining-
disjunction-context) was already resolved to `Sat`/`Unsat` *in this decide()*, reuse it —
`Sat` short-circuits (sound); `Unsat` reused only with a sound (superset/`ALL`) dep-set.
**Gate:** curated MISSED=0 + the non-Horn `ore_ont_13723` FP oracle FP=0 (this one IS an
FP risk, unlike bound-the-tail) + does `ore_ont_10019` decide materially more classes
within budget. If the memo hits are all-net-new (like SP2's cores) or the FP gate trips →
DEAD, and the honest terminal verdict stands: **`ore_ont_10019`'s stall is a search-path-
revisiting disjunctive DFS whose only remaining cure is a sound within-search state memo;
if that fails its gate, the tail needs a different search architecture, not an
incremental knob.**

The probe instrumentation was throwaway (reverted; `SB_DEPTH_PROBE`). Numbers preserved here.

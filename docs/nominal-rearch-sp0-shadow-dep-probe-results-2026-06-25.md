# SP-0 shadow precise-dependency probe — RESULTS + VERDICT (2026-06-25)

**VERDICT: NO-GO.** The per-fact dependency-node graph (CMERGED*) — the mechanism the
deep nominal rearch was committed to — is the **wrong mechanism**, proven *before*
building it. On wine's stalling class, precise per-fact merge causation recovers
**nothing** beyond the real (imprecise) deps: `shadow≠real = 0` across all 658k+ clash
records, and `bjgap_shadow == bjgap_real` identically in every class. The dense
clash-dependency chains are **genuine semantic structure, not an artifact** of imprecise
tracking. → The remaining route to Konclude-class wine is Konclude's *fully-integrated*
engine reimplemented (the search architecture itself), not a dependency-tracking
foundation port.

## The premise this gate killed

The committed thesis: wine's wall is dense clash chains created by imprecise
nominal-merge tracking (`at_most_tainted`/`nn_tainted` → `card_clash_deps` returns
`DepSet::ALL`, bjgap≈1); a precise per-fact dependency graph would sparsify them,
unlocking backjumping + UNSAT-caching + CDCL + model-reuse together. SP-0 built a
**read-only shadow** that computes the precise merge causation the live engine
discards, and measured whether it sparsifies the chains — without building the graph.

## Read-only validity (the measurement is trustworthy)

Wine closure, flag-OFF vs flag-ON (`RUSTDL_SHADOW_DEP_PROBE=1`), 25 ms/pair:
both `rustdl=653 konclude=653 FP=0 MISSED=0 unsat:rustdl=0`. **Byte-identical** — the
shadow layer perturbs no decision or verdict (unlike SP-A, which broke unsat). The
Task-1 NN-merge canary separately proves the shadow *can* differ from real (it recovers
`{level 0}` where real=`ALL`), so `shadow≠real=0` on wine is a real finding, not a no-op.

## The measurement (probe ON, adaptive OFF, depth 256)

| class | verdict | branches | clashes | real=ALL | **shadow≠real** | bjgap real | bjgap shadow |
|---|---|---|---|---|---|---|---|
| Alsatian ⊓ ¬American | Sat | 1 227 | 610 | 6 | **0** | med 2 / mean 2.76 | med 2 / mean 2.76 |
| SweetWine | Sat | 12 366 | 7 807 | 2 271 | **0** | med 2 / mean 2.44 | med 2 / mean 2.44 |
| **Zinfandel** | **Stalled** | **1 002 858** | **638 515** | **495 813** | **0** | **med 1 / mean 1.22** | **med 1 / mean 1.22** |
| WhiteNonSweetWine | Sat | 325 | 155 | 3 | **0** | med 2 / mean 2.78 | med 2 / mean 2.78 |
| RedTableWine | Sat | 34 645 | 17 154 | 0 | **0** | med 2 / mean 2.77 | med 2 / mean 2.77 |

## What it means (artifact vs genuine — the answer is GENUINE)

1. **Precise deps recover nothing on wine.** `shadow≠real = 0` on every record of every
   class, including all 495,813 taint-fired records on Zinfandel. `bjgap_shadow ==
   bjgap_real` identically. The per-fact dependency graph would track the *same* deps,
   precisely, and backjumping would be *unchanged*.
2. **Zinfandel's wall is genuinely-global clashes.** The taint→`ALL` fires on 78% of its
   clashes, but those are **partition-exhaustion `≤n` clashes** (`solve_at_most`), whose
   Unsat genuinely depends on the whole branch context — `ALL` there is *sound by
   necessity*, not imprecision (the existing `card_clash_deps` doc says so; the shadow
   correctly computes `ALL` too). The *recoverable*-taint clashes (NN/card-merge, where
   the shadow can be precise — proven by the canary) essentially **do not fire on wine's
   hot path**. So there is nothing to sparsify.
3. **bjgap is already fine where it can be, and genuinely 1 where it stalls.** The
   satisfiable classes already backjump (median 2, mean ~2.8) and terminate fast. The
   stalling class (Zinfandel) is pinned at median 1 under *both* real and precise — a
   genuine global dependency, not a tracking artifact.
4. **Reuse is degenerate, not learnable.** Zinfandel's 638k clashes map to 26 "distinct
   nogoods" with `reusable_nogood_frac=1.0` — but the dominant nogood is the global `ALL`
   set, not a compact learnable lemma. CDCL/memoization cannot exploit it. The
   `revisit_context_shared_frac` (0.21–0.80) quantifies the reuse-trap surface: a large
   fraction of revisited states recur under *differing* dep context (the snapshot-cache
   FP, historically fatal).

This **corrects the prior hypothesis** ([[wine-wall-bjgap1-genuine]]: "dense dep chains
from imprecise merge tracking"). The chains are dense because the stalling clashes are
**genuinely global** (partition exhaustion over nominal-merged `≤n` successors), not
because tracking is imprecise. Precise tracking changes nothing.

## Consequence (the evidenced fork for the user)

NO-GO is *not* "floor." It is the specific, evidenced input the user committed to obtain:

- **The per-fact dependency-node graph (CMERGED*) is the wrong mechanism** — it would
  track the same genuinely-global deps precisely; backjump/CDCL would still be pinned.
  Do **not** build it for wine.
- **The only remaining route to Konclude-class wine is the fully-integrated engine
  reimplemented** — where `≤n`-over-nominals is *not* solved by a backtracking partition
  search that manufactures globally-dependent clashes (Konclude's nominal+cardinality
  architecture avoids creating them). That is a from-scratch *search-architecture*
  change, not a dependency-tracking foundation.
- **Or stop the wine chase:** the wall is wall-time only on this fixture; `MISSED=0`
  already holds via `--pair-timeout-ms 25`, and rustdl is sound + knob-complete
  corpus-wide and Konclude-class on EL/Horn.

## Method note

7th convergent wine check; the cheapest (read-only instrumentation, zero behavior
change), and it killed the most expensive proposed build *before* building it. The
shadow layer is the foundation a CMERGED* build would have needed — retained on the
branch — but the measurement says that build would not have paid off.

## Disposition

Spike code on `feat/nominal-rearch-sp0`, **unmerged**; only this verdict doc is durable.
`main` untouched.

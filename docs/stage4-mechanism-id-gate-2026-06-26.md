# Stage-4 — mechanism-identification gate (from-scratch engine)

**Status:** verdict (durable). After the user committed to a from-scratch nominal-calculus
reimplementation, the advisor reframed the first step as mechanism *identification* (building the
wrong mechanism is the documented failure mode — precise-merge-deps FP=232, per-fact-graph 1/77
were two wrong cuts). Two of the obvious mechanisms were already refuted (CMERGED*/precise-deps =
SP-0; deterministic-expansion = det-resolution gate 18–34%; reuse/caching = minimal-sound-key gate).
The only un-refuted family was **algebraic cardinality** (Faddoul–Haarslev SHOQ algebraic tableau).
This gate tests it.

## Hypothesis

SP-0 found 78% of the stalling class's clashes are *genuinely-global partition-exhaustion ≤n* —
"globally-infeasible cardinality discovered by exhausting partitions" is precisely what algebraic
reasoning computes directly instead of by search. **But** the wine wall is 952k *disjunction*
branches (merge≈0): the clashes are cardinality, the branches are value-choices. Algebraic
cardinality only collapses wine if arithmetic ≤n-over-nominal feasibility, **evaluated at the ⊔
frontier**, prunes those disjunction branches — not just the within-branch partition search.

Advisor correction (load-bearing): rustdl **already has** the algebraic check —
`forced_distinct_exceeds` is labeled "Algebraic cardinality clash pre-check"; merge≈0 confirms it
carries the ≤n load, but it fires at the *leaves* (in `solve`, after the ⊔ branch), not at the
frontier. So the real mechanism in question is **propagating it early** (cardinality-aware
forward-checking), and the prior NO-GO "forward-checking ~1.1× net-negative corpus-wide" must be
reconciled — it measured *wall*, not *prune potential*.

## Probes (read-only, on sat(Gamay), adaptive-budget OFF)

1. **Coupling (RUSTDL_CARD_COUPLE_PROBE)** — necessary-condition upper bound: what fraction of ⊔
   points are on nodes coupled to a `≤n`/`=n` constraint.
2. **Would-prune (RUSTDL_WOULDPRUNE_PROBE)** — 1-step look-ahead at each ⊔ point: per disjunct,
   apply → `horn_fixpoint` → classify horn-killed (fixpoint Unsat) vs arith-killed (open `≤n` with
   `forced_distinct_exceeds` — the existing algebraic check, moved to the frontier). Reports
   `collapse_horn` (⊔ pts reduced to ≤1 survivor by Horn alone) vs `collapse_aug` (Horn +
   arithmetic ≤n), and the **marginal** = the extra collapse algebraic cardinality buys. This is
   the prior FC look-ahead probe (UPDATE #3, horn-clash predicate, ~9% forced) **plus** the
   arithmetic ≤n term the prior probe lacked. Measures PRUNE POTENTIAL, not wall — the
   discriminator the wall-only FC NO-GO couldn't give. Capped at 20k probed ⊔ points.

## Result

**Coupling: ~82%** — `node_card == node_atmost == 41061/50000` (stable). The disjunction branching
is strongly coupled to cardinality (encouraging; not decoupled).

**Would-prune (20000 ⊔ points):**

| metric | value |
|---|---|
| `collapse_horn` | 4384 (~21.9%) |
| `collapse_aug` (Horn + arithmetic ≤n) | **4384 — identical** |
| **`marginal`** | **0** |
| `arith_marginal_killed` | **0** |

## Verdict — algebraic cardinality REFUTED as a wine lever

Algebraic ≤n-over-nominal feasibility at the ⊔ frontier prunes **exactly nothing** beyond Horn
propagation. The 82% coupling is real but **inert**: the constraints are present, but not
arithmetically infeasible *at the decision frontier* — the infeasibility emerges only deep, after
enough value-choices accumulate to materialize >n distinct successors. `forced_distinct_exceeds`
fires in the real search (78% of clashes, SP-0) but **never at the frontier**, where a mechanism
would need it. This confirms (and sharpens, with prune-potential rather than wall) the prior
forward-checking NO-GO: the wine wall is the **disjunctive value-assignment search**, and cardinality
bookkeeping (Horn or algebraic) is not the cost — merge≈0.

The advisor's pre-committed reading: low prune potential ⇒ the FC NO-GO is confirmed, **algebraic is
dead too**, the read-only counter saved the build. Per his framing, "small fraction ⇒ even a
from-scratch nominal calculus won't collapse wine; the speed is in Konclude's *full combination* — a
far bigger thing than 'nominal calculus,' a scope change the user should re-confirm before building."

## Where this leaves the engine program

Mechanism identification is now **exhausted with no transplantable lever found**:

- CMERGED*/precise-deps — SP-0 (shadow=real, 1/77).
- deterministic-expansion — det-resolution gate (18–34% < 70%).
- sound reuse/caching — minimal-sound-key gate (dense; no sparse sound key).
- forward-checking / propagation — UPDATE #3/#4 (≈9%, net-negative wall).
- **algebraic cardinality at the frontier — THIS gate (marginal=0).**
- conflict-driven learning / 1-UIP — earned NO-GO via bjgap≈1 (dense deps, re-confirmed by the
  minimal-sound-key gate's whole-graph result).

Every candidate is defeated by the same structure: wine's value-assignment search is genuinely
combinatorial, and every pruning mechanism is blocked by the dense ancestor-dependent nominal-merge
deps. There is **no identified single mechanism** — not even Konclude's known techniques applied to
rustdl's representation — that collapses wine. A "from-scratch nominal-calculus reimplementation" is
therefore, more precisely, a **wholesale reimplementation of Konclude's entire integrated
representation + calculus + search** — betting that the *gestalt* achieves what no identifiable piece
does, with no mechanism-probe able to de-risk it before the (very large) build. The alternative is to
treat the sound 15× ∃-seed win (wine 49s→3.2s, FP=0 corpus-wide, default-ON main) as the arc's
terminal deliverable, with the 8-class genuine core documented as the open frontier.

## Scope / provenance

Branch `feat/stage4-engine-characterization` (off the ∃-seed merge `ee6904c`). `main` untouched.
Probes (`RUSTDL_CARD_COUPLE_PROBE`, `RUSTDL_WOULDPRUNE_PROBE`, `memo_node_key`/`RUSTDL_MEMO_KEY`) and
tests (`stage4_cardcouple_gamay`, `stage4_wouldprune_gamay`, `stage4_minkey_gamay`) are throwaway,
gated, `#[ignore]`d. Measurement integrity: `forced_distinct_exceeds` is the same function that fires
in the real search (SP-0's 78% clashes) — the probe's 0 at the frontier is a real structural fact
(clashes are deep), not a dead function.

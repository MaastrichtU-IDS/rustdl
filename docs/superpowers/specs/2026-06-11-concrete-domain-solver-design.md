# Concrete-domain satisfiability solver — design (2026-06-11)

## Goal

Raise rustdl's datatype reasoning from the current preprocessing-only
DKey reduction (sound but with a hard ceiling — can't express
`DataUnionOf`/`DataComplementOf`/`DataIntersectionOf` or do live
concrete-domain counting) toward **DL-completeness for datatypes**, via a
node-level concrete-domain satisfiability checker wired into the tableau
(the Motik–Horrocks approach; the role the scaffolded `owl-dl-datatypes`
crate was meant to fill).

## Measurement caveat (read this first)

This is being built **against the measurement**. A corpus + 233-ont ORE
grep found the target constructs are rare (DataUnionOf 1, DataComplementOf
0, DataIntersectionOf 3) and **zero naturally-occurring ontologies have a
datatype-construct-driven MISSED that isn't already closed** (the one
candidate, ore_ont_15682, is a Konclude≠HermiT oracle-disagreement whose
misses are wine-sugar reasoning, not its price-`DataIntersectionOf`). The
gap is, in practice, theoretical. We build it anyway by explicit user
decision — for correctness/robustness completeness, not a measured gap.
Implication: the verification net is **synthetic canaries**, since the
corpus won't exercise it.

## Load-bearing invariants (soundness)

1. **Refute-only / purely additive.** The solver may turn a tableau node
   **UNSAT** (add a clash); it may **never** make a node SAT, prune a
   branch, or license a subsumption the tableau wouldn't otherwise reject.
   A clash is a sound contradiction; "don't know" ⇒ no clash.
2. **Single source of constraints.** The solver reads from the *same*
   lowered IR the DKey path uses — never a parallel scan that could see a
   different constraint set (the two-representation seam → false clash /
   silent miss). The FP-critical failure is two views disagreeing.
3. **UNSAT is the FP-critical direction.** A wrong UNSAT = false clash =
   false subsumption between unrelated classes. So `Unsat` must be returned
   **only when provably infeasible**; everything else ⇒ `Sat`/no-clash
   (sound under-approximation of *completeness*, never of soundness).
4. **Canary shape is the inverse of DP-1/2.** There the risk was a false
   *positive* (inconsistent); here it's a false *clash on a satisfiable
   node*. So the negatives-first suite must include **satisfiable-but-tight**
   data nodes that must stay SAT (`≥2 p.[0,10]` with room, `≤1 p` with one
   value, a genuinely-covering union). These are mostly synthetic.

## Phasing

- **P0 (this commit) — standalone integer solver, no tableau wiring.**
  `owl-dl-datatypes`: `integer_sat(constraints) -> Sat | Unsat`. Decides
  feasibility of a set of distinct integers under min-demands (`≥n p.R`,
  `∃`, `DataHasValue`), max-limits (`≤m p.S`), and a universal filter
  (`∀p.U`), over integer intervals (single-interval ranges for P0;
  multi-interval/union → conservative `Sat`). Region-partition +
  capacity/counting feasibility. Pure, exhaustively unit-tested incl. the
  satisfiable-tight cases. **De-risks the algorithm; reusable core.**
- **P1 — reals/strings/temporal capacity reasoning.** Generalize the
  capacity model (dense = unbounded unless a point-demand exhausts;
  finite-set for `StrSet`; etc.). Same feasibility frame.
- **P2 — constraint collector from the lowered IR.** At a tableau node,
  gather the active data constraints per data-property (and super-property,
  as DP-1/2 already route) from the *same* lowered concepts the DKey path
  reads. Stop dropping `DataMin/Max/ExactCardinality` and
  `DataUnionOf`/`Complement`/`Intersection` — carry them to the collector.
- **P3 — tableau clash wiring.** Invoke the solver as an additive clash
  rule on data-constrained nodes; integrate with backtracking/deps. Verify
  FP=0 corpus-wide + the synthetic SAT-node canary suite.

Each phase is independently shippable and gated FP=0. P0 has no tableau
contact, so its only risk is its own correctness (caught by unit tests).

## P0 algorithm (integers)

Constraints for one property at one node, all over `xsd:integer`(/subtypes):
- `U` = ⋂ of all `∀p.R` ranges (∅ ⇒ any positive demand is unsat); `⊤` if none.
- min-demands: `(R_i, n_i)` from `≥n_i p.R_i`, `∃p.R` (`n=1`),
  `DataHasValue(p,v)` (`R={v}, n=1`). A filler is also subject to `∀`, so
  the effective region is `R_i ∩ U`.
- max-limits: `(S_j, m_j)` from `≤m_j p.S_j`.

Feasibility: ∃ finite `V ⊆ U` (distinct integers) with `|V ∩ R_i| ≥ n_i`
∀i and `|V ∩ S_j| ≤ m_j` ∀j. Decision:
1. Collect all interval endpoints; partition `U` into regions (maximal
   integer intervals with constant membership across all ranges).
2. `cap_r` = #integers in region `r` (`hi-lo+1`, or ∞ if unbounded).
3. Feasibility of `x_r ∈ [0, cap_r]` with `Σ_{r⊆R_i} x_r ≥ n_i` and
   `Σ_{r⊆S_j} x_r ≤ m_j`. For single-interval ranges the region-subsets are
   contiguous ⇒ exact greedy; multi-interval (union/complement) deferred to
   a later phase (conservative `Sat`).

Soundly-detected infeasibility (the wins): `∀`-intersection empty +
positive demand; `≤m p.S` + `≥n p.R` with `R∩U ⊆ S` and `n>m`; `≥n p.R`
with `|R∩U| < n` (the integer-counting clash, e.g. `≥3 p.[0,1]`).

Anything not provably infeasible ⇒ `Sat`.

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

### Status (2026-06-11)
- **P0 ✅** (`f307498`) — integer satisfiability core.
- **P1 ✅** (`6568d85`) — generalized to dense (`DenseInterval`) + finite-set
  (`FiniteSet`) via the `ValueRange` trait; 27 unit tests, refute-only.
- **P2 ✅** (`77bb44d`) — DKey decode + `ClassId→CardRange` side-map.
- **P3 ✅** (`b9f8143`, local/unpushed) — integer cardinality un-drop +
  `concrete_domain_clash` (additive, refute-only) in `clash_deps_at` +
  `apply_min`/`apply_max` suppression for DKey fillers.
  **Metrics (the push gate):** FP=0/MISSED=0 corpus-wide (shoiq 449=449 with P3
  ACTIVE on its `=1 xsd:int` axiom; sio/ore-10908/ore-15672/wine/alehif
  unchanged); perf-neutral (ore-10908 solo 1.16 s = baseline; empty
  `dkey_ranges` → instant early-out; suppression prevents blowup). 10
  negatives-first canaries (`concrete_domain_clash.rs`): 3 capacity/conflict
  clashes fire, 7 per-lowering-path satisfiable nodes stay SAT.
  **Utility caveat:** real-corpus utility ≈0 (only shoiq's single satisfiable
  axiom activates) — as the pre-build measurement predicted; the win is on
  synthetic integer-cardinality constructs.
  **Scope:** integer bucket only; the clash is in the MAIN tableau (the classify
  *wedge* doesn't run it — so utility is on the `is_*`/consistency paths, not the
  classify pair-loop). Float/decimal/temporal/string buckets + wedge integration
  are future extensions.

### P2/P3 resolved integration architecture (the `ClassId → CardRange` side-map)
Two facts discovered while mapping the tableau pinned the only viable shape:
1. `TableauContext` (lib.rs:112) holds `pool`/`tbox`/`hierarchy` — **no
   vocabulary**, so the tableau cannot map a `ClassId` → IRI; and DKeys are
   interned as ordinary IRI-deduped classes (NOT a reserved range like
   nominals' `nominal_base + i`). ⇒ the tableau cannot detect a DKey class by
   itself.
2. `owl-dl-datatypes` depends on `owl-dl-core` (not the reverse), so `card_sat`
   can only be *called* from `owl-dl-tableau`/`owl-dl-reasoner`, never from
   `owl-dl-core` preprocessing (this also rules out a `data_axioms`-style
   class-level card_sat pass).

**Design:** build a `HashMap<ClassId, CardRange>` for the DKey classes **once**,
where the vocab exists (`PreparedOntology::from_internal` / reasoner), by
decoding each DKey IRI; thread it into `TableauContext` as a new borrowed field
(parallel to `tbox`/`hierarchy`). The clash rule consults the map — no IRI access
in the tableau — to (a) recognise a `∃/∀/Min/Max` filler as a data range and
(b) get its decoded `CardRange`. `CardRange` is a `owl-dl-datatypes` enum over
`IntInterval`/`DenseInterval`/`FiniteSet` with a single `card_sat` dispatch.

**P2 task breakdown (turn-key):**
1. `owl-dl-datatypes`: `enum CardRange { Int(IntInterval), Float(DenseInterval<…>),
   Dec/Date/Dt(DenseInterval<…>), Str(FiniteSet<String>) }` + a unified
   `card_sat_ranges(universal, mins, maxs)` dispatching by bucket (ranges in one
   call must share a bucket — cross-bucket never interacts, as in seeding).
2. `owl-dl-core`: a single public DKey decode point — `pub fn decode_dkey(iri)
   -> Option<DecodedDkey>` (move the `parse_*_dkey_iri` family behind it) — where
   `DecodedDkey` carries primitive bounds (no leaking the internal range structs).
3. `owl-dl-reasoner`: in `PreparedOntology::from_internal`, build the
   `ClassId → CardRange` map (decode every class IRI that `is_dkey_iri`).
4. `convert.rs`: stop dropping `DataMin/Max/ExactCardinality` whose qualifier is
   a recognised datatype range; lower to `Min`/`Max` over the DKey filler.

**P3 task breakdown (the FP-critical clash):**
5. `owl-dl-tableau`: new `TableauContext` field `dkey_ranges:
   Option<&HashMap<ClassId, CardRange>>`; `apply_concrete_domain_check(ctx,
   node)` after `apply_max`. Collect, per data-property-role at the node, the
   `∃`(`≥1`)/`Min`(`≥n`)/`Max`(`≤n`)/`∀`(filter) labels whose filler is in the
   map; run `card_sat_ranges`; if `Unsat`, add `Bot` with `DepSet` = **union of
   the deps of EVERY data label fed to the solver** (sound superset — see the
   invariants; `card_sat` should report which it used so we union exactly
   those-or-a-superset). Suppress object-expansion (`apply_min`/`apply_max`)
   for map-recognised DKey fillers (avoids materialising `n` successors
   corpus-wide — the advisor's Q4).
6. **COMPLETENESS OBLIGATION + canaries** (the real P3 risk): the collector must
   see *every* lowering path (`DataHasValue` → `∃p.DKey({v})`, sub-property
   ranges, `∀` injected post-rule, absorbed GCIs). Synthetic per-lowering-path
   SAT-node canaries that MUST stay SAT, plus the unsat capacity/conflict
   canaries. Then arm the clash; gate FP=0 corpus-wide.

P3 (5–6) is FP-critical and must not be rushed: its failure mode is a subtle,
corpus-invisible incomplete-collection clash → false subsumption. Build the
collector with the completeness obligation and per-path SAT canaries BEFORE
arming the clash.

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

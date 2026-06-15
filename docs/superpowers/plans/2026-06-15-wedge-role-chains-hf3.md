# Plan: Role-chains (RIAs) in the hypertableau wedge ("HF3")

Date: 2026-06-15
Branch: `feat/wedge-role-chains-hf3` (worktree off `main`)
Status: DRAFT → in-progress

## Goal

Make the hypertableau wedge (`HyperEngine`) derive role edges from
role-inclusion axioms (RIAs) and transitivity, so chain-dependent
inconsistencies/subsumptions are no longer dropped. Primary acceptance
target: `family-stripped.ofn` / `family.ofn` reported **inconsistent**
via `owl_dl_reasoner::is_consistent`.

SOUNDNESS-CRITICAL. The reasoner must stay FP=0 / MISSED=0 corpus-wide.
Role-edge derivation is additive (derives genuinely-entailed role facts),
sound only if (a) the encoding is exact, (b) derived edges carry correct
backjump deps, (c) termination is preserved.

## Verified facts (file:line)

1. Clausifier drops RIAs: `crates/owl-dl-core/src/clause.rs:317-321` (`_ => {}`).
2. Engine no-ops role heads: `crates/owl-dl-tableau/src/hyper.rs` `apply_head_atom`
   arm `Atom::Equal(_,_) | Atom::Role(..) => FireOutcome::NoChange` (~line 2210).
3. `match_body` already handles 2-role-atom tree bodies (`eval_order`, `MAX_BODY_VARS=8`).
4. `Event::Edge(src,role,tgt)` fires `role_trigger[role]` clauses **at `src` only**
   (`hyper.rs:1060`). No predecessor back-prop on edges → the **second-leg gap**.
5. Role hierarchy is sized by `vocabulary.num_roles()` (`lib.rs:2830`); `is_sub_role`
   indexes `super_closure[sub.index()]` and **panics out of range**
   (`role_hierarchy.rs:133`). Aux roles MUST live in the vocabulary range.
6. Consistency wedge builds engine `.with_sub_roles(self.sub_roles.clone())`
   (`lib.rs:1266-1268`); `ConsistencyCache::build` clausifies + builds hierarchy
   from the SAME `internal` clone (`lib.rs:1199,1205`).
7. Main tableau `collect_chain_axioms` (`lib.rs:2920`) reads `SubObjectPropertyOf{Chain}`
   but takes only len==2 (drops N>2). It will pick up our decomposed 2-leg chains
   (additive, FP-safe — but watch ro/ore-10908 walls).
8. Closure-diff fixtures with N>2 chains: **ro / ro-stripped** (3-leg) and
   **ore-10908** (chains). Decomposition affects their main-tableau path too.
9. `clause_body_deps` (`hyper.rs:2006`) reconstructs deps from `birth_deps` of
   bound nodes + body class-label deps. Edges carry NO dep-set. A chain-derived
   edge between two pre-existing nodes (both `birth_deps=EMPTY`) has no node to
   carry `D` → SOUNDNESS LANDMINE.

## Design decisions (advisor-reconciled)

- **N>2 chain decomposition lives in `convert_ontology`** (NOT clause.rs), because
  the vocabulary is the single source of truth for `num_roles` and both the
  clausifier and `build_role_hierarchy` read the same `internal`. Aux roles are
  allocated via `vocabulary.intern_role(<unique IRI>)`. The brief's "decompose in
  clause.rs" is overridden on this point (the brief was written without knowing
  the role-id coupling). The 2-leg + transitivity **encoding** stays in clause.rs
  as the brief says.
- **Aux IRI = `urn:rustdl-aux-role:<axiom-idx>:<leg-idx>`** — unique per
  decomposition site. NO common-prefix CSE (sound only if it denotes the identical
  leg-prefix; getting it wrong is a silent FP). `R₁∘R₂∘R₃⊑S` →
  `R₁∘R₂⊑aux₀`, `aux₀∘R₃⊑S` with `aux₀` fresh, produced only by the prefix and
  consumed only by the suffix → exact, any associativity.
- **Edge-dep soundness centerpiece:** when `apply_head_atom` adds a derived
  edge `R₃(u,v)`, fold the clause body deps into the target node's `birth_deps`:
  `nodes[v].birth_deps = nodes[v].birth_deps.union(deps)`. The edge is only ever
  traversed with `v` as a bound node, so `clause_body_deps` always includes
  `v.birth_deps ⊇ D`. Widening `birth_deps` only reduces backjumping ⇒ sound
  (same argument `merge_with_cause` uses at `hyper.rs:1906`). Dedicated backjump
  test required.
- **Second-leg back-prop:** build a separate index `role_back_trigger[r]` =
  clauses with a body role atom `(r,u,v)` where `u != X`. On `Event::Edge(src,role,_)`,
  additionally fire `role_back_trigger[role]` at `src`'s predecessors. `match_body`
  re-verifies, so over-firing is perf-only, never soundness. Scoped index (not
  "fire all role clauses at all preds") to avoid the Phase 3e edge-heavy regression.
- **Inverse head storage:** if the super-role canonicalizes to inverse `R⁻`,
  store the edge as forward `R(v,u)` (flip endpoints) to preserve the engine's
  "edges are Named, inverse via preds" invariant. Test (d) covers head-inverse.
- **Termination:** add each derived `(R,u,v)` edge at most once (dedup check
  before push). Finite nodes ⇒ finite edges. Blocking unaffected.

## Encoding (clause.rs)

- `SubObjectPropertyOf{ sub: Chain([R₁,R₂]), sup: R₃ }` →
  `DlClause { body: [Role(R₁,X,y), Role(R₂,y,z)], head: [Role(R₃,X,z)] }`
  with `y = fresh_var()`, `z = fresh_var()`; roles `canon_role`'d.
- `TransitiveRole(R)` → `DlClause { body: [Role(R,X,y), Role(R,y,z)], head: [Role(R,X,z)] }`.
- `SubObjectPropertyOf{ sub: Role(R), sup: S }` (role hierarchy) — already handled
  via `build_role_hierarchy` / `role_matches`. Do NOT duplicate.
- N>2 chains never reach clause.rs (decomposed away in convert).

## Tasks (TDD, bite-sized)

### T1 — clausifier: 2-leg chain + transitivity encoding (white-box, clause.rs)
### T2 — engine: derive role edges in `apply_head_atom` (white-box, hyper.rs)
### T3 — second-leg back-prop index (white-box, hyper.rs)
### T4 — DEDICATED backjump soundness test (white-box, hyper.rs)
### T5 — N>2 chain decomposition pass (convert.rs)
### T6 — integration: synthetic family pattern (reasoner test)
### T7 — real targets + release build (family-stripped / family → inconsistent)
### T8 — SOUNDNESS GATE: konclude_closure_diff, FP=0/MISSED=0 every fixture
### T9 — fmt + clippy + Opus self-review (FP direction)

(Full task detail in the working notes; see plan body above the task list.)

## Commit cadence
One commit per task (plan first). Messages end with
`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
Do NOT merge to main; do NOT push.

## RESULTS (2026-06-15)

### Implemented (all sound, all tested)
- T1 clausifier: 2-leg chain + transitivity → role-head DL-clauses.
- T2 engine: `derive_role_edge` in `apply_head_atom` (forward storage,
  inverse-head flipped, both-endpoints `birth_deps` fold, dedup, re-queue).
- T3 `role_back_trigger` index (second-leg back-prop, scoped).
- T4/T4b backjump soundness tests (incl. the FP-direction guard: chain
  clash under one disjunct stays Sat).
- T5 convert `decompose_long_chains` (N>2 → 2-leg cascade, unique aux roles
  in the vocabulary).
- T6 wedge_consistency canaries (chain over generated ∃-successor → Unsat,
  3-leg, transitivity, + catastrophic-guard consistent variant).

### SOUNDNESS GATE — PASS (FP=0 AND MISSED=0 on EVERY fixture)
galen 27997, notgalen 32739, sio 8904, ore-10908 6001, ore-15672 142,
wine 653, pizza 499, alehif 247, shoiq-knowledge 449, ro 158 (3-leg chains,
decomposed), sulo 51, bibtex 16, + ro-stripped/sulo-stripped/sio-stripped.
fmt clean, clippy `-D warnings` clean, full workspace tests green.

### family / family-stripped: STILL `consistent` (sound MISS — NOT closed)
The chain machinery is correct and works over ABox individuals + generated
successors (proved by `mech2`: chain → Bad → disjoint clash → wedge Unsat).
But the family inconsistency additionally needs a **functional-role merge
over a generated ∃-successor on an ABox-seeded node** (`∃hasSex.Female ⊓
∃hasSex.Male` under `Functional(hasSex)` → `Female⊓Male` clash). That merge
is NOT detected by the wedge — and a minimal test WITHOUT any chain
(`mech3`: `Start⊑∃hasSex.Female`, `Start⊑∃hasSex.Male`, functional, disjoint,
`a:Start`) ALSO returns Sat. So this is a **pre-existing, separate
functional-merge gap on ABox-seeded nodes**, independent of HF3 (mech3 has
zero chains). Closing it is out of scope for this feature and was NOT
attempted (would risk the gate / touch the merge+blocking interaction).
The two `family*_inconsistency_detected` tests remain `#[ignore]`d and
were NOT un-ignored or weakened. family stays a sound MISS (reported
`consistent`; the safe direction). Do NOT raise the global `FIXPOINT_ITERS`
(would risk MISSED-via-timeout on chain-heavy classify pairs).

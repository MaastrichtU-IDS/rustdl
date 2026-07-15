# Wedge semantic branching / in-search BCP (Fix #2) — design

**Status:** advisor-scoped (2026-07-15) — for user review
**Parent:** dense-SROIQ deep-R&D. Diagnosis + Fix-fork:
`docs/superpowers/specs/2026-07-15-wedge-backjump-precision-design.md`; Phase-1
result `docs/2026-07-15-backjump-precision-findings.md` (Fix #1 ruled out).
**Goal:** decide `ore_ont_10019`'s 33 disjunctive-DFS-stalled classes within budget
by giving the wedge **in-search boolean constraint propagation (semantic
branching)** over the surviving disjunctive clauses and the ontology's
disjointness axioms — the capability the diagnosis showed is the remaining gap
(Konclude 90 ms / HermiT 360 ms; rustdl stalls).

## Diagnosis recap (established — do not re-litigate)

The stall is **H2**: disjunctive-DFS thrash over a blocking-bounded, redundantly
re-explored state space. All quick levers are exhausted: SP1 throughput, SP2
node-local no-goods (DEAD — do NOT revive), MRV ordering (on, inert),
`sat_lookahead` (inert), backjump-precision / Fix #1 (ruled out — `bjgap_real ≡
bjgap_shadow` incl. max). The **gap is in-search BCP**, established concretely:
- The 25 disjunctions are **genuine covering axioms** `C ⊑ D₁ ⊔ … ⊔ Dₙ`
  (`clause.rs` `emit_head` Or arm) — irreducible; no absorption removes them.
- The 55 disjointness axioms are **Horn `⊥`-headed clauses** `A ⊓ B → ⊥` — the
  *fuel* for propagation, not branch points. `=n` cardinality goes through the
  `≤n` merge path, not `⊔` (top stalled classes are `merge=0`).
- Today disjointness is enforced **only reactively** (a dead disjunct is found a
  full `horn_fixpoint` *after* the branch commits) and **only positively** (nodes
  carry positive `labels: Vec<ClassId>`; there is no "¬D asserted here" state).
  `find_open_disjunction` prunes a disjunct only when already TRUE, never when
  forced FALSE. Decisively, **`build_disjoint_pairs` already exists and is built
  per engine, but is consulted only in the `≤n` merge path — never in the `⊔`
  path.** The raw material for BCP is present and unused where it is needed.

## Design — semantic branching driven by `disjoint_pairs`

Confined to `crates/owl-dl-tableau/src/hyper.rs`. Env flag
`RUSTDL_SEMANTIC_BRANCHING`, **default OFF**. Two layers; **Layer A ships and is
validated before Layer B is built.**

### Layer A — dead-disjunct pruning + unit forcing (verdict-preserving)

In `solve`, at an open disjunction, before branching, compute the live disjuncts
(reuse the existing `live: Vec<usize>` slot / `sat_lookahead` plumbing): drop any
disjunct `Class(c, X)` for which the node's current label already carries some `e`
with `(min(c,e), max(c,e)) ∈ self.disjoint_pairs` — that disjunct would clash on
the very next fixpoint anyway.
- `live` empty → `Unsat` with the body dep-set (the existing `2275-2278` path).
- exactly one `live` → assert it via `apply_head_atom` **without incrementing the
  decision level** (unit propagation — not a branch point).
- else → branch over `live` only (fewer children).

**Soundness: verdict-preserving, cannot even MISS** — it only removes branches
that provably clash immediately (a disjoint co-occurrence the reactive
`horn_fixpoint` would have clashed on next pass) and only *forces* a disjunct the
search would have had to take anyway. No exclusion / negative state. ~80–120 LOC.

### Layer B — semantic branching via a per-node exclusion set

Add a per-node `excluded: Vec<ClassId>` that rides the existing whole-node-clone
`Snapshot { nodes: self.nodes.clone() }` (**no `trail.rs` change** — save/restore
already clones nodes). In `solve`'s `for k in live` branch loop: when a prior
sibling disjunct `Dⱼ` (j < k) returned a **clean `Unsat`**, add `Dⱼ`'s class to
the node's `excluded` set before trying branch k. Then:
- `add_label` (the single label chokepoint) treats adding an `excluded` class as a
  clash;
- `find_open_disjunction` liveness treats an `excluded` disjunct as dead.

This converts the syntactic branch `D₁ | D₂ | D₃` into the sound partition
`D₁ | ¬D₁∧D₂ | ¬D₁∧¬D₂∧D₃`. Because the covering disjuncts are pairwise disjoint
via the 55 axioms, each asserted `¬Dⱼ` **propagates** — downstream disjunctions
that mention `Dⱼ` collapse to unit (via Layer A's forcing). This is Konclude's
disjunction-collapsing behaviour, scoped to **atomic** disjuncts; compound
(`∃`/structural-`Q`) disjuncts stay live (conservative, still sound). ~200–300 LOC.

## Soundness invariant (the load-bearing rule)

**Layer B's exclusion is sound ONLY for a sibling that returned `Unsat`, NEVER
`Stalled`.** Under a deadline / `is_diverging`, branches routinely return
`Stalled`; excluding a merely-*stalled* disjunct's class asserts an unproven
`¬Dⱼ`, which can manufacture a false clash → **unsound → FP subsumption.** This is
the same hazard family as `reuse-trap-A1` / the snapshot-cache soundness fix
(trusting an incomplete result).
- **Invariant:** only add an exclusion for a sibling whose recursive `solve`
  returned `Unsat`. If any sibling returns `Stalled`, the frame's result is
  `Stalled` and **no exclusion is added** from it.
- Layer A adds no exclusion and no negative state, so it carries none of this
  hazard.
- Only **atomic** `Class` disjuncts are excluded (a compound disjunct has no
  single class to exclude) — conservative.

## Gate (per `RUSTDL_INVERSE_FUNC_MERGE` / `precise_card_deps` precedent)

- Env flag `RUSTDL_SEMANTIC_BRANCHING`, **default OFF**. Flip default-ON only in a
  separate reviewed commit after the gate is green.
- **FP=0** on the curated corpus **AND the non-Horn adversarial oracle**
  (`ore_ont_13723` vs Konclude∩HermiT) — the primary hazard for Layer B.
- **MISSED=0 / byte-identical curated closures** (completeness guard; Layer A must
  be byte-identical since it's verdict-preserving).
- **Dedicated canary: a `Stalled` sibling is never excluded** (the FP tripwire) —
  a synthetic fixture where a disjunct only *stalls* (deadline) and asserting its
  negation would clash; assert the flag-ON verdict does NOT flip.
- Differential closure-diff harness (the existing one) OFF vs ON on every fixture.

## Success criterion / go-no-go

- **Primary:** with the flag ON, `ore_ont_10019 classify` decides **≥ ~half of the
  33 stalled classes** within the Konclude/HermiT budget envelope (a few seconds),
  FP=0/MISSED=0. If met → corpus FP=0/MISSED=0 gate → default-ON (separate commit).
- **If not met** (Layer A+B don't clear ~half within budget): **STOP and take
  bound-the-tail** — make the `Stalled → NoVerdict → search.rs` fallthrough return
  sound-incomplete fast, and document "dense-SROIQ disjunctive tail needs
  Konclude-class caching/learning, deferred." A legitimate, evidence-backed
  outcome, not a failure.

## Staging

1. **Layer A** — build, gate (byte-identical curated, verdict-preserving), measure
   `ore_ont_10019` (does unit-forcing + fewer branches alone move any of the 33?).
2. **Layer B** — only after A lands; the `Unsat`-only-exclusion invariant + the
   `Stalled` canary are built with it. Measure `ore_ont_10019` (the real test).
3. **Decide** per the go-no-go.

## Risk / honest framing

Buildable as an incremental change (high confidence): the clausal wedge is the
right shape for BCP, the `live`-set hook + `disjoint_pairs` table already exist,
and whole-node-clone save/restore means the exclusion state needs no new trail
machinery. **Whether it *closes* the 33 is ~40% (advisor):** the reactive
`horn_fixpoint` already catches disjoint co-occurrence promptly (bounding Layer
A's marginal win); `bjgap_real ≡ bjgap_shadow` says every stacked decision
matters (few locally-forced disjuncts, capping BCP's reach); `revisit_frac ≈ 1.0`
with node-local no-goods DEAD says the redundancy is whole-graph, whose cure
(whole-model caching / CDCL learning) is out of scope for its reuse-trap FP
surface. The counterweight: covering axioms + pairwise-disjoint disjuncts + 55
disjointness axioms are the *ideal* substrate for semantic branching, so it
deserves a real measurement. Cheap to falsify; the go-no-go makes the bound-tail
fork clean.

## Out of scope

- Whole-model / status caching, CDCL clause-learning (reuse-trap-A1 FP surface).
- Node-local UNSAT no-goods (SP2 — DEAD).
- Compound (`∃`/`Q`) disjunct exclusion (atomic-only, conservative).
- Absorption changes (the disjunctions are irreducible — not the lever).
- `≤n` partition-blowup (`merge=0` on the stalled 33).

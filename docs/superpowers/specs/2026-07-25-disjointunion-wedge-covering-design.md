# Design + plan: DisjointUnion covering direction in the wedge clausifier (#40)

**Date:** 2026-07-25
**Status:** design confirmed (empirical gap reproduced); ready to implement.
**Author:** Claude + Michel.
**Issue:** #40 (partial). Most of #40 was already implemented before the issue was
filed (2026-07-24): `convert.rs:1824` produces `Axiom::DisjointUnion` (not dropped),
and `absorb.rs` fully handles it (covering `C ≡ ⊔members` + pairwise disjoint) for
the tableau. The **only** residual is in the **wedge clausifier** (`clause.rs`).

## The confirmed gap

The `clause.rs` `Axiom::DisjointUnion` arm emits the pairwise disjointness **and**
`member ⊑ class` (Horn), but **explicitly defers** the covering half
`class ⊑ ⊔members`. Empirically reproduced (default config):

Fixture: `DisjointUnion(C, D1, D2)` + `SubClassOf(D1, E)` + `SubClassOf(D2, E)`.
Expected entailment: `C ⊑ E` (since `C ⊑ D1⊔D2 ⊑ E`).

| Path | `C ⊑ E` |
|------|---------|
| `classify` (default, trust_sat ON) | **MISSED** |
| `classify` (trust_sat OFF) | **MISSED** (Phase-7 label heuristic prunes the candidate — the wedge's covering-deferred model of `C` doesn't include `E`) |
| `is_subclass_of` / `explain` (direct tableau) | ✅ derived |

So `classify` — which the Protégé plugin's class hierarchy uses — misses covering-
dependent subsumptions, while the direct subclass query already gets them. `Dᵢ ⊑ C`
is not the gap (told tables + the arm's own `member ⊑ class` supply it); only
`C ⊑ ⊔Dᵢ` is missing.

## The fix (surgical, no pool mutation)

In the `clause.rs` `Axiom::DisjointUnion` arm, additionally emit the covering
clause `C(X) → D1(X) ∨ … ∨ Dn(X)`:

```rust
// class ⊑ ⊔members : C(X) → D1(X) ∨ … ∨ Dn(X)  (the previously-deferred covering half)
let mut head = Vec::with_capacity(members.len());
let mut all_atomic = true;
for &m in members {
    match self.class_id_of(m) {
        Some(cid) => head.push(Atom::Class(cid, X)),
        None => { all_atomic = false; break; }   // complex member ⇒ defer (sound under-approx)
    }
}
if all_atomic {
    // empty members ⇒ head == [] ⇒ `C ⊑ ⊥` (empty union), matching the ⊥-clause convention.
    self.clauses.push(DlClause { body: vec![Atom::Class(*class, X)], head });
} else {
    self.defer("disjoint-union-covering");
}
```

Set `self.next_var = X + 1;` before, consistent with the arm's other blocks. Replace
the stale "defer the equivalence half" comment. `class` is a `ClassId` (atomic body);
`members` are `ConceptId`s — `class_id_of` yields the atomic/nominal `ClassId` or
`None` for a compound member (deferred, as the arm already defers complex antecedents).

**Soundness:** the covering GCI is the genuine DisjointUnion semantics — an entailed
axiom, so it can only *add* true subsumptions, never an FP. The head disjunction is a
standard non-Horn clause the hypertableau already branches on. Risk is limited to
perturbing wedge clausification for DisjointUnion-bearing ontologies → gated by a
**corpus FP=0 closure-diff**.

## Plan (single task, TDD, subagent-driven with review)

### Task 1: emit the covering clause + canaries
**Files:**
- Modify: `crates/owl-dl-core/src/clause.rs` (the `Axiom::DisjointUnion` arm, ~line 275).
- Test: `crates/owl-dl-core/src/clause.rs` `#[cfg(test)]` and/or a reasoner-level
  integration test in `crates/owl-dl-reasoner/tests/` (classify-level, the level the
  gap manifests at).

- [ ] **Step 1 — RED (classify-level canary):** add a test that classifies
  `DisjointUnion(C, D1, D2)` + `D1 ⊑ E` + `D2 ⊑ E` and asserts `C ⊑ E` is entailed
  (`is_subclass(C, E)` on the `Classification`, or the direct-subsumption closure).
  Run; confirm it FAILS today (`C ⊑ E` missing).
- [ ] **Step 2 — GREEN:** apply the covering-clause emission above. Run; canary passes.
- [ ] **Step 3 — regression canaries:** (a) a covering-dependent **unsat** case —
  `DisjointUnion(C, D1, D2)` + `D1 ⊑ ⊥` + `D2 ⊑ ⊥` ⇒ `C ⊑ ⊥` (C unsatisfiable);
  (b) retain a **pairwise-disjoint** case — `DisjointUnion(C, D1, D2)` + an individual
  or class forced into `D1 ⊓ D2` ⇒ unsat, confirming the disjoint half still fires;
  (c) a **complex-member defer** case (a DisjointUnion with a non-atomic member) still
  behaves soundly (no panic; covering deferred). Run all; pass.
- [ ] **Step 4 — FP=0 corpus gate (the soundness gate):** run the closure-diff /
  classify on the curated fixtures that contain DisjointUnion (e.g. `pizza`, and any
  in `ontologies/real` — fetch via `scripts/fetch-real-ontologies.sh` if needed) and
  confirm **no new FP** vs the pre-change baseline (a new subsumption is acceptable
  ONLY if it is genuinely entailed — verify any delta is a true covering-derived
  subsumption, not an FP). If a Konclude/HermiT oracle run is easy on the affected
  fixtures, use it; otherwise closure-diff + manual check of any delta.
- [ ] **Step 5 — full checks:** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core -p owl-dl-reasoner`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all`.
- [ ] **Step 6 — commit:** `fix(clause): emit DisjointUnion covering clause in the wedge clausifier (#40)`.

## Global constraints
- Build/test `RUSTUP_TOOLCHAIN=stable cargo …`. Clippy pedantic `-D warnings`;
  `unwrap`/`dbg` only under `#[cfg(test)]`; rustfmt max_width=100.
- **FP=0 is the cardinal gate** (this touches wedge clausification).
- Isolated worktree `rustdl-wt/expressivity-fills`, branch `feat/expressivity-fills-40-42`
  (a concurrent agent works elsewhere in the repo — do not leave this worktree).

## Out of scope / #42
Non-string `DataOneOf` is already implemented; nested composite data ranges and
general range-size cardinality counting are documented ~0-corpus-value sound
under-approximations. #42 handled separately (document/close), not here.

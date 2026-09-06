# Conjunctive `ObjectPropertyDomain` / `Range` filler (#110) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the EL saturator process a conjunctive `ObjectPropertyDomain`/`Range` filler, and move both fragment gates in the same commit so the gate and the engine cannot disagree.

**Architecture:** `Domain(r, P ⊓ Q) ≡ Domain(r,P) ∧ Domain(r,Q)` is a **logical identity**, so decomposing the conjunction into atomic conjuncts is sound and completeness-preserving by construction. `role_domains`/`role_ranges` are already `HashMap<RoleId, Vec<ClassId>>`, so a conjunction is already representable — this pushes n entries where it used to push none. **One shared function** performs the decomposition and reports whether it was COMPLETE; the engine consumes the entries, both gates consume the boolean. They therefore cannot drift, which is the mechanism that generates D10 bugs.

**Tech Stack:** Rust (edition 2024), `owl-dl-saturation`, `owl-dl-reasoner`. Build/test with `RUSTUP_TOOLCHAIN=stable cargo …` or `PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` — a bare `cargo` on this host is 1.75 and rejects edition2024.

**Spec:** `docs/superpowers/plans/2026-09-06-d10-arc-roadmap.md` § WS1, and issue #110.

## Global Constraints

- **`RUSTUP_TOOLCHAIN=stable`** for every build/test; a failed build silently reuses a stale `target/release/` binary.
- **Unflagged.** A soundness/completeness fix is not opt-in. No new `RUSTDL_*` flag.
- **Direction of risk is INVERTED:** this ADDS domain/range inferences, so the failure mode is a false POSITIVE, not a miss. Every canary needs an oracle, and the negative controls carry the weight.
- **Partial decomposition is SOUND; partial ADMISSION is not.** The engine may push the atomic conjuncts of `P ⊓ ∃s.C` (a weaker domain is the safe direction). The gates must refuse that axiom, because a conjunct the engine never processed inside a complete-certified fragment is a fresh D10.
- **`ore_ont_9347` = 113 and `ore_ont_5368` = 18,620,251** are the standing DKey discriminators; if either moves, something unintended changed.
- Oracles: Konclude `/data/dumontier/reasoners/run-konclude.sh`, HermiT `/data/dumontier/reasoners/run-hermit.sh`, KM `/data/dumontier/kobayashi-marust/engine/target/release/km classify`. Judge Konclude from CONTENT (it exits 0 on junk, writing an 896-byte stub).
- Corpus: `/data/dumontier/ore-run/pool_sample/files` (1,920).

---

### Task 1: Shared decomposer + engine wiring

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (new `pub fn decompose_role_filler`; the `ObjectPropertyDomain` arm at ~3457 and `ObjectPropertyRange` arm at ~3473)
- Test: `crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs` (new)

**Interfaces:**
- Produces: `pub fn owl_dl_saturation::decompose_role_filler(c: ConceptId, pool: &ConceptPool, sink: &mut Vec<ClassId>) -> bool` — pushes every atomic conjunct onto `sink`; returns `true` iff the filler decomposed COMPLETELY. Task 2's gates call it with a throwaway sink and read only the `bool`.

- [ ] **Step 1: Write the failing canaries**

Create `crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs`:

```rust
//! Issue #110 — a CONJUNCTIVE `ObjectPropertyDomain`/`Range` filler is dropped.
//!
//! `collect_el_rules`' Domain/Range arms handled `Bot` (poison) and `Atomic`
//! (push) and fell through silently on `And`, so `Domain(r, P ⊓ Q)` reached the
//! engine as nothing at all. `is_el_axiom` correctly refused the axiom, routing
//! the ontology to the hybrid path — where the tier walk then never compared
//! `X` against `P`, because dropping the filler left `X` with no EL subsumer and
//! therefore in `P`'s own tier. `classify` returned ZERO rows with
//! `incomplete: false`; Konclude, HermiT and KM all derive the pairs.
//!
//! `Domain(r, P ⊓ Q) ≡ Domain(r,P) ∧ Domain(r,Q)` is a logical identity, so the
//! fix decomposes rather than approximates.

use owl_dl_reasoner::classify;

fn entails(body: &str, sub: &str, sup: &str) -> bool {
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    let c = classify(&onto).expect("classify");
    c.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

const DECLS: &str = "Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))
     Declaration(Class(:B)) Declaration(Class(:W)) Declaration(Class(:S))
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))";

/// #110, domain half. Both conjuncts must be derived, not just the first.
#[test]
fn conjunctive_domain_filler_derives_every_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
    );
    assert!(entails(&body, "X", "P"), "X ⊑ P (first conjunct)");
    assert!(entails(&body, "X", "Q"), "X ⊑ Q (second conjunct)");
}

/// #110, range half — the LARGER population (13 of the 14 ORE candidates carry a
/// conjunctive Range against 4 for Domain).
///
/// NB the existential filler is `:B`, NOT `owl:Thing`. `∃r.⊤` lowers to a
/// deliberately subsumer-less ⊤-witness that `Range` is not folded into — the
/// documented `topwitness.ofn` DESIGN DECISION — so a `⊤` fixture fails on the
/// atomic control too and cannot discriminate.
#[test]
fn conjunctive_range_filler_derives_every_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :W)"
    );
    assert!(entails(&body, "X", "W"), "X ⊑ W via the folded range");
}

/// CONTROL that must keep passing: the atomic filler was never broken.
#[test]
fn atomic_domain_filler_still_derives_its_subsumption() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r :P)"
    );
    assert!(entails(&body, "X", "P"));
}

/// FP GUARD. Decomposition must never invent a domain the axiom does not state:
/// `Domain(r, P)` alone must NOT yield `X ⊑ Q`.
#[test]
fn decomposition_does_not_invent_an_unstated_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r :P)"
    );
    assert!(!entails(&body, "X", "Q"), "Q is not a domain of r");
}

/// PARTIAL decomposition is SOUND: `P ⊓ ∃s.S` yields the atomic `P`, which is a
/// WEAKER (larger) domain than the axiom states — the safe direction. Task 2
/// pins that the GATE nonetheless refuses this axiom.
#[test]
fn partially_decomposable_filler_still_derives_its_atomic_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))"
    );
    assert!(entails(&body, "X", "P"), "the atomic conjunct is entailed");
}

/// A DISJUNCTIVE filler must NOT decompose. `Domain(r, P ⊔ Q)` does not entail
/// `Domain(r,P)`; deriving `X ⊑ P` from it would be a false POSITIVE, which is
/// this change's failure direction.
#[test]
fn a_disjunctive_filler_does_not_decompose() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectUnionOf(:P :Q))"
    );
    assert!(!entails(&body, "X", "P"), "FP: a disjunct is not a domain");
    assert!(!entails(&body, "X", "Q"), "FP: a disjunct is not a domain");
}
```

- [ ] **Step 2: Run them and confirm the two bug tests FAIL and the four others PASS**

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
cargo test -p owl-dl-reasoner --test conjunctive_domain_range_filler
```

Expected: `conjunctive_domain_filler_derives_every_conjunct` and
`conjunctive_range_filler_derives_every_conjunct` FAIL;
`atomic_domain_filler_still_derives_its_subsumption`,
`decomposition_does_not_invent_an_unstated_conjunct`,
`a_disjunctive_filler_does_not_decompose` PASS;
`partially_decomposable_filler_still_derives_its_atomic_conjunct` FAILS (nothing is pushed today).

**If a bug test PASSES here, stop** — the defect does not reproduce on this tree and the plan's premise (S1) is stale.

- [ ] **Step 3: Add the shared decomposer to `owl-dl-saturation/src/lib.rs`**

Place it immediately above `collect_el_rules`:

```rust
/// Decompose an `ObjectPropertyDomain` / `ObjectPropertyRange` filler into the
/// ATOMIC classes the saturator's `role_domains` / `role_ranges` can hold,
/// pushing each onto `sink`. Returns `true` iff the filler decomposed
/// **completely**.
///
/// `Domain(r, P ⊓ Q)` is logically identical to `Domain(r,P) ∧ Domain(r,Q)`
/// (likewise `Range`), so this is sound and completeness-preserving by
/// construction — the same identity argument as `flatten_union_of_oneofs`
/// (#42 item 1) and #81's range fold, not an approximation.
///
/// **Partial decomposition is deliberate and sound.** `P ⊓ ∃s.C` yields `P`
/// alone: a WEAKER (larger) domain than the axiom states, which derives fewer
/// subsumptions and never a wrong one. Contrast `DataUnionOf`, where keeping
/// half a union NARROWS a range and manufactures clashes — there all-or-nothing
/// is load-bearing, here it is not.
///
/// **The `bool` is what the fragment gates consume**, and it is why this
/// function is `pub`. A gate that re-implemented "is this filler decomposable?"
/// could drift from what this actually processes, and a conjunct the engine
/// silently skipped inside a complete-certified fragment is exactly the D10 bug
/// class. One function, no drift — the same reasoning that factored out
/// `abox_saturation_inconsistent`.
///
/// `Bot` returns `false` **without** pushing: `Domain(r, ⊥)` is handled earlier
/// by `poisoned_roles`, and a `Bot` nested inside a conjunction (`P ⊓ ⊥ ≡ ⊥`)
/// means the whole role is poisoned — which this function does not model, so it
/// declines and the gates route the ontology to the hybrid path.
pub fn decompose_role_filler(
    c: ConceptId,
    pool: &ConceptPool,
    sink: &mut Vec<ClassId>,
) -> bool {
    match pool.get(c) {
        ConceptExpr::Atomic(id) => {
            sink.push(*id);
            true
        }
        // `Domain(r, ⊤)` states nothing; vacuously complete, nothing to push.
        ConceptExpr::Top => true,
        ConceptExpr::And(ops) => {
            let mut complete = true;
            for op in ops {
                // Deliberately NOT short-circuiting: every atomic conjunct is
                // entailed and worth pushing even when a sibling is not
                // representable.
                complete &= decompose_role_filler(*op, pool, sink);
            }
            complete
        }
        _ => false,
    }
}
```

- [ ] **Step 4: Wire both engine arms to it**

In `collect_el_rules`, replace the `ObjectPropertyDomain` arm's final branch:

```rust
                } else if let ConceptExpr::Atomic(id) = internal.concepts.get(*domain) {
                    rules
                        .role_domains
                        .entry(role.role_id())
                        .or_default()
                        .push(*id);
                }
```

with:

```rust
                } else {
                    let entry = rules.role_domains.entry(role.role_id()).or_default();
                    decompose_role_filler(*domain, &internal.concepts, entry);
                }
```

and the identical change in the `ObjectPropertyRange` arm, targeting `rules.role_ranges` and `*range`.

- [ ] **Step 5: Run the canaries — all six must pass**

```sh
cargo test -p owl-dl-reasoner --test conjunctive_domain_range_filler
```

Expected: `test result: ok. 6 passed; 0 failed`.

- [ ] **Step 6: Add decomposer unit tests in `owl-dl-saturation`**

In that file's `mod tests`, using this crate's real `ConceptPool` API
(`pool.atomic(ClassId::new(n))`, `pool.and([..])`, `pool.or([..])`, `pool.top()`, `pool.bot()`):

```rust
    #[test]
    fn decompose_pushes_every_atomic_conjunct_and_reports_complete() {
        let mut pool = ConceptPool::default();
        let p = pool.atomic(owl_dl_core::ClassId::new(0));
        let q = pool.atomic(owl_dl_core::ClassId::new(1));
        let and = pool.and([p, q]);
        let mut sink = Vec::new();
        assert!(decompose_role_filler(and, &pool, &mut sink), "fully decomposable");
        assert_eq!(sink.len(), 2, "both conjuncts pushed");
    }

    /// PARTIAL: `P ⊓ (Q ⊔ R)` pushes `P` and reports INCOMPLETE. A disjunction is
    /// used as the non-decomposable conjunct because it needs no `Role`, keeping
    /// the unit test free of role-hierarchy setup.
    #[test]
    fn decompose_reports_incomplete_but_still_pushes_the_atomic_half() {
        let mut pool = ConceptPool::default();
        let p = pool.atomic(owl_dl_core::ClassId::new(0));
        let q = pool.atomic(owl_dl_core::ClassId::new(1));
        let r = pool.atomic(owl_dl_core::ClassId::new(2));
        let or = pool.or([q, r]);
        let and = pool.and([p, or]);
        let mut sink = Vec::new();
        assert!(!decompose_role_filler(and, &pool, &mut sink), "not complete");
        assert_eq!(sink.len(), 1, "the atomic conjunct is still pushed");
    }

    /// ORDER-INDEPENDENCE, and it is what sabotage #5 (short-circuiting the `And`
    /// loop) is caught by: with the non-decomposable conjunct FIRST, a
    /// short-circuiting loop would push nothing.
    #[test]
    fn decompose_does_not_short_circuit_when_the_bad_conjunct_comes_first() {
        let mut pool = ConceptPool::default();
        let p = pool.atomic(owl_dl_core::ClassId::new(0));
        let q = pool.atomic(owl_dl_core::ClassId::new(1));
        let r = pool.atomic(owl_dl_core::ClassId::new(2));
        let or = pool.or([q, r]);
        let and = pool.and([or, p]);
        let mut sink = Vec::new();
        assert!(!decompose_role_filler(and, &pool, &mut sink));
        assert_eq!(sink.len(), 1, "the atomic conjunct is pushed regardless of order");
    }

    /// FP GUARD: a bare disjunction decomposes to NOTHING. `Domain(r, P ⊔ Q)`
    /// does not entail `Domain(r, P)`.
    #[test]
    fn decompose_declines_a_disjunction_and_pushes_nothing() {
        let mut pool = ConceptPool::default();
        let p = pool.atomic(owl_dl_core::ClassId::new(0));
        let q = pool.atomic(owl_dl_core::ClassId::new(1));
        let or = pool.or([p, q]);
        let mut sink = Vec::new();
        assert!(!decompose_role_filler(or, &pool, &mut sink));
        assert!(sink.is_empty(), "a disjunct is not a domain — FP guard");
    }

    /// `Top` states nothing and is vacuously complete: `Domain(r, ⊤)` must be
    /// admitted by the gate (it adds no constraint) while pushing no class.
    #[test]
    fn decompose_treats_top_as_complete_and_pushes_nothing() {
        let mut pool = ConceptPool::default();
        let top = pool.top();
        let mut sink = Vec::new();
        assert!(decompose_role_filler(top, &pool, &mut sink));
        assert!(sink.is_empty());
    }

    /// `Bot` declines WITHOUT pushing — it is handled earlier by
    /// `poisoned_roles`, and `is_processed_role_filler` re-admits it explicitly.
    #[test]
    fn decompose_declines_bot_without_pushing() {
        let mut pool = ConceptPool::default();
        let bot = pool.bot();
        let mut sink = Vec::new();
        assert!(!decompose_role_filler(bot, &pool, &mut sink));
        assert!(sink.is_empty());
    }
```

- [ ] **Step 7: Run the saturation unit tests**

```sh
cargo test -p owl-dl-saturation decompose_
```

Expected: 6 passed.

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-saturation/src/lib.rs crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs
git commit -m "fix(saturation): decompose a conjunctive Domain/Range filler (#110)"
```

---

### Task 2: Move both fragment gates in lockstep

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (`is_atomic_or_trivial_concept:2132`; `is_el_axiom`'s Domain/Range arms at 2183/2186; `is_saturator_axiom`'s at 2574/2577)
- Test: `crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs` (extend)

**Interfaces:**
- Consumes: `owl_dl_saturation::decompose_role_filler` from Task 1.

- [ ] **Step 1: Write the failing gate tests**

Append to `conjunctive_domain_range_filler.rs`:

```rust
use owl_dl_core::convert::convert_ontology;
use owl_dl_reasoner::{FragmentClassification as FC, analyze_fragment};

fn fragment_of(body: &str) -> FC {
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    analyze_fragment(&convert_ontology(&onto).expect("convert"))
}

/// The gate must move WITH the engine: a fully-decomposable filler is now
/// processed, so the ontology belongs on the pure-EL fast path.
#[test]
fn a_fully_decomposable_filler_is_admitted_to_pure_el() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
    );
    assert_eq!(fragment_of(&body), FC::PureEl);
}

/// THE LOAD-BEARING NEGATIVE. A partially-decomposable filler leaves `∃s.S`
/// unprocessed by the engine, so admitting it to a complete-certified fragment
/// would be a FRESH D10 — the exact bug class this fix exists to close.
#[test]
fn a_partially_decomposable_filler_is_refused_by_the_gate() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))"
    );
    assert_ne!(fragment_of(&body), FC::PureEl);
}

/// A disjunctive filler is not decomposable and must stay out of the fragment.
#[test]
fn a_disjunctive_filler_is_refused_by_the_gate() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectUnionOf(:P :Q))"
    );
    assert_ne!(fragment_of(&body), FC::PureEl);
}
```

- [ ] **Step 2: Run and confirm exactly the first one FAILS**

```sh
cargo test -p owl-dl-reasoner --test conjunctive_domain_range_filler
```

Expected: `a_fully_decomposable_filler_is_admitted_to_pure_el` FAILS (gate still refuses); the two negatives PASS.

- [ ] **Step 3: Point both gates at the shared decomposer**

Replace `is_atomic_or_trivial_concept` (used ONLY by the four Domain/Range arms — verify with `grep -n is_atomic_or_trivial_concept crates/owl-dl-reasoner/src/classify.rs` before editing; if it has other callers, add a new function instead of changing this one):

```rust
/// True iff a Domain/Range filler is one the saturator processes IN FULL.
///
/// Delegates to `owl_dl_saturation::decompose_role_filler` and reads only its
/// completeness flag, so the gate cannot drift from what the engine actually
/// does. Re-implementing the predicate here is how D10 bugs are born — see that
/// function's doc.
fn is_processed_role_filler(c: ConceptId, pool: &ConceptPool) -> bool {
    let mut sink = Vec::new();
    owl_dl_saturation::decompose_role_filler(c, pool, &mut sink)
        || matches!(pool.get(c), ConceptExpr::Bot)
}
```

`Bot` is admitted explicitly because the engine handles it via `poisoned_roles` *before* the
decomposer is reached, so the decomposer legitimately returns `false` for it.

Then rename the four call sites at 2183/2186/2574/2577 from `is_atomic_or_trivial_concept` to
`is_processed_role_filler`.

- [ ] **Step 4: Run the full canary file — all nine must pass**

```sh
cargo test -p owl-dl-reasoner --test conjunctive_domain_range_filler
```

Expected: `test result: ok. 9 passed; 0 failed`.

- [ ] **Step 5: Run the flag-default and fragment guards**

```sh
cargo test -p owl-dl-reasoner --test flag_defaults
cargo test -p owl-dl-reasoner saturator_fragment_
```

Expected: all pass — this change must not move any flag default or loosen the D10 allowlist for
`∀` / `≤n` / `⊔` / `DisjointClasses`.

- [ ] **Step 6: Commit**

```sh
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs
git commit -m "fix(classify): admit a fully-decomposable Domain/Range filler to the EL fragment (#110)"
```

---

### Task 3: Oracle adjudication and the sabotage battery

**Files:**
- Create: `docs/benchmarks/2026-09-06-conjunctive-domain-range-adjudication.md`

- [ ] **Step 1: Build the six probe files**

```sh
S=$(mktemp -d)
mk() { printf 'Prefix(:=<http://rustdl.test/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(\n%s\n)\n' "$2" > "$S/$1.ofn"; }
D='Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X)) Declaration(Class(:B)) Declaration(Class(:W)) Declaration(Class(:S)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))'
mk dom_conj    "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
mk dom_atomic  "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyDomain(:r :P)"
mk rng_conj    "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyRange(:r ObjectIntersectionOf(:P :Q)) SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :W)"
mk rng_atomic  "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyRange(:r :P) SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :W)"
mk dom_partial "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))"
mk dom_disj    "$D SubClassOf(:X ObjectSomeValuesFrom(:r :B)) ObjectPropertyDomain(:r ObjectUnionOf(:P :Q))"
echo "$S"
```

- [ ] **Step 2: Run all three oracles on all six**

```sh
KM=/data/dumontier/kobayashi-marust/engine/target/release/km
for f in dom_conj dom_atomic rng_conj rng_atomic dom_partial dom_disj; do
  echo "=== $f ==="
  /data/dumontier/reasoners/run-konclude.sh $S/$f.ofn $S/$f.kon.owx >/dev/null 2>&1
  echo "  konclude bytes=$(stat -c%s $S/$f.kon.owx)"   # 896 == the failure stub, NOT an answer
  grep -o '<SubClassOf>' $S/$f.kon.owx | wc -l
  timeout 300 /data/dumontier/reasoners/run-hermit.sh $S/$f.ofn $S/$f.hermit.out >/dev/null 2>&1
  grep -oE 'SubClassOf\([^)]*\)' $S/$f.hermit.out | grep -v 'owl:Thing\|Nothing'
  timeout 120 $KM classify $S/$f.ofn
done
```

- [ ] **Step 3: Record the adjudication table**

Write `docs/benchmarks/2026-09-06-conjunctive-domain-range-adjudication.md` with one row per
probe × {rustdl before, rustdl after, Konclude, HermiT, KM}.

**Pre-registered pass condition (S3):** all three oracles agree with post-fix rustdl on **6 of 6**.
`dom_disj` and `dom_partial` are the DISCRIMINATING controls — the oracles must *not* report
`X ⊑ Q` there, and `dom_atomic`/`rng_atomic` are where they *do* report, which is what makes their
silence on the negatives meaningful (S5). Any disagreement blocks the merge until adjudicated.

- [ ] **Step 4: Run the sabotage battery — five, each with its predicted failure**

| # | Sabotage | Predicted |
|---|---|---|
| 1 | Revert both engine arms to the `Atomic`-only branch | the 2 bug canaries + the partial canary fail |
| 2 | Make `decompose_role_filler`'s `And` arm return `true` unconditionally | `a_partially_decomposable_filler_is_refused_by_the_gate` fails |
| 3 | Add a `ConceptExpr::Or` arm that decomposes like `And` | `a_disjunctive_filler_does_not_decompose` + `a_disjunctive_filler_is_refused_by_the_gate` fail |
| 4 | Make `is_processed_role_filler` re-implement the check locally (`matches!(Atomic|Bot|Top|And(all atomic))`) instead of calling the decomposer | **predicted: SURVIVES** — the two agree today. Recording the survivor is the point: it shows the canaries pin behaviour, not the no-drift property, which rests on the shared call site alone. |
| 5 | Short-circuit the `And` loop on the first `false` | `decompose_does_not_short_circuit_when_the_bad_conjunct_comes_first` fails — that unit test exists specifically because the integration canary orders the atomic conjunct FIRST and cannot see this |

Run each, record caught/survived, **revert before the next**. Report every survivor in the doc
(S4). Do not paper over #4 — an honest recorded limit is worth more than a claimed catch.

- [ ] **Step 5: Commit the adjudication doc**

```sh
git add docs/benchmarks/2026-09-06-conjunctive-domain-range-adjudication.md
git commit -m "docs: oracle adjudication + sabotage battery for #110"
```

---

### Task 4: Corpus gates

**Files:**
- Create: `docs/benchmarks/2026-09-06-conjunctive-filler-sweep.md`

- [ ] **Step 1: Pin both binaries (S2)**

```sh
git stash && cargo build --release && cp target/release/rustdl /tmp/rustdl-110-BEFORE && git stash pop
cargo build --release && cp target/release/rustdl /tmp/rustdl-110-AFTER
sha256sum /tmp/rustdl-110-BEFORE /tmp/rustdl-110-AFTER
```

**Verify the pin against a discriminating input before trusting it:**

```sh
for b in BEFORE AFTER; do
  echo -n "$b: "; /tmp/rustdl-110-$b classify --json $S/dom_conj.ofn | python3 -c 'import json,sys;print(len(json.load(sys.stdin)["direct_subsumptions"]))'
done
```

Expected `BEFORE: 0`, `AFTER: 2`. **If they read the same, the pin is wrong — stop.**

- [ ] **Step 2: FP=0 net**

```sh
./scripts/run-soundness-diff.sh 2>&1 | grep '^\[fp0\]'
```

Expected: 11 VERIFIED, every closure exact (galen 28007, notgalen 32739, sio 8904, wine 653,
ore-10908 6001, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16).

**Record this as INERTNESS, not correctness (S9):** corpus reach is measured zero, so a green net
here demonstrates non-regression only. The evidence is Task 3.

- [ ] **Step 3: Two-arm sweep over the 14 conjunctive-filler ontologies + 20 controls**

The 14: `ore_ont_{10080,10908,11064,11296,11305,11647,12107,12342,12451,15993,16372,16814,5964,714}`.
Controls: any 20 with zero `ObjectIntersectionOf` inside a Domain/Range, where the pass provably
cannot fire.

Run each ontology in BOTH arms, **alternating arm order by index** (S7), sequential, 240 s cap,
comparing the TRIPLE (S6). `ore_ont_10080` DNFs symmetrically at 600 s and is expected UNMEASURED;
`ore_ont_12451` needs ≥900 s (it is 230 s in one arm and 777 s in another — a one-sided timeout
here is a CANDIDATE loss, not a loss; raise the cap before recording one, S8).

**Pre-registered pass condition:** 0 lost entailments; any GAIN adjudicated against the three
oracles (a gain is the expected direction and must still be confirmed); 0 `ok → dnf` surviving
sequential re-adjudication with ≥3 runs.

- [ ] **Step 4: Fragment-routing sweep — the behaviour change this fix actually ships**

Admitting these axioms moves ontologies from `Horn` onto the **pure-EL fast path**, changing
which engine answers. Enumerate the movers **by GATE, not by grep** (grep ≠ gate — the Lever 1
precedent):

```sh
for f in /data/dumontier/ore-run/pool_sample/files/*; do
  b=$(basename "$f"); 
  before=$(timeout 120 /tmp/rustdl-110-BEFORE classify "$f" 2>/dev/null | grep -m1 '^# fragment:')
  after=$(timeout 120 /tmp/rustdl-110-AFTER  classify "$f" 2>/dev/null | grep -m1 '^# fragment:')
  [ "$before" != "$after" ] && echo -e "$b\t$before\t$after"
done | tee /tmp/fragment-movers.tsv
```

**Pre-registered pass condition:** every mover goes `Horn → pure-EL` or `OutOfFragment → Horn`
(never the reverse — the gate may only widen), and each mover is triple-identical across arms.

- [ ] **Step 5: Confirm the standing DKey discriminators did not move**

```sh
for b in BEFORE AFTER; do
  /tmp/rustdl-110-$b tbox-stats /data/dumontier/ore-run/pool_sample/files/ore_ont_9347.owl | grep -i 'concept.rules'
done
```

Expected 113 in both. If it moves, something unintended changed.

- [ ] **Step 6: Commit the sweep**

```sh
git add docs/benchmarks/2026-09-06-conjunctive-filler-sweep.md
git commit -m "docs: corpus gates for #110"
```

---

### Task 5: Documentation and close-out

**Files:**
- Modify: `CLAUDE.md` (the `owl-dl-verify` §, where #110 is recorded as UNFIXED)
- Modify: `docs/benchmarks/2026-09-05-verify-el-horn-widening-market.md` (its closing section states the defect is open)

- [ ] **Step 1: Update `CLAUDE.md`**

Rewrite the "#110 UNFIXED" block to record: the fix (decomposition as a logical identity), the
gate/engine shared-call-site design and *why* (drift is the D10 generator), the partial-vs-complete
asymmetry, the sabotage results **including survivor #4**, and — per S9 — that the corpus evidence
is inertness while Task 3's adjudication is the real evidence.

- [ ] **Step 2: Update the widening doc's closing section**

Change its "UNFIXED" framing to point at the fix, keeping the diagnosis (it is the provenance
record for how the defect was found).

- [ ] **Step 3: Full gates**

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace 2>&1 | grep -E 'test result:.*failed' | grep -v ' 0 failed'
```

Expected: fmt clean, clippy clean, no line printed by the third command.

- [ ] **Step 4: Commit, PR, close #110**

```sh
git add CLAUDE.md docs/benchmarks/2026-09-05-verify-el-horn-widening-market.md
git commit -m "docs: record the #110 fix"
gh pr create --base main --title "fix: decompose a conjunctive ObjectPropertyDomain/Range filler (#110)" --body-file <(...)
```

The PR body must state, in this order: the identity argument; that gate and engine moved in one
commit through one shared function; the oracle table; the sabotage results with survivors named;
and that **corpus reward is measured ZERO** so the sweeps are non-regression evidence only.

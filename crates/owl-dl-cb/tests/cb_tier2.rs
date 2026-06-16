//! B2 Tier-2 (ALCHQ) equality-reasoning canaries.
//!
//! These are the §8.1 / §9.4 acceptance tests for qualified number restrictions
//! `≤n R.C` / `≥n R.C`. The two headline canaries are the FP guard
//! (`..._stays_sat`) and the case-exhaustion guard (`..._pairwise_disjoint_unsat`,
//! via the §9.2 recursive Eq-disjunction discharge).
//!
//! Soundness discipline: an FP (`..._stays_sat` reports unsat / a spurious
//! subsumption) means STOP and fix. A MISS (`..._unsat` reports sat) is a
//! completeness gap to report, never papered over by relaxing the residual guard.
//!
//! Run: `cargo test -p owl-dl-cb --test cb_tier2`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::{CbHierarchy, CbOutcome};
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

struct Classified {
    hierarchy: CbHierarchy,
    internal: owl_dl_core::ontology::InternalOntology,
}

fn classify_alchq(body: &str) -> Classified {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("OFN parse error");
    let internal = convert_ontology(&onto).expect("convert_ontology error");
    let hierarchy = match owl_dl_cb::classify(&internal) {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(reason) => {
            panic!("unexpected OutOfFragment for pure-ALCHQ input: {reason}")
        }
    };
    Classified {
        hierarchy,
        internal,
    }
}

fn cls(c: &Classified, local: &str) -> owl_dl_core::ir::ClassId {
    c.internal
        .vocabulary
        .class_id(&format!("http://t/{local}"))
        .unwrap_or_else(|| panic!("class {local} not in vocabulary"))
}

fn assert_unsat(c: &Classified, local: &str) {
    assert!(
        c.hierarchy.unsat.contains(&cls(c, local)),
        "expected {local} ∈ unsat (MISSED)"
    );
}

fn assert_sat(c: &Classified, local: &str) {
    assert!(
        !c.hierarchy.unsat.contains(&cls(c, local)),
        "spurious {local} ∈ unsat (FALSE POSITIVE)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// THE Tier-2 headline canaries (§8.1 / §9.4)
// ══════════════════════════════════════════════════════════════════════════════

/// THE FP guard. `Test ⊑ ≤2 r.⊤ ⊓ ∃r.A ⊓ ∃r.B ⊓ ∃r.D`, A,B,D pairwise
/// NON-disjoint ⇒ SAT. Three witnesses, ≤2 obligation fires, but every pairwise
/// merge is consistent ⇒ no ⊥ back-propagates ⇒ no spurious unsat/subsumption.
/// If this yields `Test ⊑ ⊥` a speculative merge back-propagated ⊥
/// unconditionally instead of conditioned on its residual (§4.2). STOP and fix.
#[test]
fn tier2_at_most_two_three_witnesses_stays_sat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:B))\n\
         Declaration(Class(:D))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMaxCardinality(2 :r owl:Thing)\n\
             ObjectSomeValuesFrom(:r :A)\n\
             ObjectSomeValuesFrom(:r :B)\n\
             ObjectSomeValuesFrom(:r :D)))\n",
    );
    assert_sat(&c, "Test");
}

/// THE case-exhaustion guard (the §9.4 trace: 3→2→1→0 disjuncts). Same as above
/// but `DisjointClasses(A,B,D)` pairwise ⇒ every pairwise merge clashes ⇒ the
/// recursive discharge drives the equality disjunction to the empty clause ⇒
/// UNSAT.
#[test]
fn tier2_at_most_two_three_pairwise_disjoint_unsat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:B))\n\
         Declaration(Class(:D))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B :D)\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMaxCardinality(2 :r owl:Thing)\n\
             ObjectSomeValuesFrom(:r :A)\n\
             ObjectSomeValuesFrom(:r :B)\n\
             ObjectSomeValuesFrom(:r :D)))\n",
    );
    assert_unsat(&c, "Test");
}

/// `≤3 r.⊤` + 3 witnesses (≤ n, no surplus) ⇒ no obligation fires ⇒ SAT.
#[test]
fn tier2_at_most_three_two_distinct_pairs_stays_sat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:B))\n\
         Declaration(Class(:D))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B :D)\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMaxCardinality(3 :r owl:Thing)\n\
             ObjectSomeValuesFrom(:r :A)\n\
             ObjectSomeValuesFrom(:r :B)\n\
             ObjectSomeValuesFrom(:r :D)))\n",
    );
    assert_sat(&c, "Test");
}

/// `≤1 r.⊤` + `∃r.A` + `∃r.B` + `A ⊓ B ⊑ ⊥` (disjoint via subclass) ⇒ forced
/// merge core `{A,B}` ⊑ ⊥ ⇒ UNSAT. Exercises union-core derivation + merge
/// back-prop, not just the clique clash.
#[test]
fn tier2_merge_then_forall_clash_unsat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:B))\n\
         Declaration(Class(:Bot))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMaxCardinality(1 :r owl:Thing)\n\
             ObjectSomeValuesFrom(:r :A)\n\
             ObjectSomeValuesFrom(:r :B)))\n",
    );
    assert_unsat(&c, "Test");
}

/// `=2 r.A` exact cardinality, no conflict ⇒ SAT (no merge forced).
#[test]
fn tier2_exact_cardinality_consistent_sat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Test ObjectExactCardinality(2 :r :A))\n",
    );
    assert_sat(&c, "Test");
}

// ── Neq-meets-forced-Eq clash (fixture 47 shape) ───────────────────────────────

/// `≥2 r.A ⊓ ≤1 r.A` ⇒ Neq(s,t) from ≥2, forced Eq(s,t) from ≤1 ⇒ same-pair
/// clash ⇒ UNSAT.
#[test]
fn tier2_neq_meets_forced_eq_is_bot() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMinCardinality(2 :r :A)\n\
             ObjectMaxCardinality(1 :r :A)))\n",
    );
    assert_unsat(&c, "Test");
}

/// §10 CLOSURE (was a characterized MISS). `C ⊑ (≥2 r.A ⊔ E) ⊓ ≤1 r.A` ⟹
/// `C ⊑ E`. The `Neq(s,t)` (from `≥2`, riding residual `{E}`) and the `Eq(s,t)`
/// (from `≤1`, riding residual `{E}`) both carry the residual, so neither is a
/// unit clause — the former unit-only same-pair clash MISSED it. The general
/// §10.1 `Eq/Neq` resolution (`{E ⊔ Eq} , {E ⊔ Neq} ⟹ {E}`) now derives it
/// (sound binary resolution, §10.2; FP=0).
#[test]
fn tier2_residual_conditioned_neq_eq_clash_resolves() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:C))\n\
         Declaration(Class(:E))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:C ObjectUnionOf(ObjectMinCardinality(2 :r :A) :E))\n\
         SubClassOf(:C ObjectMaxCardinality(1 :r :A))\n",
    );
    // The sound entailment the full calculus derives; CB currently misses it.
    assert!(
        c.hierarchy
            .subsumptions
            .contains(&(cls(&c, "C"), cls(&c, "E"))),
        "C ⊑ E via residual-conditioned Eq/Neq resolution"
    );
}

/// §10 CLOSURE (was the CORE characterized MISS). `≥n R.A ⊓ ≤m R.A` with
/// `n > m ≥ 2` is UNSAT (pigeonhole). `≥3` emits 3 UNIT `Neq` pairs; `≤2` (on 3
/// witnesses) emits the 3-way disjunction `{Eq12,Eq13,Eq23}` — never a unit
/// `Eq`. The general §10.1 `Eq/Neq` resolution collapses it: resolve
/// `{Eq12,Eq13,Eq23}` with `{Neq12}` → `{Eq13,Eq23}`, with `{Neq13}` →
/// `{Eq23}`, with `{Neq23}` → `{}` = `⊥` ⇒ UNSAT (§10.3). Sound (§10.2; FP=0).
#[test]
fn tier2_min3_max2_pigeonhole_unsat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMinCardinality(3 :r :A)\n\
             ObjectMaxCardinality(2 :r :A)))\n",
    );
    assert_unsat(&c, "Test");
}

// ══════════════════════════════════════════════════════════════════════════════
// §10.5 FP-guard canaries for the general Eq/Neq resolution rule
// ══════════════════════════════════════════════════════════════════════════════

/// FP guard (a): `Eq` and `Neq` on DIFFERENT pairs must NOT resolve. A real unit
/// `Eq` on the s-pair (`≤1 s.⊤` forces the merge of the distinct, NON-disjoint
/// `B`- and `C`-witnesses — union-core `{B,C}` is SAT) coexists with a unit `Neq`
/// on the r-pair (from `≥2 r.A`). Correct pair-keying ⇒ different canonical pairs
/// ⇒ no resolvent ⇒ SAT. A pair-ignoring bug would resolve `{Eq(sB,sC)}` against
/// `{Neq(r1,r2)}` → `{}` = `⊥` ⇒ spurious UNSAT. Sharp discriminator.
#[test]
fn tier2_eq_neq_different_pairs_no_resolution_stays_sat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:B))\n\
         Declaration(Class(:C))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         Declaration(ObjectProperty(:s))\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMinCardinality(2 :r :A)\n\
             ObjectMaxCardinality(1 :s owl:Thing)\n\
             ObjectSomeValuesFrom(:s :B)\n\
             ObjectSomeValuesFrom(:s :C)))\n",
    );
    assert_sat(&c, "Test");
}

/// FP guard (b): `≥2 r.A ⊓ ≤2 r.A` (n ≤ m, no pigeonhole) stays SAT. The 2 `Neq`
/// witnesses and the `≤2` obligation (only 2 witnesses, n+1=3 not reached) ⇒ no
/// `r≤` Eq-disjunction is emitted at all ⇒ no resolution ⇒ SAT.
#[test]
fn tier2_min2_max2_no_pigeonhole_stays_sat() {
    let c = classify_alchq(
        "Declaration(Class(:A))\n\
         Declaration(Class(:Test))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Test ObjectIntersectionOf(\n\
             ObjectMinCardinality(2 :r :A)\n\
             ObjectMaxCardinality(2 :r :A)))\n",
    );
    assert_sat(&c, "Test");
}

// ══════════════════════════════════════════════════════════════════════════════
// TERMINATION: cyclic `≥n` with NO relevant `≤m` (the CB_NONTERM_REPRO shape)
// ══════════════════════════════════════════════════════════════════════════════

/// REGRESSION (cyclic-`≥n`-no-`≤m` non-termination, 2026-06-16). The minimal
/// reproducer that hung the engine >90s:
///
/// ```text
/// C0 ≡ ∃r0.(∀r0.C0)                     -- cyclic ∃/∀ on the shared role r0
/// C1 ≡ ∃r0.(≥3 r0.C4)                   -- a nested ≥3 on the SAME role r0
/// ```
///
/// There is NO `≤m` anywhere on `r0` (nor a super-role of it), so the `≥3`'s
/// distinctness (3 terms + 3 pairwise `Neq`) can never pigeonhole — it is pure
/// waste, and under the cyclic `∃` on `r0` the `Neq`-bearing / residual-bearing
/// term signatures bred unboundedly many witnesses (one `process()` pass
/// exploded). Fix: (A) normalize collapses `≥n`→`∃` on Max-free roles, and the
/// engine SHARES count-1 witnesses by `(role, ctx)` on Max-free roles (coarsening
/// the minting signature past the residual). Both are MISS-biased (sound).
///
/// The whole ontology is consistent: all three classes are satisfiable and there
/// is no atomic subsumption among them (Konclude + the hybrid agree, 0 ms). This
/// test asserts BOTH termination (it runs to completion) AND the verdict.
#[test]
fn tier2_cyclic_min_no_max_terminates_and_is_sat() {
    let c = classify_alchq(
        "Declaration(Class(:C0))\n\
         Declaration(Class(:C1))\n\
         Declaration(Class(:C4))\n\
         Declaration(ObjectProperty(:r0))\n\
         EquivalentClasses(:C0 ObjectSomeValuesFrom(:r0 ObjectAllValuesFrom(:r0 :C0)))\n\
         EquivalentClasses(:C1 ObjectSomeValuesFrom(:r0 ObjectMinCardinality(3 :r0 :C4)))\n",
    );
    // Verdict (matches the hybrid + Konclude): every class satisfiable …
    assert_sat(&c, "C0");
    assert_sat(&c, "C1");
    assert_sat(&c, "C4");
    // … and no non-trivial atomic subsumption among the three.
    for (a, b) in [("C0", "C1"), ("C1", "C0"), ("C0", "C4"), ("C1", "C4")] {
        assert!(
            !c.hierarchy.subsumptions.contains(&(cls(&c, a), cls(&c, b))),
            "spurious subsumption {a} ⊑ {b}"
        );
    }
}

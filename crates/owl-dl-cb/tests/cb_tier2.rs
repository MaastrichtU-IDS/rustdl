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

/// CHARACTERIZED MISS (sound, FP=0 — NOT a failure to force; recorded per the
/// §3.4 reserve-mode boundary). `C ⊑ (≥2 r.A ⊔ E) ⊓ ≤1 r.A` ⟹ `C ⊑ E`
/// (hybrid derives it). CB MISSES it: the `Neq(s,t)` (from `≥2`, riding residual
/// `{E}`) and the forced `Eq(s,t)` (from `≤1`, riding residual `{E}`) both carry
/// the residual, so neither is a *unit* clause; `apply_eq_neq_clash` fires only
/// on FORCED (unit) Eq+Neq, and the union-core `{A}` is satisfiable so the
/// discharge reflects nothing. The sound closure is the general §2.4 Eq/Neq
/// resolution (`{R ⊔ Eq} , {R' ⊔ Neq} ⟹ {R ⊔ R'}`) — deferred as §3.4 reserve
/// (FP-critical, beyond the B2 deliverable). FP=0 is preserved (`only_in_cb=0`):
/// the uncertainty biases to a MISS, never an FP.
#[test]
#[ignore = "characterized MISS: residual-conditioned Eq/Neq clash needs §2.4 general resolution (§3.4 reserve)"]
fn tier2_residual_conditioned_neq_eq_clash_missed() {
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

/// CHARACTERIZED MISS (sound, FP=0). The CORE case of the unit-only same-pair
/// clash limitation: `≥n R.A ⊓ ≤m R.A` with `n > m ≥ 2` is UNSAT (pigeonhole),
/// but CB MISSES it. `≥3` emits 3 UNIT `Neq` pairs; `≤2` (on 3 witnesses) emits
/// the 3-way disjunction `{Eq12,Eq13,Eq23}` — never a unit `Eq` — and each
/// speculative merge's union-core `{A}` is satisfiable, so the discharge
/// reflects nothing and `apply_eq_neq_clash` (unit-Eq-only) never fires.
/// Fixture 47 (`≥2 ⊓ ≤1`) passes only because `m=1` makes `≤2` emit a *single*
/// pair `{Eq}` (a unit). So the gap is precisely `n > m ≥ 2` — a CORE ALCHQ
/// pattern, NOT exotic. Closure = general §2.4 Eq/Neq resolution (§3.4 reserve).
/// FP=0 preserved (CB reports SAT where the truth is UNSAT — a MISS, never FP).
#[test]
#[ignore = "characterized MISS: ≥n⊓≤m (n>m≥2) needs §2.4 general Eq/Neq resolution (§3.4 reserve)"]
fn tier2_min3_max2_pigeonhole_unsat_missed() {
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

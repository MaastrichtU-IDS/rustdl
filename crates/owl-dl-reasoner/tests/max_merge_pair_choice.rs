//! Issue #76 — SOUNDNESS. `subclass` reported a subsumption that does not hold.
//!
//! The SROIQ `≤`-rule is don't-know NONDETERMINISTIC over which pair of
//! witnesses to merge. `apply_max` runs inside the deterministic saturation
//! pass and used to commit to the first pair not marked distinct; when that
//! pair was the one inconsistent combination, its clash propagated as `Unsat`
//! and the consistent merges were never tried — a FALSE POSITIVE.
//!
//! Live on the `pizza` corpus fixture as `Margherita ⊑ InterestingPizza`,
//! which both Konclude and `HermiT` refute.
//!
//! THE CONTROLS ARE THE POINT. `fp` must be refuted and `entailed` must still
//! hold: a "fix" that simply stopped deriving `≥n` subsumptions would pass the
//! first and fail the second. `lucky` pins the diagnosis — with the disjoint
//! pair moved, the OLD code answered correctly by accident, so the verdict
//! flipping with WHICH pair is disjoint is what shows this was order-dependent
//! determinism rather than semantics.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const A: &str = "http://ex.org/p#A";
const I: &str = "http://ex.org/p#I";

/// `A` has three `r`-successors but only `M`/`PT` are disjoint, so the
/// `T`-successor may merge with either: a two-successor model exists and
/// `≥3 r` is NOT entailed.
const FP: &str = r"Prefix(:=<http://ex.org/p#>)
Ontology(<http://ex.org/p>
SubClassOf(:A ObjectSomeValuesFrom(:r :M))
SubClassOf(:A ObjectSomeValuesFrom(:r :T))
SubClassOf(:A ObjectSomeValuesFrom(:r :PT))
DisjointClasses(:M :PT)
EquivalentClasses(:I ObjectMinCardinality(3 :r))
)";

/// All three pairwise disjoint ⇒ no merge is possible ⇒ `≥3 r` IS entailed.
const ENTAILED: &str = r"Prefix(:=<http://ex.org/p#>)
Ontology(<http://ex.org/p>
SubClassOf(:A ObjectSomeValuesFrom(:r :M))
SubClassOf(:A ObjectSomeValuesFrom(:r :T))
SubClassOf(:A ObjectSomeValuesFrom(:r :PT))
DisjointClasses(:M :T :PT)
EquivalentClasses(:I ObjectMinCardinality(3 :r))
)";

/// Same shape as `FP` with the disjoint pair moved. Not entailed either — and
/// the OLD code got this one RIGHT, purely because the first-enumerated pair
/// happened to be compatible.
const LUCKY: &str = r"Prefix(:=<http://ex.org/p#>)
Ontology(<http://ex.org/p>
SubClassOf(:A ObjectSomeValuesFrom(:r :M))
SubClassOf(:A ObjectSomeValuesFrom(:r :T))
SubClassOf(:A ObjectSomeValuesFrom(:r :PT))
DisjointClasses(:M :T)
EquivalentClasses(:I ObjectMinCardinality(3 :r))
)";

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

fn sub(src: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(&onto(src), A, I).unwrap()
}

/// THE FIX. Konclude and `HermiT` both refute this.
#[test]
fn non_entailed_min_cardinality_is_refuted() {
    assert!(
        !sub(FP),
        "A ⊑ ≥3 r is NOT entailed when only M/PT are disjoint — the \
         T-successor may merge with either (issue #76)"
    );
}

/// POSITIVE CONTROL — the fix must not work by refusing to derive `≥n`.
#[test]
fn genuinely_entailed_min_cardinality_still_holds() {
    assert!(
        sub(ENTAILED),
        "with all three pairwise disjoint no merge is possible, so A ⊑ ≥3 r \
         IS entailed and must still be derived"
    );
}

/// The pair the old code happened to get right. Pins that the verdict does not
/// depend on WHICH pair carries the disjointness.
#[test]
fn verdict_does_not_depend_on_which_pair_is_disjoint() {
    assert!(!sub(LUCKY), "not entailed here either");
    assert_eq!(
        sub(FP),
        sub(LUCKY),
        "same semantics, different disjoint pair — the answers must agree; \
         they did not before the fix, which is what identified the defect as \
         order-dependent determinism"
    );
}

/// `A` must remain satisfiable — a merge is available, so nothing here is
/// contradictory. Guards against "fixing" the FP by making the probe unsat.
#[test]
fn the_subject_class_stays_satisfiable() {
    for src in [FP, ENTAILED, LUCKY] {
        assert!(owl_dl_reasoner::is_class_satisfiable(&onto(src), A).unwrap());
    }
}

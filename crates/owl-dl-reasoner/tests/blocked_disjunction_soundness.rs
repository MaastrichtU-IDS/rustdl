//! Completeness canary for the blocked-node ⊔-rule termination fix.
//!
//! The corpus closures are all saturation-carried (`saturation=N tableau=0`), so
//! the Konclude closure-diff never exercises the wedge's disjunctive-`Unsat`
//! (subsumption-proving) path — it only ever asks the wedge to confirm `Sat`.
//! The fix (skip ⊔ on blocked nodes) is FP-safe by construction (skipping a rule
//! can only bias toward `Sat`, never invent an `Unsat`), so the ONLY possible
//! regression is the dual: a *missed* subsumption — a `Sat` where truth is
//! `Unsat` because a needed ⊔-clash was skipped. This canary exercises exactly
//! that path on a self-contained ontology, in the blocking-active regime.
//!
//! Ontology (all named; `r` cyclic so blocking is live). The disjuncts clash by
//! DIFFERENT paths so the told-common-subsumer preprocessing cannot compile the
//! `⊔` away into a Horn fact (that is what makes the wedge actually branch):
//!   A ⊑ ∃r.C
//!   C ⊑ ∃r.C                 — cyclic ⟹ deep r-successors get blocked
//!   C ⊑ D ⊔ E                — disjunction lives on the generated successor C
//!   C ⊑ ¬D,  C ⊑ ¬E          — each disjunct self-clashes ⟹ C ⊑ ⊥
//! Entailment: A ⊑ K for the unrelated K (A needs an r-successor that is the
//! unsatisfiable C, so A ⊑ ⊥ ⊑ K). The refutation of `A ⊓ ¬K` MUST branch C's
//! `D ⊔ E` on a successor node: the D-branch clashes `D ⊓ ¬D`, the E-branch
//! clashes `E ⊓ ¬E`, so C is unsat. With the fix the depth-1 C (unblocked) still
//! gets ⊔ applied; only the deeper cyclic repeats are blocked+skipped. If the
//! skip were over-broad (dropping the unblocked C's ⊔) the subsumption would be
//! MISSED. We assert it is found AND that the ⊔ rule was actually used.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::HyperResult;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<urn:t#>)
Ontology(
  Declaration(Class(:A))
  Declaration(Class(:C))
  Declaration(Class(:D))
  Declaration(Class(:E))
  Declaration(Class(:K))
  Declaration(ObjectProperty(:r))
  SubClassOf(:A ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectUnionOf(:D :E))
  SubClassOf(:C ObjectComplementOf(:D))
  SubClassOf(:C ObjectComplementOf(:E))
)";

fn load() -> SetOntology<RcStr> {
    let mut r = Cursor::new(ONT.as_bytes().to_vec());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

#[test]
fn disjunctive_unsat_on_blockable_successor_still_subsumes() {
    let ont = load();
    let (result, stats, _wall) =
        owl_dl_reasoner::decide_pair_probe(&ont, "urn:t#A", "urn:t#K", 256, None)
            .expect("probe ok")
            .expect("A and K resolve to named classes");
    eprintln!(
        "decide(A, K) = {result:?}  disj_branches={} branches={}",
        stats.disj_branches, stats.branches_taken
    );
    assert_eq!(
        result,
        HyperResult::Unsat,
        "A ⊑ K must still be PROVEN (Unsat of A ⊓ ¬K) post blocked-⊔ fix — a \
         Sat/Stalled here means a needed ⊔-clash on the successor was dropped"
    );
    assert!(
        stats.disj_branches > 0,
        "the proof must actually exercise the ⊔ rule (disj_branches>0); else this \
         canary is vacuous and does not guard the disjunctive-Unsat path"
    );
}

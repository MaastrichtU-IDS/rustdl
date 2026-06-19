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
//! SCOPE (precise — do not overstate): this proves the wedge's ⊔→clash→`Unsat`
//! (subsumption-proving) path SURVIVES the fix and is exercised on a generated
//! successor. It does NOT empirically cover "skipping ⊔ on a *blocked* node
//! drops no clash" — here the disjunction-bearing successor C1 is UNBLOCKED, and
//! because it clashes immediately the cyclic `∃r.C` never generates a second
//! (blockable) C, so `find_open_disjunction` never skips anything. The
//! blocked-node case rests on standard pair-blocking unravelling theory (a
//! blocked `n` realises its model via its unblocked blocker `m`, whose ⊔ IS
//! applied), reinforced by the fact that the fix moves the engine toward the
//! textbook calculus (all rules skip blocked nodes; generation already did).
//!
//! Ontology (all named). The disjuncts clash by DIFFERENT paths so the
//! told-common-subsumer preprocessing cannot compile the `⊔` away into a Horn
//! fact (that is what makes the wedge actually branch):
//!   A ⊑ ∃r.C
//!   C ⊑ ∃r.C                 — cyclic (keeps blocking machinery live)
//!   C ⊑ D ⊔ E                — disjunction on the generated successor C
//!   C ⊑ ¬D,  C ⊑ ¬E          — each disjunct self-clashes ⟹ C ⊑ ⊥
//! Entailment: A ⊑ K for the unrelated K (A needs an r-successor that is the
//! unsatisfiable C, so A ⊑ ⊥ ⊑ K). The refutation of `A ⊓ ¬K` MUST branch C's
//! `D ⊔ E`: the D-branch clashes `D ⊓ ¬D`, the E-branch clashes `E ⊓ ¬E`. If the
//! fix had over-broadly dropped the ⊔ rule, this subsumption would be MISSED. We
//! assert it is found AND that the ⊔ rule was actually used (`disj_branches`>0).

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

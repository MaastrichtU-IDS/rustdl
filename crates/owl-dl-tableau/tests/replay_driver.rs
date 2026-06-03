//! Phase 1b T2: replay driver test.
//!
//! Synthetic Horn ontology where `A ⊑ B` holds. Replay should
//! return `Subsumed` when probing `A ⊑ B` (because A's snapshot
//! plus `¬B` clashes), and `NotSubsumed` when probing `A ⊑ C`
//! (because A's snapshot plus `¬C` is satisfiable).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::{Atom, DlClause, X, clausify_with_stats};
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
use owl_dl_tableau::replay::{ReplayVerdict, replay_with_neg_sup};
use std::io::Cursor;

fn setup(
    src: &str,
) -> (
    Vec<DlClause>,
    owl_dl_core::ClassId,
    owl_dl_core::ClassId,
    owl_dl_core::ClassId,
) {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let a = internal.vocabulary.class_id("http://t/A").expect("A");
    let b = internal.vocabulary.class_id("http://t/B").expect("B");
    let c = internal.vocabulary.class_id("http://t/C").expect("C");
    (clauses, a, b, c)
}

#[test]
fn replay_subsumed_when_neg_sup_clashes() {
    let (clauses, a, b, _c) = setup(
        "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
)
",
    );
    let mut eng = HyperEngine::new(&clauses, a);
    assert_eq!(eng.decide(64), HyperResult::Sat);
    let snap = eng.satisfiability_snapshot(a).expect("snap");

    // Probe A ⊑ B: A's snapshot already contains B (told subsumer);
    // adding ¬B should clash → Subsumed.
    let neg_sup_clause = DlClause {
        body: vec![Atom::Class(b, X)],
        head: vec![], // empty head = ⊥ (clash)
    };

    let verdict = replay_with_neg_sup(&clauses, &snap, vec![neg_sup_clause]);
    assert_eq!(verdict, ReplayVerdict::Subsumed);
}

#[test]
fn replay_not_subsumed_when_neg_sup_satisfiable() {
    let (clauses, a, _b, c) = setup(
        "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
)
",
    );
    let mut eng = HyperEngine::new(&clauses, a);
    assert_eq!(eng.decide(64), HyperResult::Sat);
    let snap = eng.satisfiability_snapshot(a).expect("snap");

    // Probe A ⊑ C: A's snapshot doesn't contain C; adding ¬C is sat → NotSubsumed.
    let neg_c_clause = DlClause {
        body: vec![Atom::Class(c, X)],
        head: vec![],
    };
    let verdict = replay_with_neg_sup(&clauses, &snap, vec![neg_c_clause]);
    assert_eq!(verdict, ReplayVerdict::NotSubsumed);
}

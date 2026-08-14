//! `ClassificationStats::dropped` must be populated on EVERY classify return
//! path.
//!
//! Why this exists: the CLI used to obtain the dropped-axiom tally by calling
//! `dropped_axioms`, which is `convert_ontology(onto)?.dropped` — a SECOND full
//! conversion per invocation. Its doc comment called that "negligible next to
//! actual reasoning", which is true right up until it isn't: on
//! conversion-bound ontologies reasoning is ~0 and conversion is the whole
//! wall, so `ore_ont_868` paid 42 s twice inside a 92 s classify. The tally is
//! now carried on the result instead.
//!
//! `classify_top_down_internal` / `classify_internal_with_timeout` have several
//! return paths — the pure-EL fast path, the inconsistency short-circuit, and
//! the pairwise loop — so the stamp lives in a thin wrapper around each body
//! rather than at the construction sites. These tests pin all three paths; a
//! stamp applied per-construction-site would pass one and fail the others, and
//! a future fourth path would silently report an empty tally.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("fixture parses")
        .0
}

/// `HasKey` is unrepresentable in the IR and is RECORDED as dropped rather than
/// erroring (issue #43), so it is a stable way to make a fixture drop exactly
/// one axiom.
const DROPPED_KEY_SUBSTR: &str = "HasKey";

/// Pure-EL fast path: `classify_pure_el` returns early, before the pairwise
/// loop ever runs.
#[test]
fn dropped_survives_pure_el_fast_path() {
    let onto = parse(
        r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A :B)
    HasKey(:A () (:r))
)
",
    );
    let h = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        h.stats().pure_el_mode,
        "fixture must exercise the pure-EL fast path; got hybrid"
    );
    let stats = h.stats();
    let dropped = stats.dropped.by_kind();
    assert!(
        dropped.keys().any(|k| k.contains(DROPPED_KEY_SUBSTR)),
        "pure-EL path lost the dropped tally: {dropped:?}"
    );
}

/// Inconsistency short-circuit: `classify_inconsistent` returns early with
/// every class unsatisfiable.
#[test]
fn dropped_survives_inconsistency_short_circuit() {
    let onto = parse(
        r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:r))
    Declaration(NamedIndividual(:a))
    DisjointClasses(:A :B)
    ClassAssertion(:A :a)
    ClassAssertion(:B :a)
    HasKey(:A () (:r))
)
",
    );
    let h = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        h.stats().inconsistent,
        "fixture must exercise the inconsistency short-circuit"
    );
    let stats = h.stats();
    let dropped = stats.dropped.by_kind();
    assert!(
        dropped.keys().any(|k| k.contains(DROPPED_KEY_SUBSTR)),
        "inconsistency path lost the dropped tally: {dropped:?}"
    );
}

/// The ordinary hybrid pairwise path.
#[test]
fn dropped_survives_hybrid_pairwise_path() {
    let onto = parse(
        r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A :B)
    SubClassOf(:B ObjectAllValuesFrom(:r :C))
    HasKey(:A () (:r))
)
",
    );
    let h = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        !h.stats().pure_el_mode,
        "fixture must leave the EL fragment (it has ObjectAllValuesFrom)"
    );
    let stats = h.stats();
    let dropped = stats.dropped.by_kind();
    assert!(
        dropped.keys().any(|k| k.contains(DROPPED_KEY_SUBSTR)),
        "hybrid path lost the dropped tally: {dropped:?}"
    );
}

/// Negative control: an ontology that drops NOTHING must report an empty
/// tally, not a spuriously populated one. Without this, a stamp that copied
/// some unrelated non-empty map would pass every test above.
#[test]
fn nothing_dropped_reports_empty_tally() {
    let onto = parse(
        r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)
)
",
    );
    let h = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        h.stats().dropped.is_empty(),
        "nothing is unrepresentable here: {:?}",
        h.stats().dropped.by_kind()
    );
}

/// The tally must match what a standalone conversion reports — the value the
/// CLI previously obtained by re-converting. This is the equivalence the
/// change rests on, so it is asserted directly rather than inferred.
#[test]
fn stats_dropped_equals_standalone_conversion() {
    let src = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubClassOf(:A :B)
    HasKey(:A () (:r))
    HasKey(:B () (:s))
)
";
    let onto = parse(src);
    let h = owl_dl_reasoner::classify(&onto).expect("classify");
    let standalone = owl_dl_reasoner::dropped_axioms(&onto).expect("dropped_axioms");
    assert_eq!(
        h.stats().dropped.by_kind(),
        standalone.by_kind(),
        "the tally carried on the result must equal the one a fresh conversion reports"
    );
    assert_eq!(
        standalone.by_kind().values().sum::<u64>(),
        2,
        "fixture should drop exactly two HasKey axioms"
    );
}

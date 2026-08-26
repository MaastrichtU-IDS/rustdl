//! Issue #72 — `rdf:langString` end-to-end.
//!
//! Two defects, one fixture:
//!
//! 1. `inferred_data_property_values` dropped the `lang` element and THEN
//!    deduped, so two language-tagged assertions on one subject sharing a
//!    lexical form collapsed to a single row. That is data loss, not a
//!    formatting choice, and it was silent.
//! 2. No `rdf:langString` `DKey` bucket existed, so a language-tagged literal
//!    failed conversion (`exact_string_literal` rejects `Literal::Language`
//!    by design) and the axiom was DROPPED — nothing could confirm it.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{dropped_axioms, inferred_data_property_values};
use std::io::Cursor;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

/// One subject, two tags, ONE lexical form — the row-loss case. Before the
/// fix this returned 2 rows for 3 assertions.
const SHARED_LEXICAL: &str = r#"Ontology(<http://example.org/o>
    Declaration(DataProperty(<http://example.org/v>))
    DataPropertyAssertion(<http://example.org/v> <http://example.org/a> "bonjour"@de)
    DataPropertyAssertion(<http://example.org/v> <http://example.org/a> "bonjour"@fr)
    DataPropertyAssertion(<http://example.org/v> <http://example.org/a> "hello"@en)
)"#;

#[test]
fn tagged_values_sharing_a_lexical_form_do_not_collapse() {
    let v = inferred_data_property_values(&onto(SHARED_LEXICAL)).unwrap();
    let rows = v.quints();
    assert_eq!(
        rows.len(),
        3,
        "3 assertions must yield 3 rows; dropping `lang` before dedup merged \
         the @de and @fr rows (issue #72). got: {rows:?}"
    );
    let tags: Vec<&str> = rows.iter().map(|(_, _, _, _, l)| l.as_str()).collect();
    for t in ["de", "en", "fr"] {
        assert!(tags.contains(&t), "tag {t} missing from {tags:?}");
    }
}

/// A non-langString value reports an EMPTY tag — the column is always present
/// so the row shape is uniform for consumers.
#[test]
fn typed_value_reports_an_empty_tag() {
    let o = onto(
        r#"Ontology(<http://example.org/o>
    Declaration(DataProperty(<http://example.org/v>))
    DataPropertyAssertion(<http://example.org/v> <http://example.org/m> "37.8"^^<http://www.w3.org/2001/XMLSchema#double>)
)"#,
    );
    let v = inferred_data_property_values(&o).unwrap();
    assert_eq!(v.quints().len(), 1);
    let (_, _, lex, dt, lang) = &v.quints()[0];
    assert_eq!(lex, "37.8");
    assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#double");
    assert!(lang.is_empty(), "a typed literal has no language tag");
}

/// The second defect: a language-tagged assertion must now REACH the
/// reasoner. Before the fix both assertions were dropped at conversion, which
/// is why no query could confirm one.
#[test]
fn language_tagged_assertions_are_no_longer_dropped() {
    let o = onto(
        r#"Ontology(<http://example.org/o>
    Declaration(DataProperty(<http://example.org/v>))
    DataPropertyAssertion(<http://example.org/v> <http://example.org/de> "bonjour"@de)
    DataPropertyAssertion(<http://example.org/v> <http://example.org/fr> "bonjour"@fr)
)"#,
    );
    let dropped = dropped_axioms(&o).unwrap();
    assert!(
        dropped.is_empty(),
        "language-tagged assertions must convert, not drop: {dropped:?}"
    );
}

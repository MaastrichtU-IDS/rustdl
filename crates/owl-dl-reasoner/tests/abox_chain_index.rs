//! Correctness gate for the Phase-1a ABox-saturation chain indexing
//! (perf/abox-chain-index): the `(role, node)`-indexed inner leg must derive
//! exactly the same role-chain closure as the old brute all-pairs scan.
//!
//! Family (the perf motivation) is inconsistent, so its closure is cleared and
//! `materialize_object_property_assertions` errors — it can only validate the
//! *verdict*. This consistent fixture exercises the four closure mechanisms the
//! indexing touches: a transitive role (`ancestorOf`), a sub-role feeding it
//! (`parentOf ⊑ ancestorOf`), an **inverse** role (`childOf = inv(parentOf)`), a
//! **symmetric** role (`relatedTo`), and a role **chain with an inverse first
//! leg** (`childOf ∘ ancestorOf ⊑ relatedTo`). The full derived edge set is
//! asserted (golden), which is the byte-identity gate in committed form.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_object_property_assertions;
use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const NS: &str = "http://example.org/chain#";

fn load() -> SetOntology<RcStr> {
    let path = Path::new("tests/fixtures/regression/chain_closure_consistent.ofn");
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixture: {e}"));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn short(t: &(String, String, String)) -> (String, String, String) {
    let s = |x: &str| x.replace(NS, "");
    (s(&t.0), s(&t.1), s(&t.2))
}

#[test]
fn chain_closure_is_complete_and_exact() {
    let got: BTreeSet<(String, String, String)> = materialize_object_property_assertions(&load())
        .expect("consistent ⇒ Ok")
        .into_iter()
        .map(|t| short(&t))
        .collect();

    let expect: BTreeSet<(String, String, String)> = [
        // transitive closure of ancestorOf (parentOf ⊑ ancestorOf, transitive)
        ("a", "ancestorOf", "b"),
        ("a", "ancestorOf", "c"),
        ("a", "ancestorOf", "d"),
        ("b", "ancestorOf", "c"),
        ("b", "ancestorOf", "d"),
        ("c", "ancestorOf", "d"),
        // asserted / sub-role parentOf edges
        ("a", "parentOf", "b"),
        ("b", "parentOf", "c"),
        ("c", "parentOf", "d"),
        // inverse childOf = inv(parentOf)
        ("b", "childOf", "a"),
        ("c", "childOf", "b"),
        ("d", "childOf", "c"),
        // symmetric relatedTo (asserted d→e ⇒ e→d) + chain-derived, all symmetric-closed
        ("b", "relatedTo", "b"),
        ("b", "relatedTo", "c"),
        ("b", "relatedTo", "d"),
        ("c", "relatedTo", "b"),
        ("c", "relatedTo", "c"),
        ("c", "relatedTo", "d"),
        ("d", "relatedTo", "b"),
        ("d", "relatedTo", "c"),
        ("d", "relatedTo", "d"),
        ("d", "relatedTo", "e"),
        ("e", "relatedTo", "d"),
    ]
    .into_iter()
    .map(|(a, r, b)| (a.to_owned(), r.to_owned(), b.to_owned()))
    .collect();

    assert_eq!(
        got,
        expect,
        "chain closure mismatch\n  only in got:    {:?}\n  only in expect: {:?}",
        got.difference(&expect).collect::<Vec<_>>(),
        expect.difference(&got).collect::<Vec<_>>(),
    );
}

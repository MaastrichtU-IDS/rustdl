//! Regression: a NOMINAL-forcing range is a `DKey`-merge source too.
//!
//! Found by adversarial review of the non-merging-component gate
//! (`RUSTDL_DKEY_MERGING_GATE`, 2026-07-30). The gate skips seeding
//! `DisjointClasses(DKey_a, DKey_b)` for role components containing no
//! merge-inducing role, on the argument that `∃p.A ⊓ ∃p.B` yields two DISTINCT
//! successors so the two value keys can never share a node label.
//!
//! That argument has a hole. If `p`'s range forces every successor to be the SAME
//! individual, the o-rule collapses them and one node carries both value keys:
//!
//! ```text
//! ObjectPropertyRange(:p ObjectOneOf(:o))     <- both p-successors ARE :o
//! DataPropertyAssertion(:p :i "a")
//! DataPropertyAssertion(:p :i "b")            <- "a" and "b" now share :o's label
//! ```
//!
//! The first cut of the gate marked `p` merge-inducing only when the range filler
//! mentioned a `DKey` (`filler_mentions_dkey`), which is false for `ObjectOneOf`,
//! so it dropped the pair and reported this KB **consistent** — a MISS (sound, but
//! a completeness regression versus the pre-gate behaviour). Fixed by treating ANY
//! `ObjectPropertyRange` / `∀` as merge-inducing: a filler that forces successors
//! to coincide need not mention a `DKey`, and `ObjectPropertyRange(p, C)` with
//! `C ⊑ ObjectOneOf(o)` is not even syntactically detectable.
//!
//! NOTE the fixture uses object/data **punning** on `:p` (declared as both), which
//! is OWL 2 Full rather than OWL 2 DL. rustdl accepts it because `Vocabulary`
//! interns object and data properties into one role space. The test is kept because
//! (a) it pins the mechanism, and (b) accepting an input and then silently missing
//! its inconsistency is worse than rejecting it.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_nominal_range_merge`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const SRC: &str = r#"Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/pun>
  Declaration(ObjectProperty(:p))
  Declaration(DataProperty(:p))
  Declaration(NamedIndividual(:i))
  Declaration(NamedIndividual(:o))
  ObjectPropertyRange(:p ObjectOneOf(:o))
  DataPropertyAssertion(:p :i "a"^^xsd:string)
  DataPropertyAssertion(:p :i "b"^^xsd:string)
)
"#;

fn parse() -> SetOntology<RcStr> {
    let mut reader = Cursor::new(SRC);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// A nominal range collapses both `p`-successors onto `:o`, so `DKey("a")` and
/// `DKey("b")` share one label and clash. Must be detected regardless of the
/// merging gate, because the collapse comes from the o-rule and not from
/// functionality or a cardinality bound.
#[test]
fn nominal_range_collapses_successors_so_distinct_values_clash() {
    let onto = parse();
    let consistent = owl_dl_reasoner::is_consistent(&onto).expect("consistency check");
    assert!(
        !consistent,
        "two distinct data values on a property whose range is a singleton nominal must \
         clash: the o-rule merges both successors onto :o, so DKey(\"a\") and DKey(\"b\") \
         end up in one node label"
    );
}

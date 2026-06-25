//! Soundness regression: inverse role + qualified number restrictions must not
//! make a satisfiable class spuriously unsatisfiable.
//!
//! Minimal core distilled (via ddmin) from ORE-2015 `ore_ont_9786` (an
//! approximated-DL SIO), where rustdl reported `SIO_000500 ⊑ SIO_000440/441`
//! that both Konclude and HermiT reject. Root cause: `apply_min` generated a
//! redundant `≥n r.C532` witness because the `≥m r.C500` witnesses had not yet
//! had the told super-label `C532` (`C500 ⊑ C532`) propagated, and the extra
//! pairwise-distinct witness then made `≤k r.C532` spuriously clash — so the
//! class became unsatisfiable (⊑ everything). The fix counts a told *subclass*
//! of the filler as a witness (`ConceptRule` told-subsumer closure).
//!
//! With `r ≡ s⁻`:
//!   C500 ⊑ C532
//!   C500 ⊑ ≥2 s.C501
//!   C501 ⊑ C512
//!   C501 ⊑ =2 r.C500
//!   C512 ⊑ =2 r.C532
//! `C500` is satisfiable (Konclude + HermiT agree); the inverse role is
//! essential (it makes the C500-node a shared `r`-neighbour of the C501-nodes).
//!
//! Run: `cargo test -p owl-dl-reasoner --test inverse_cardinality_soundness`

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const SRC: &str = r#"Prefix(:=<http://ex/>)
Ontology(<http://ex/cardfp>
Declaration(Class(:C500)) Declaration(Class(:C501)) Declaration(Class(:C512))
Declaration(Class(:C532)) Declaration(Class(:Probe))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
InverseObjectProperties(:r :s)
SubClassOf(:C500 :C532)
SubClassOf(:C500 ObjectMinCardinality(2 :s :C501))
SubClassOf(:C501 :C512)
SubClassOf(:C501 ObjectExactCardinality(2 :r :C500))
SubClassOf(:C512 ObjectExactCardinality(2 :r :C532))
)"#;

fn onto() -> SetOntology<RcStr> {
    let mut r = Cursor::new(SRC);
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    o
}

const C500: &str = "http://ex/C500";
const C532: &str = "http://ex/C532";
const PROBE: &str = "http://ex/Probe";

#[test]
fn c500_is_satisfiable() {
    let o = onto();
    assert!(
        owl_dl_reasoner::is_class_satisfiable(&o, C500).expect("sat check"),
        "C500 must be satisfiable (Konclude + HermiT agree); spurious unsat = inverse+cardinality soundness bug"
    );
}

#[test]
fn c500_not_subsumed_by_unrelated_class() {
    let o = onto();
    // If C500 were spuriously unsatisfiable it would be ⊑ everything, incl. the
    // unrelated `Probe`. This is the direct false-positive symptom.
    assert!(
        !owl_dl_reasoner::is_subclass_of(&o, C500, PROBE).expect("subclass check"),
        "C500 ⊑ Probe (unrelated) is a false positive — C500 wrongly unsatisfiable"
    );
}

#[test]
fn c500_genuine_subsumption_preserved() {
    let o = onto();
    // The real subsumption must still hold (no completeness regression here).
    assert!(
        owl_dl_reasoner::is_subclass_of(&o, C500, C532).expect("subclass check"),
        "C500 ⊑ C532 is a told subsumption and must still be found"
    );
}

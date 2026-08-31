//! Canaries for cross-datatype-FAMILY `DKey` disjointness
//! (`RUSTDL_DKEY_FAMILY_DISJOINT`, default OFF).
//!
//! THE DEFECT. `seed_disjoint_bucket` runs once per datatype BUCKET, so
//! disjointness is only ever seeded WITHIN a bucket and no cross-bucket pair is
//! constructible. OWL 2 §4.1 makes the value spaces of `xsd:double`, `xsd:float` and
//! `owl:real` PAIRWISE DISJOINT, so this two-axiom ontology is inconsistent:
//!
//! ```text
//! DataPropertyRange(:p xsd:double)
//! DataPropertyAssertion(:p :a "1.0"^^xsd:float)
//! ```
//!
//! Konclude v0.7.0-1138 AND `HermiT` 1.4.3 both report it inconsistent; rustdl reported
//! `consistent` with no incompleteness signal. It is the minimal core of BOTH
//! `ore_ont_16321` and `ore_ont_4198` — two-peer-confirmed corpus instances.
//!
//! This is the completeness TWIN of the v0.4.9 FALSE POSITIVE: that fix split
//! `xsd:float` from `xsd:double` so they could no longer cross-SUBSUME, and nothing
//! then made the split buckets DISJOINT.
//!
//! DIRECTION OF RISK — INVERTED. The lever emits MORE disjointness, so its failure
//! mode is a FALSE POSITIVE, not a miss. The negative controls below are the point:
//! [`same_family_integer_under_decimal_stays_consistent`] is the load-bearing one,
//! because `xsd:integer ⊆ xsd:decimal ⊆ owl:real` means those two BUCKETS share one
//! FAMILY and must NOT be seeded disjoint.
//!
//! SCOPE — deliberately three families, chosen by MEASUREMENT, not by symmetry:
//! `real = {integer, decimal}`, `double`, `float`. Two candidates were REFUTED by
//! oracle probes and are excluded: `xsd:date`/`xsd:dateTime` (not in the OWL 2
//! datatype map at all — `HermiT` refuses them, Konclude says consistent), and
//! numeric-vs-`string` (already caught on this path, and left as a documented miss on
//! the class-expression path by `dkey_emit_order.rs`'s
//! `cross_datatype_stays_a_deliberate_miss`, which must stay green).
//!
//! NOTE ON EVIDENCE. The curated corpus is INERT for this area, so the FP=0 net shows
//! non-regression only. These canaries plus the Konclude ∪ `HermiT` adjudication are the
//! actual evidence.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_family_disjointness`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Mirrors dkey_emit_order.rs.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => unsafe { std::env::set_var(self.key, v) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

/// A two-axiom ontology: one `DataPropertyRange` and one `DataPropertyAssertion`.
fn probe(range: &str, value: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/fam>\n\
         Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))\n\
         DataPropertyRange(:p {range})\n\
         DataPropertyAssertion(:p :a {value})\n)\n"
    );
    let (o, _) = read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    o
}

fn consistent_with(flag: &str, range: &str, value: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_DKEY_FAMILY_DISJOINT", flag);
    owl_dl_reasoner::is_consistent(&probe(range, value)).unwrap()
}

// ---- POSITIVES: entailed inconsistencies, both oracles agree -----------------

/// The exact two-axiom core `ore_ont_16321` and `ore_ont_4198` reduce to.
#[test]
fn double_range_with_float_value_is_inconsistent() {
    assert!(
        !consistent_with("1", "xsd:double", "\"1.0\"^^xsd:float"),
        "OWL 2 gives xsd:float and xsd:double disjoint value spaces, so a float-typed \
         literal cannot satisfy an xsd:double range — Konclude AND `HermiT` both report \
         this inconsistent"
    );
}

#[test]
fn integer_range_with_float_value_is_inconsistent() {
    assert!(
        !consistent_with("1", "xsd:integer", "\"1.0\"^^xsd:float"),
        "xsd:float's value space is disjoint from owl:real, which contains xsd:integer"
    );
}

#[test]
fn integer_range_with_double_value_is_inconsistent() {
    assert!(
        !consistent_with("1", "xsd:integer", "\"1.5\"^^xsd:double"),
        "xsd:double's value space is disjoint from owl:real, which contains xsd:integer"
    );
}

// ---- NEGATIVE CONTROLS: the FP direction this lever risks --------------------

/// THE LOAD-BEARING FP GUARD. `xsd:integer ⊆ xsd:decimal ⊆ owl:real`, so these two
/// BUCKETS are one FAMILY. A naive "different bucket ⇒ disjoint" rule makes this
/// unsatisfiable — a false positive. Both oracles report it CONSISTENT.
#[test]
fn same_family_integer_under_decimal_stays_consistent() {
    assert!(
        consistent_with("1", "xsd:decimal", "\"1\"^^xsd:integer"),
        "integer ⊆ decimal: an integer-typed value satisfies a decimal range, and \
         seeding those two buckets disjoint would be a FALSE POSITIVE"
    );
}

#[test]
fn double_range_with_double_value_stays_consistent() {
    assert!(consistent_with("1", "xsd:double", "\"1.0\"^^xsd:double"));
}

#[test]
fn float_range_with_float_value_stays_consistent() {
    assert!(consistent_with("1", "xsd:float", "\"1.0\"^^xsd:float"));
}

#[test]
fn integer_range_with_integer_value_stays_consistent() {
    assert!(consistent_with("1", "xsd:integer", "\"1\"^^xsd:integer"));
}

// ---- The flag is load-bearing ------------------------------------------------

/// Pins the default OFF and proves the lever — not something else — is what closes
/// the gap. Without this, a canary that passed for an unrelated reason would read as
/// coverage.
#[test]
fn the_gap_is_open_with_the_lever_off() {
    assert!(
        consistent_with("0", "xsd:double", "\"1.0\"^^xsd:float"),
        "with the lever off this must remain the documented MISS"
    );
}

// ---- The markers are internal machinery, not user classes --------------------

/// REGRESSION GUARD for a defect this change actually shipped mid-development:
/// `ReportedClasses::collect` excluded only `DKEY_IRI_PREFIX`, so enabling the
/// lever turned a 2-class ontology into a reported 5 — the three family markers
/// surfaced as user classes. `DFAM_IRI_PREFIX` now joins that exclusion (and the
/// two `realize` filters).
#[test]
fn family_markers_never_surface_as_user_classes() {
    let src = "Prefix(:=<http://ex.org/>)\n\
        Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
        Ontology(<http://ex.org/leak>\n\
        Declaration(Class(:A)) Declaration(Class(:B)) Declaration(DataProperty(:p))\n\
        SubClassOf(:A :B)\n\
        SubClassOf(:A DataSomeValuesFrom(:p xsd:double))\n\
        SubClassOf(:B DataSomeValuesFrom(:p xsd:integer))\n)\n";
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .unwrap();
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_DKEY_FAMILY_DISJOINT", "1");
    let c = owl_dl_reasoner::classify(&onto).unwrap();
    let leaked: Vec<_> = c
        .classes()
        .iter()
        .filter(|i| i.contains("rustdl-dfam"))
        .collect();
    assert!(
        leaked.is_empty(),
        "the per-family marker classes are internal machinery and must never be \
         reported: {leaked:?}"
    );
    assert_eq!(
        c.classes().len(),
        2,
        "only :A and :B are user classes; the lever must not change the reported count"
    );
}

// ---- Inertness: the lever must not touch what it cannot fire on --------------

/// Count the interned family-marker classes after conversion. Inspecting the
/// VOCABULARY, not `classify` output — a marker is filtered from reporting, so a
/// spuriously interned one is INVISIBLE there. An earlier version of these two
/// tests compared classify rows and a sabotage of the guard SURVIVED them.
fn marker_count(flag: &str, body: &str) -> usize {
    let src = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/inert>\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_DKEY_FAMILY_DISJOINT", flag);
    let internal = owl_dl_core::convert_ontology(&onto).unwrap();
    internal
        .vocabulary
        .classes()
        .filter(|(_, iri)| owl_dl_core::is_dfam_iri(iri))
        .count()
}

/// REGRESSION GUARD for a defect a two-arm ORE sweep caught. The first cut
/// interned all three markers and emitted all three `DisjointClasses`
/// UNCONDITIONALLY, so every ontology gained 3 classes and 3 axioms — including
/// the great majority of ORE containing no numeric datatype at all. Class ids
/// shifted corpus-wide and walls moved on ontologies the lever cannot possibly
/// affect (`ore_ont_1016` 0.24 s → 18.77 s, with ZERO
/// `xsd:double`/`float`/`integer`/`decimal` in it).
#[test]
fn an_ontology_with_no_numeric_data_is_completely_inert() {
    let body = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                SubClassOf(:A :B)\n\
                SubClassOf(:B :C)";
    assert_eq!(marker_count("0", body), 0);
    assert_eq!(
        marker_count("1", body),
        0,
        "no DKeys at all ⟹ the lever must intern NO marker class; interning them \
         unconditionally shifted class ids corpus-wide"
    );
}

/// A single populated family has no cross-family partner, so nothing should be
/// seeded and no marker interned. Pins the `live.len() < 2` early return.
#[test]
fn a_single_numeric_family_alone_seeds_nothing() {
    let body = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(DataProperty(:p))\n\
                SubClassOf(:A :B)\n\
                SubClassOf(:A DataSomeValuesFrom(:p xsd:double))\n\
                SubClassOf(:B DataSomeValuesFrom(:p xsd:double))";
    assert_eq!(marker_count("0", body), 0);
    assert_eq!(
        marker_count("1", body),
        0,
        "one family alone cannot form a disjoint pair — interning a marker for it \
         is dead weight that perturbs class ids for nothing"
    );
}

/// The positive control for [`marker_count`]: with TWO populated families the
/// markers ARE interned. Without this, a `marker_count` that always returned 0
/// would make both inertness tests pass vacuously.
#[test]
fn two_populated_families_do_intern_their_markers() {
    let body = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(DataProperty(:p))\n\
                SubClassOf(:A DataSomeValuesFrom(:p xsd:double))\n\
                SubClassOf(:B DataSomeValuesFrom(:p xsd:integer))";
    assert_eq!(marker_count("0", body), 0, "lever off ⟹ never interned");
    assert_eq!(
        marker_count("1", body),
        2,
        "double and real are both populated here, so exactly those two markers \
         are interned — and float, which is absent, is not"
    );
}

// ---- SCOPE: which surface the lever actually reaches --------------------------

/// PINS A KNOWN LIMITATION so the lever is not mistaken for a full fix.
///
/// The clash needs `∃p.DKey(float) ⊓ ∀p.DKey(double)` to meet on a GENERATED
/// successor, which the hybrid consistency path derives and `classify`'s
/// inconsistency detection does not: the latter is pre-check-only
/// (`top_is_unsat` + `abox_saturation`, both over NAMED individuals), and a data
/// value is not a named individual. So with the lever ON, `is_consistent` is
/// correct while `classify` still reports `consistent: true`.
///
/// That is a DISAGREEMENT between two surfaces — the class of bug
/// `RUSTDL_CLASSIFY_INCONSISTENCY` was introduced to fix for `family.ofn` — and
/// it is the reason this lever stays default OFF pending a classify-path fix.
/// If someone closes that gap, THIS test fails, which is the intent.
#[test]
fn the_lever_reaches_is_consistent_but_not_classify() {
    let onto = probe("xsd:double", "\"1.0\"^^xsd:float");
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_DKEY_FAMILY_DISJOINT", "1");
    assert!(
        !owl_dl_reasoner::is_consistent(&onto).unwrap(),
        "is_consistent runs the hybrid path and MUST see the clash"
    );
    let c = owl_dl_reasoner::classify(&onto).unwrap();
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "KNOWN LIMITATION, pinned deliberately: classify's inconsistency detection \
         is pre-check-only and does not see this clash. If this assertion starts \
         failing the classify path has been extended — delete this test, update \
         docs/known-limitations/dkey-cross-family-disjointness-missing.md, and \
         reconsider the default."
    );
}

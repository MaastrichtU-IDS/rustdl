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

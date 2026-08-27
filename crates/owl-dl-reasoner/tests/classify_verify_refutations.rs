//! Issue #66 — `classify` omitted a subsumption that `subclass` on the SAME
//! binary proves.
//!
//! Out of the fragment where the wedge's `Sat` verdict is complete BY
//! CONSTRUCTION, `trust_sat` concludes "not subsumed" without consulting the
//! tableau. On this 6-axiom ALC fixture the wedge is incomplete
//! (`∀p.(A ⊔ ¬R)` against `A ⊑ V`, `V ⊑ R`, `Disjoint(Pizza, R)`), so
//! `Cap ⊑ VE` was silently absent from the hierarchy while `subclass`,
//! `prove` and `HermiT` all confirm it — and `incomplete` stayed `false`, so a
//! consumer could not tell. A migration tool diffing two `classify`
//! hierarchies read those absences as real and reported ~46% spurious "lost"
//! subsumptions.
//!
//! `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1` withdraws the trust exactly there.
//! It is OFF by default: a two-arm sweep of all 1,920 ORE ontologies found
//! **zero** entailments recovered corpus-wide against +21.6% wall, so the
//! pattern is real but does not occur in real ontologies.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::sync::Mutex;

/// Serialises env mutation within this binary — these tests set a process-wide
/// variable, so they must not run concurrently with each other.
static ENV: Mutex<()> = Mutex::new(());

struct Guard(Option<std::ffi::OsString>);
impl Drop for Guard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: single-threaded within the ENV mutex.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS", v),
                None => std::env::remove_var("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS"),
            }
        }
    }
}
#[allow(unsafe_code)]
fn set_flag(v: Option<&str>) -> Guard {
    let g = Guard(std::env::var_os("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS"));
    // SAFETY: single-threaded within the ENV mutex.
    unsafe {
        match v {
            Some(x) => std::env::set_var("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS", x),
            None => std::env::remove_var("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS"),
        }
    }
    g
}

/// The issue's own fixture, verbatim.
const MIN66: &str = r"Prefix(:=<http://ex#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:V)) Declaration(Class(:R)) Declaration(Class(:Pizza)) Declaration(Class(:Cap)) Declaration(Class(:VE))
Declaration(ObjectProperty(:p))
SubClassOf(:A :V)
SubClassOf(:Cap :Pizza)
SubClassOf(:V :R)
DisjointClasses(:Pizza :R)
EquivalentClasses(:VE ObjectIntersectionOf(:Pizza ObjectAllValuesFrom(:p ObjectUnionOf(:V ObjectComplementOf(:R)))))
SubClassOf(:Cap ObjectAllValuesFrom(:p ObjectUnionOf(:A ObjectComplementOf(:R))))
)";

const CAP: &str = "http://ex#Cap";
const VE: &str = "http://ex#VE";

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

/// `subclass` proves it. This is the ground truth the hierarchy must match,
/// and it is independent of the flag.
#[test]
fn subclass_proves_the_subsumption() {
    let o = onto(MIN66);
    assert!(owl_dl_reasoner::is_subclass_of(&o, CAP, VE).unwrap());
}

/// THE FIX: with the flag on, `classify` agrees with `subclass`.
#[test]
fn flag_on_classify_agrees_with_subclass() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(Some("1"));
    let c = owl_dl_reasoner::classify(&onto(MIN66)).unwrap();
    assert!(
        c.is_subclass(CAP, VE),
        "with RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1, classify must not omit \
         a subsumption `subclass` proves (issue #66)"
    );
}

/// NEGATIVE CONTROL — the flag is LOAD-BEARING. Off (the default), the
/// subsumption is still missing. Without this, the test above could pass for
/// some unrelated reason and the flag would be doing nothing.
#[test]
fn flag_off_reproduces_the_defect() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(Some("0"));
    let c = owl_dl_reasoner::classify(&onto(MIN66)).unwrap();
    assert!(
        !c.is_subclass(CAP, VE),
        "flag OFF must reproduce issue #66 — if this now passes, the defect \
         was closed elsewhere and this flag's rationale needs re-checking"
    );
}

/// The default is OFF, so the shipped behaviour is unchanged. Pinned here as
/// well as in `flag_defaults.rs` because the DEFAULT is the whole reason this
/// is safe to merge without a corpus flip.
#[test]
fn default_is_off() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(None);
    assert!(!owl_dl_reasoner::classify_verify_refutations_enabled());
    let c = owl_dl_reasoner::classify(&onto(MIN66)).unwrap();
    assert!(!c.is_subclass(CAP, VE), "default must be unchanged");
}

/// The flag must NOT invent subsumptions. It makes classify do MORE tableau
/// work, so its risk direction is a false POSITIVE — the opposite of the
/// default path's. Every pair the flag-on hierarchy reports must also be
/// reported by `is_subclass_of`, which is the complete per-pair oracle.
#[test]
fn flag_on_introduces_no_subsumption_the_oracle_rejects() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(Some("1"));
    let o = onto(MIN66);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    let names = [
        "http://ex#A",
        "http://ex#V",
        "http://ex#R",
        "http://ex#Pizza",
        CAP,
        VE,
    ];
    for sub in names {
        for sup in names {
            if sub == sup {
                continue;
            }
            if c.is_subclass(sub, sup) {
                assert!(
                    owl_dl_reasoner::is_subclass_of(&o, sub, sup).unwrap(),
                    "classify reported {sub} ⊑ {sup} but the per-pair oracle refutes it"
                );
            }
        }
    }
}

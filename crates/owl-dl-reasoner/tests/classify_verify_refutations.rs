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
//!
//! ## #66 IS CLOSED, AND NOT BY THIS FLAG (2026-08-30)
//!
//! #78/#83 fixed the underlying wedge incompleteness at ROOT: `head_atom_for`
//! emitted the fresh-`Q` naming clause on the successor variable when the clause
//! states a UNIVERSAL property, leaving it with no `X` atom and no `Role` atom —
//! unmatchable, never fired, so the wedge returned `Sat` on genuinely subsumed
//! pairs. `Cap ⊑ VE` is therefore now found **at the default**, flag or no flag.
//!
//! Two tests in this file asserted the OPPOSITE and failed the moment the branch
//! was rebased onto that fix — `flag_off_reproduces_the_defect` and the
//! behavioural half of `default_is_off`. They were correct when written and are
//! kept here, inverted, because the flip is the evidence that #66 is closed by a
//! mechanism rather than by assertion. Both now pin the fixed behaviour.
//!
//! **What the flag is still FOR.** Not completeness — that is settled. It is the
//! only in-tree way to route `classify` through the per-pair tableau, which is
//! what makes per-pair false positives visible to an oracle diff. It is the
//! instrument that found #76, and
//! `crates/owl-dl-reasoner/tests/per_pair_fp_gate.rs` is the standing gate built
//! on it. Keep the flag even though its original rationale has evaporated.
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

/// WAS a negative control asserting the flag was load-bearing for #66; it
/// asserted `!is_subclass(CAP, VE)` with the flag OFF and **failed on rebase**,
/// exactly as its own message instructed ("if this now passes, the defect was
/// closed elsewhere"). It was: #78/#83 fixed the wedge at root. Inverted, it now
/// pins that the fix is real and independent of this flag — which is also what
/// makes the flag safe to keep purely as an FP instrument.
#[test]
fn sixty_six_is_closed_at_the_default_without_this_flag() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(Some("0"));
    let c = owl_dl_reasoner::classify(&onto(MIN66)).unwrap();
    assert!(
        c.is_subclass(CAP, VE),
        "issue #66 must stay closed at the DEFAULT — #78/#83 fixed the wedge \
         clause anchoring at root, so `Cap ⊑ VE` must be found with \
         RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=0. A failure here means that root \
         fix regressed, NOT that this flag is needed again."
    );
}

/// The default is OFF. Pinned here as well as in `flag_defaults.rs` because the
/// default is the whole reason the flag is safe to carry: enabling it costs
/// +21.6% wall corpus-wide and recovers nothing.
///
/// The second assertion USED to read `!c.is_subclass(CAP, VE)` ("default must be
/// unchanged") and failed on rebase. That was not a regression: #78/#83 closed
/// #66 at root, so the unflagged default now finds the subsumption. Asserting
/// the live behaviour keeps this a real check rather than a stale one.
#[test]
fn default_is_off() {
    let _l = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = set_flag(None);
    assert!(!owl_dl_reasoner::classify_verify_refutations_enabled());
    let c = owl_dl_reasoner::classify(&onto(MIN66)).unwrap();
    assert!(
        c.is_subclass(CAP, VE),
        "unset must behave as `0`, and at `0` #66 is closed by #78/#83"
    );
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

//! #180: the EL saturator's role hierarchy closes over SIGNED roles
//! (`RUSTDL_SAT_SIGNED_ROLE_SUPER`, default ON, `=0` reverts).
//!
//! THE MISS. `hasMember ⊑ hasElt`, `memberOf ≡ hasMember⁻`, `eltOf ≡ hasElt⁻`
//! entail `memberOf ⊑ eltOf`, so `ObjectPropertyDomain(eltOf, G)` applies to
//! `P ⊑ ∃memberOf.Z` and `P ⊑ G` follows. The saturator's role closure was
//! named-only, so it never derived the inclusion and left the pair to the wedge.
//! The wedge reads inverses only with Layer A on, and Layer A is off above
//! `RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`, which is where the COSMO
//! variants `ore_ont_16420` / `ore_ont_9577` sit: both missed
//! `JewishPerson ⊑ Group` and `Rabbi ⊑ Group` (Konclude derives them).
//!
//! These tests force Layer A off to stand in for "above the cap" on a small
//! fixture. Saturator-level unit tests (`signed_closure_*` in
//! `owl-dl-saturation`) pin the closure itself, including two FP guards.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_signed_role_super`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var/remove_var are unsafe under edition 2024. Every test in
        // this binary holds ENV_MUTEX while the guard lives; restored on Drop.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// The COSMO shape, reduced: the domain is the union `L ⊔ G` with `L ⊑ … ⊑ G`,
/// and the filler is a nominal, as in `ore_ont_16420`.
const COSMO: &str = "Prefix(:=<http://t/>)
Ontology(<http://t/x>
Declaration(Class(:P)) Declaration(Class(:G)) Declaration(Class(:L))
Declaration(Class(:LO)) Declaration(Class(:OG))
Declaration(ObjectProperty(:memberOf)) Declaration(ObjectProperty(:hasMember))
Declaration(ObjectProperty(:hasElt)) Declaration(ObjectProperty(:eltOf))
Declaration(NamedIndividual(:j))
SubClassOf(:OG :G) SubClassOf(:L :LO) SubClassOf(:LO :OG)
SubObjectPropertyOf(:hasMember :hasElt)
InverseObjectProperties(:hasElt :eltOf)
InverseObjectProperties(:hasMember :memberOf)
SubClassOf(:P ObjectHasValue(:memberOf :j))
ObjectPropertyDomain(:eltOf ObjectUnionOf(:L :G))
)
";

/// `(P ⊑ G derived, wedge_hierarchy_blind)` with Layer A off and the signed
/// closure at `signed` (`None` = default).
fn classify_cosmo(signed: Option<&str>) -> (bool, bool) {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _a = SetEnvGuard::set("RUSTDL_CLASSIFY_ROLE_HIERARCHY", Some("0"));
    let _s = SetEnvGuard::set("RUSTDL_SAT_SIGNED_ROLE_SUPER", signed);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(COSMO.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap();
    let c = owl_dl_reasoner::classify(&onto).unwrap();
    (
        c.is_subclass("http://t/P", "http://t/G"),
        c.stats().wedge_hierarchy_blind,
    )
}

/// Default: the saturator derives `P ⊑ G` itself, so the hierarchy-blind wedge
/// no longer matters for it.
#[test]
fn the_saturator_derives_a_domain_through_inverse_pairs() {
    let (derived, blind) = classify_cosmo(None);
    assert!(blind, "precondition: the wedge ran without Layer A");
    assert!(derived, "memberOf ⊑ eltOf carries eltOf's domain to P");
}

/// `=0` reverts to the named-only closure, and the pair is missed again. This
/// pins both the default (ON) and that the flag is what recovers it.
#[test]
fn the_named_only_closure_misses_it() {
    let (derived, blind) = classify_cosmo(Some("0"));
    assert!(blind);
    assert!(
        !derived,
        "with the flag off nothing else derives P ⊑ G here"
    );
}

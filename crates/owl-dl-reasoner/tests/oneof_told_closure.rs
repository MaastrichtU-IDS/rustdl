//! #180: the enumeration common-subsumer rule intersects the members' told
//! closures (`RUSTDL_ONEOF_TOLD_CLOSURE`, default ON, `=0` reverts).
//!
//! THE MISS. `ore_ont_6822` has `EducationOrdering ≡ {a₁ … a₈}` whose members are
//! typed with eight different leaves (`ehBachelor`, `eElementary`, …), each with a
//! told path to `Education`. So `EducationOrdering ⊑ Education` (Konclude and
//! `HermiT` agree). The saturator's rule intersected only the ASSERTED types, which
//! share nothing but `EducationOrdering` itself, and the pair was left to a tableau
//! probe that does not finish inside the per-pair budget. `classify` reported it
//! missing with `incomplete: false`.
//!
//! Run: `cargo test -p owl-dl-reasoner --test oneof_told_closure`

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

/// The 6822 shape, reduced: `Education` is a defined class outside EL, so only
/// the enumeration rule can derive `Ordering ⊑ Education` in the saturator.
const SHAPE: &str = "Prefix(:=<http://t/>)
Ontology(<http://t/x>
Declaration(Class(:Ordering)) Declaration(Class(:Education))
Declaration(Class(:Higher)) Declaration(Class(:Bachelor)) Declaration(Class(:Elementary))
Declaration(Class(:Other))
Declaration(ObjectProperty(:inDomain))
Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:e))
EquivalentClasses(:Education ObjectIntersectionOf(ObjectMinCardinality(1 :inDomain)
  ObjectUnionOf(:Elementary :Higher)))
SubClassOf(:Bachelor :Higher) SubClassOf(:Higher :Education)
SubClassOf(:Elementary :Education)
DisjointClasses(:Elementary :Higher)
EquivalentClasses(:Ordering ObjectOneOf(:b :e))
ClassAssertion(:Ordering :b) ClassAssertion(:Bachelor :b)
ClassAssertion(:Ordering :e) ClassAssertion(:Elementary :e)
)
";

fn ordering_is_education(flag: Option<&str>) -> bool {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _f = SetEnvGuard::set("RUSTDL_ONEOF_TOLD_CLOSURE", flag);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(SHAPE.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap();
    let c = owl_dl_reasoner::classify(&onto).unwrap();
    // FP guard: `b` is Higher, `e` is not, so Ordering ⊄ Higher.
    assert!(
        !c.is_subclass("http://t/Ordering", "http://t/Higher"),
        "only one member reaches Higher"
    );
    c.is_subclass("http://t/Ordering", "http://t/Education")
}

#[test]
fn members_meet_at_a_told_superclass() {
    assert!(ordering_is_education(None));
}

/// `=0` pins the default and shows the flag is what derives the pair. If another
/// engine starts deriving it with the flag off, flip this test rather than
/// deleting it.
#[test]
fn asserted_types_alone_do_not_meet() {
    assert!(!ordering_is_education(Some("0")));
}

//! #180: a definition's `∀R.B` conjunct is implied by its `∃R.B` conjunct when
//! `R` is functional (`RUSTDL_FUNCTIONAL_FORALL_DEF`, default ON, `=0` reverts).
//!
//! THE MISS. `ore_ont_13859` (SAO) defines `D ≡ A ⊓ ∃R.B ⊓ ∀R.B` with `R`
//! functional through `InverseFunctional` on its declared inverse, and has
//! `Y ⊑ A ⊓ ∃R.B`. So `Y ⊑ D`, and 17 classes below `Y` with it. Konclude and
//! `HermiT` agree. `classify` missed all 17 with `incomplete: false`: the EL
//! saturator cannot read `D`'s sufficient direction (the `∀`), and the per-pair
//! tableau needs over 120 s each.
//!
//! Run: `cargo test -p owl-dl-reasoner --test functional_forall_def`

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

/// The 13859 shape, reduced. `X` reaches `A ⊓ ∃r.B` through a told chain, and
/// `r` is functional only as the inverse of an inverse-functional `q`.
const SHAPE: &str = "Prefix(:=<http://t/>)
Ontology(<http://t/x>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:A))
Declaration(Class(:B)) Declaration(Class(:D)) Declaration(Class(:E))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:q))
InverseObjectProperties(:q :r)
InverseFunctionalObjectProperty(:q)
SubClassOf(:X :Y)
SubClassOf(:Y :A)
SubClassOf(:Y ObjectSomeValuesFrom(:r :B))
EquivalentClasses(:D ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :B) ObjectAllValuesFrom(:r :B)))
EquivalentClasses(:E ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :B) ObjectAllValuesFrom(:r :A)))
)
";

fn classify(flag: Option<&str>) -> owl_dl_reasoner::Classification {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _f = SetEnvGuard::set("RUSTDL_FUNCTIONAL_FORALL_DEF", flag);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(SHAPE.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap();
    owl_dl_reasoner::classify(&onto).unwrap()
}

#[test]
fn a_functional_closure_definition_is_reached() {
    let c = classify(None);
    assert!(c.is_subclass("http://t/X", "http://t/D"));
    assert!(c.is_subclass("http://t/Y", "http://t/D"));
}

/// FP guard: `E` closes `r` to `A`, which the `∃r.B` conjunct does not imply.
#[test]
fn an_unimplied_forall_is_not_dropped() {
    let c = classify(None);
    assert!(!c.is_subclass("http://t/Y", "http://t/E"));
}

/// FP guard: the `∃r.B` conjunct must stay in the emitted left-hand side. Without
/// it the pass would emit `A ⊑ D`, and `A` alone has no `r`-successor.
#[test]
fn the_existential_conjunct_is_kept() {
    let c = classify(None);
    assert!(!c.is_subclass("http://t/A", "http://t/D"));
}

/// `=0` pins the default and shows the pass is what derives the pair. If another
/// engine starts deriving it with the pass off, flip this test, don't delete it.
#[test]
fn without_the_pass_the_pair_is_missed() {
    let c = classify(Some("0"));
    assert!(!c.is_subclass("http://t/Y", "http://t/D"));
}

//! Disjointness on the saturator fast path (no-functional): clash detection,
//! ∃-fact back-propagation, a satisfiable control, and fast-vs-hybrid identity.
#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

// SetEnvGuard: set/unset an env var for the duration of a test, restore on drop.
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
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

// A) basic disjointness clash: X ⊑ A, X ⊑ B, Disjoint(A,B) ⇒ X unsatisfiable.
#[test]
fn disjoint_clash_makes_class_unsatisfiable() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)",
    );
    let c = classify(&o).expect("classify");
    assert!(
        c.unsatisfiable_classes().contains(&"http://e#X"),
        "X⊑A⊓B (disjoint) must be unsatisfiable"
    );
}

// B) unsat back-propagates through an ∃-fact: Y ⊑ ∃r.X, X ⊑ A⊓B disjoint ⇒ X and Y unsatisfiable.
#[test]
fn disjoint_clash_backpropagates_through_existential() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Y))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)\n\
         SubClassOf(:Y ObjectSomeValuesFrom(:r :X))",
    );
    let c = classify(&o).expect("classify");
    let u = c.unsatisfiable_classes();
    assert!(u.contains(&"http://e#X"), "X must be unsatisfiable");
    assert!(
        u.contains(&"http://e#Y"),
        "Y⊑∃r.X with X unsat must be unsatisfiable (∃-fact back-prop)"
    );
}

// C) satisfiable control: disjoint classes with DISTINCT subclasses ⇒ NO spurious unsat.
#[test]
fn disjoint_without_shared_subclass_is_satisfiable() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Z))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:Z :B)",
    );
    let c = classify(&o).expect("classify");
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "no class is ⊑ both A and B; nothing must be unsat"
    );
}

// D) fast (shortcircuit ON) vs hybrid (shortcircuit OFF) produce the SAME unsatisfiable set.
#[test]
fn disjoint_fastpath_matches_hybrid() {
    let body = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Y))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)\n\
         SubClassOf(:Y ObjectSomeValuesFrom(:r :X))";
    let _lock = ENV_LOCK.lock().unwrap();
    let fast = {
        let _g = SetEnvGuard::set("RUSTDL_HORN_SHORTCIRCUIT", "1");
        let mut u = classify(&onto(body))
            .expect("classify fast")
            .unsatisfiable_classes()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        u.sort();
        u
    };
    let hybrid = {
        let _g = SetEnvGuard::set("RUSTDL_HORN_SHORTCIRCUIT", "0");
        let mut u = classify(&onto(body))
            .expect("classify hybrid")
            .unsatisfiable_classes()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        u.sort();
        u
    };
    assert_eq!(
        fast, hybrid,
        "fast-path (saturator) unsat set must equal hybrid-path unsat set"
    );
}

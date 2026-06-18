//! Inverse-role domain / range triggering in the hypertableau wedge.
//!
//! The wedge clausifies `ObjectPropertyDomain(ObjectInverseOf(p), C)` as
//! a single first-leg role-body clause `Atom::Role(Inverse(p), X, y) → C(X)`.
//! Before the fix, `Event::Edge(src, p, tgt)` only fired `role_trigger` clauses
//! at `src` — so the clause rooted at `tgt` (which now has an `Inverse(p)`-
//! successor) never fired.  The fix adds an `inverse_first_trigger` index and
//! fires it at `tgt`.
//!
//! NEGATIVES-FIRST: the `*_consistent` control must stay consistent after the
//! fix — if it flips inconsistent the fix introduced a FP.
//!
//! Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
";

// Env-mutation plumbing: serialize RUSTDL_ABOX_CHECK=0 against other
// env-mutating tests, restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one
        // test, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Parse + consistency-check `body` with the A1 ABox pre-check DISABLED, so the
/// verdict comes from the tableau/wedge calculus alone.
fn consistent_engine_only(body: &str) -> bool {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _abox = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent")
}

// ─── POSITIVE tests: must be INCONSISTENT ─────────────────────────────

/// tinv: domain on a syntactic `ObjectInverseOf(p)` — INCONSISTENT.
/// `Domain(p⁻, C)` means every domain of a `p⁻`-edge (= target of `p`) is `C`.
/// `p(a,b)` makes `b` a domain node; `b:D` + `Disjoint(C,D)` ⇒ clash.
#[test]
fn inverse_domain_syntactic_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(ObjectInverseOf(:p) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

/// trinv: range on `ObjectInverseOf(p)` — INCONSISTENT.
/// `Range(p⁻, C)` means every range of a `p⁻`-edge (= source of `p`) is `C`.
/// `p(a,b)` makes `a` a range node; `a:D` + `Disjoint(C,D)` ⇒ clash.
#[test]
fn inverse_range_syntactic_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyRange(ObjectInverseOf(:p) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// H4: declared `InverseObjectProperties(p,q)` + `domain(q,C)` — INCONSISTENT.
/// `q = p⁻`; `p(a,b)` makes `b` a `q`-source; `b:D` + `Disjoint(C,D)` ⇒ clash.
#[test]
fn inverse_domain_declared_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    InverseObjectProperties(:p :q)
    ObjectPropertyDomain(:q :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

// ─── NEGATIVE control: must stay CONSISTENT (FP guard) ────────────────

/// Unrelated role `r` has the domain constraint; the edge is on `p` with no
/// domain axiom — no clash possible. Must stay CONSISTENT.
#[test]
fn unrelated_role_domain_stays_consistent() {
    assert!(consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

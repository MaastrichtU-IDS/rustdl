//! #353 — a clause body over the wedge's variable cap is split into named branches.
//!
//! The wedge's matcher refuses a body with more than `MAX_BODY_VARS` (8) variables,
//! and a refused clause never fires. So `(∃s₁.S₁ ⊓ … ⊓ ∃s₈.S₈ ⊓ ≥2 q.J) ⊑ G` was
//! never enforced: the wedge's `Sat` was trusted and `classify` missed `A ⊑ G`
//! with `incomplete: false` (`ore_ont_16461`, `Patient1 ⊑ Systemic_JIA_Patient`).
//! The clausifier now names each branch `sᵢ(X,y) ∧ Sᵢ(y) → Eᵢ(X)` and rewrites the
//! clause over `Eᵢ(X)` (`RUSTDL_WEDGE_BODY_DECOMPOSE`, default ON). What it cannot
//! split is counted in `ClassificationStats::wedge_clauses_refused`.
//!
//! ORACLE: `HermiT` 1.4.5 (`y : A ⊓ ¬G` inconsistent or not) and Konclude agree on
//! every verdict below.
//!
//! Every test locks the env mutex, since some set flags.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

const A: &str = "http://ex.org/A";
const G: &str = "http://ex.org/G";

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for one test and restores it on drop (the `adaptive_budget.rs`
/// pattern); callers hold [`lock`].
struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialised via ENV_MUTEX, restored on Drop.
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

fn classify(body: &str) -> Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A)) Declaration(Class(:G))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    classify_top_down_with_timeout(&onto, Duration::from_secs(30)).expect("classify")
}

/// `A` has two `q`-successors in disjoint subclasses of `J` (so `≥2 q.J`, which
/// the EL saturator cannot derive) and one `sᵢ`-successor for `i` in `has`; the
/// sufficient condition for `G` asks for all `n` existentials plus `≥2 q.J`.
fn star(n: u32, has: impl Fn(u32) -> bool) -> String {
    let mut s = String::from(
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n",
    );
    let mut lhs = String::new();
    for i in 1..=n {
        if has(i) {
            writeln!(s, "SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))").unwrap();
        }
        write!(lhs, " ObjectSomeValuesFrom(:s{i} :S{i})").unwrap();
    }
    writeln!(
        s,
        "SubClassOf(ObjectIntersectionOf({lhs} ObjectMinCardinality(2 :q :J)) :G)"
    )
    .unwrap();
    s
}

/// The issue's shape: 8 and 10 existentials (9 and 11 variables) are derived now;
/// 7 (8 variables, under the cap) always was. Nothing is refused.
#[test]
fn an_over_cap_star_body_is_split_and_fires() {
    let _g = lock();
    for n in [7, 8, 10] {
        let c = classify(&star(n, |_| true));
        assert!(c.is_subclass(A, G), "n = {n}");
        assert_eq!(c.stats().wedge_clauses_refused, 0, "n = {n}");
        assert_eq!(c.stats().timed_out_pairs, 0, "n = {n}");
    }
}

/// The split must not invent the subsumption: with one branch's existential
/// missing, `A ⊑ G` is not entailed.
#[test]
fn a_missing_branch_still_blocks_the_subsumption() {
    let _g = lock();
    let c = classify(&star(8, |i| i != 8));
    assert!(!c.is_subclass(A, G));
    assert_eq!(c.stats().wedge_clauses_refused, 0);
}

/// A branch's name must depend on its filler, not just its role. Two sufficient
/// conditions differ only in the 8th branch's filler (`S8` for `G`, `T` for `H`),
/// and `A`'s `s8`-successor is a `T`: `A ⊑ H` holds and `A ⊑ G` must not.
#[test]
fn a_branch_name_depends_on_its_filler() {
    let _g = lock();
    let mut s = String::from(
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s8 :T))\n",
    );
    let mut common = String::new();
    for i in 1..=7 {
        writeln!(s, "SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))").unwrap();
        write!(common, " ObjectSomeValuesFrom(:s{i} :S{i})").unwrap();
    }
    for (filler, head) in [("S8", "G"), ("T", "H")] {
        writeln!(
            s,
            "SubClassOf(ObjectIntersectionOf({common} ObjectSomeValuesFrom(:s8 :{filler}) \
             ObjectMinCardinality(2 :q :J)) :{head})"
        )
        .unwrap();
    }
    let c = classify(&s);
    assert!(c.is_subclass(A, "http://ex.org/H"));
    assert!(!c.is_subclass(A, G));
}

/// A branch binding a head variable stays inline: the `t`-edge of
/// `(∃×8) ⊑ ∀t.B` (and of `∀t.∃u.H`) is what the head's successor variable is
/// bound by. The control puts the `∀` on a role `A` has no successor for.
#[test]
fn a_branch_the_head_needs_stays_inline() {
    let _g = lock();
    let base = |head: &str| {
        let mut s = String::from("SubClassOf(:A ObjectSomeValuesFrom(:t :C))\n");
        let mut lhs = String::new();
        for i in 1..=8 {
            writeln!(s, "SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))").unwrap();
            write!(lhs, " ObjectSomeValuesFrom(:s{i} :S{i})").unwrap();
        }
        writeln!(s, "SubClassOf(ObjectIntersectionOf({lhs}) {head})").unwrap();
        s
    };
    let forall = base("ObjectAllValuesFrom(:t :B)")
        + "SubClassOf(ObjectSomeValuesFrom(:t ObjectIntersectionOf(:C :B)) :G)";
    assert!(classify(&forall).is_subclass(A, G));
    let nested = base("ObjectAllValuesFrom(:t ObjectSomeValuesFrom(:u :H))")
        + "SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :H)) :G)";
    assert!(classify(&nested).is_subclass(A, G));
    let control = base("ObjectAllValuesFrom(:u :B)")
        + "SubClassOf(ObjectSomeValuesFrom(:t ObjectIntersectionOf(:C :B)) :G)";
    assert!(!classify(&control).is_subclass(A, G));
}

/// One branch that is itself over the cap (`∃r.(∃s₁.S₁ ⊓ … ⊓ ∃s₉.S₉)`) is split
/// one level down.
#[test]
fn an_over_cap_branch_is_split_recursively() {
    let _g = lock();
    let mut s = String::from(
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n",
    );
    let mut inner = String::new();
    for i in 1..=9 {
        writeln!(s, "SubClassOf(:B ObjectSomeValuesFrom(:s{i} :S{i}))").unwrap();
        write!(inner, " ObjectSomeValuesFrom(:s{i} :S{i})").unwrap();
    }
    writeln!(
        s,
        "SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf({inner})) \
         ObjectMinCardinality(2 :q :J)) :G)"
    )
    .unwrap();
    let c = classify(&s);
    assert!(c.is_subclass(A, G));
    assert_eq!(c.stats().wedge_clauses_refused, 0);
}

/// `RUSTDL_WEDGE_BODY_DECOMPOSE=0` reverts: the body is refused, the pair is
/// missed, and the refusal is now visible in `wedge_clauses_refused` (not in
/// `timed_out_pairs`, which counts deadlines only).
#[test]
fn the_flag_off_reverts_and_the_refusal_is_reported() {
    let _g = lock();
    let _env = SetEnvGuard::set("RUSTDL_WEDGE_BODY_DECOMPOSE", "0");
    let c = classify(&star(8, |_| true));
    assert!(!c.is_subclass(A, G));
    assert_eq!(c.stats().wedge_clauses_refused, 1);
    assert_eq!(c.stats().timed_out_pairs, 0);
}

/// `RUSTDL_REPORT_WEDGE_CLAUSES_REFUSED=0` turns the report off, and nothing else.
#[test]
fn the_report_flag_off_reports_zero() {
    let _g = lock();
    let _env = SetEnvGuard::set("RUSTDL_WEDGE_BODY_DECOMPOSE", "0");
    let _rep = SetEnvGuard::set("RUSTDL_REPORT_WEDGE_CLAUSES_REFUSED", "0");
    let c = classify(&star(8, |_| true));
    assert_eq!(c.stats().wedge_clauses_refused, 0);
    assert!(!c.is_subclass(A, G));
}

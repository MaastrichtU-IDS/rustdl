//! `≤n` merges in the main tableau (#293, #294). Each verdict is HermiT's.
//!
//! #294: `apply_max` counted the `≤n` witnesses when the parent was processed,
//! before the witnesses carried the qualifier label, and nothing re-ran it, so
//! `sat` missed the clash. Fixed by re-saturating once at a `Sat` leaf.
//!
//! #293: once the count is right, the pair to merge is a real choice. Committing
//! to the first pair that does not clash at once read a satisfiable class as
//! unsat when that pair clashed later. Fixed by branching over the pairs.
//!
//! The two are coupled: the #294 fix alone turns `late_clash` into a false unsat.
//! Most tests run with the wedge OFF so the main tableau is the engine answering.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::fmt::Write as _;
use std::io::Cursor;

// Env mutation is process-wide; serialise and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(&'static str, Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: every mutation here happens while ENV_MUTEX is held by the caller.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the lock is still held by the test body.
        unsafe {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:C1)) Declaration(Class(:C2))\n\
         Declaration(Class(:C3)) Declaration(Class(:X)) Declaration(Class(:D1)) Declaration(Class(:D2))\n\
         Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:p))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `sat A` under the given env settings, plus the wedge setting.
fn sat_with(body: &str, wedge: bool, env: &[(&'static str, &str)]) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_HYPERTABLEAU", if wedge { "1" } else { "0" });
    let _guards: Vec<EnvGuard> = env.iter().map(|(k, v)| EnvGuard::set(k, v)).collect();
    owl_dl_reasoner::is_class_satisfiable(&onto(body), "http://t/A").expect("sat")
}

/// `≤1 s.C` forces the two `s.C` witnesses together, and the merged node has
/// `∀p.X` and `∃p.¬X`. HermiT: `A ≡ owl:Nothing`.
const FORCED: &str = "SubClassOf(:A ObjectMaxCardinality(1 :s :C))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C ObjectAllValuesFrom(:p :X))))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C ObjectSomeValuesFrom(:p ObjectComplementOf(:X)))))";

/// `≤2 s.C` over three witnesses. Merging the first two clashes, but only after
/// generation; merging either with the `C3` witness is harmless, which arrives
/// last behind two unfoldings. HermiT: satisfiable.
const LATE_CLASH: &str = "SubClassOf(:C1 :C) SubClassOf(:C2 :C) SubClassOf(:C3 :C)\n\
     SubClassOf(:A ObjectMaxCardinality(2 :s :C))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C1 ObjectAllValuesFrom(:p :X))))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C2 ObjectSomeValuesFrom(:p ObjectComplementOf(:X)))))\n\
     SubClassOf(:A :D1) SubClassOf(:D1 :D2) SubClassOf(:D2 ObjectSomeValuesFrom(:s :C3))";

/// `LATE_CLASH` with the harmless witness gone: every merge clashes. Unsat.
const LATE_CLASH_NO_ESCAPE: &str = "SubClassOf(:C1 :C) SubClassOf(:C2 :C)\n\
     SubClassOf(:A ObjectMaxCardinality(1 :s :C))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C1 ObjectAllValuesFrom(:p :X))))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C2 ObjectSomeValuesFrom(:p ObjectComplementOf(:X)))))";

#[test]
fn a_forced_merge_that_clashes_is_unsat() {
    assert!(!sat_with(FORCED, false, &[]));
    assert!(!sat_with(FORCED, true, &[]));
}

#[test]
fn a_forced_merge_clash_through_told_subsumers_is_unsat() {
    assert!(!sat_with(LATE_CLASH_NO_ESCAPE, false, &[]));
}

#[test]
fn a_late_clashing_pair_does_not_hide_a_harmless_one() {
    assert!(sat_with(LATE_CLASH, false, &[]));
    assert!(sat_with(LATE_CLASH, true, &[]));
}

/// The re-saturation is what finds the forced clash (#294).
#[test]
fn without_the_sat_leaf_resaturation_the_forced_clash_is_missed() {
    assert!(sat_with(
        FORCED,
        false,
        &[("RUSTDL_SAT_LEAF_RESATURATE", "0")]
    ));
}

/// The choice point is what keeps `LATE_CLASH` satisfiable once the merge fires
/// (#293): committing to the first pair reads it unsat.
#[test]
fn without_the_choice_point_the_late_clash_reads_unsat() {
    assert!(!sat_with(
        LATE_CLASH,
        false,
        &[("RUSTDL_MAX_CHOICE_POINT", "0")]
    ));
}

// ── Review cases (#299) ───────────────────────────────────────────────────────

/// `sat`-style check on an ontology with its own declarations.
fn sat_raw(src: &str, class: &str, wedge: bool) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_HYPERTABLEAU", if wedge { "1" } else { "0" });
    let mut reader = Cursor::new(src);
    let onto: SetOntology<RcStr> = read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0;
    owl_dl_reasoner::is_class_satisfiable(&onto, class).expect("sat")
}

fn subclass_raw(src: &str, sub: &str, sup: &str, wedge: bool) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_HYPERTABLEAU", if wedge { "1" } else { "0" });
    let mut reader = Cursor::new(src);
    let onto: SetOntology<RcStr> = read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0;
    owl_dl_reasoner::is_subclass_of(&onto, sub, sup).expect("subclass")
}

/// Three nominal witnesses under `≤2 r.C`; `ib` exists only under the `B1`
/// disjunct and is distinct from `ic` and `id`, so under `B1` the only merge is
/// `ic`/`id`, which clashes one step later. `B2` avoids it, so `A` is
/// satisfiable and `A ⋢ X` (HermiT). The forced merge's deps used to omit the
/// `B1` decision, so the clash back-jumped past `B2` and read `A` unsat.
const FORCED_UNDER_DISJUNCT: &str = "Prefix(:=<http://t/>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:B1)) Declaration(Class(:B2))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:p))
Declaration(NamedIndividual(:ib)) Declaration(NamedIndividual(:ic)) Declaration(NamedIndividual(:id))
SubClassOf(:A ObjectMaxCardinality(2 :r :C))
SubClassOf(:A ObjectHasValue(:r :ic))
SubClassOf(:A ObjectHasValue(:r :id))
SubClassOf(:A ObjectUnionOf(:B1 :B2))
SubClassOf(:B1 ObjectHasValue(:r :ib))
ClassAssertion(:C :ib) ClassAssertion(:C :ic) ClassAssertion(:C :id)
ClassAssertion(ObjectAllValuesFrom(:p :X) :ic) ClassAssertion(ObjectSomeValuesFrom(:p ObjectComplementOf(:X)) :id)
DifferentIndividuals(:ib :ic) DifferentIndividuals(:ib :id)
)";

/// Control: `ib` unconditional, so the clashing merge is forced. Unsat (HermiT).
const FORCED_UNCONDITIONAL: &str = "Prefix(:=<http://t/>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:X))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:p))
Declaration(NamedIndividual(:ib)) Declaration(NamedIndividual(:ic)) Declaration(NamedIndividual(:id))
SubClassOf(:A ObjectMaxCardinality(2 :r :C))
SubClassOf(:A ObjectHasValue(:r :ic))
SubClassOf(:A ObjectHasValue(:r :id))
SubClassOf(:A ObjectHasValue(:r :ib))
ClassAssertion(:C :ib) ClassAssertion(:C :ic) ClassAssertion(:C :id)
ClassAssertion(ObjectAllValuesFrom(:p :X) :ic) ClassAssertion(ObjectSomeValuesFrom(:p ObjectComplementOf(:X)) :id)
DifferentIndividuals(:ib :ic) DifferentIndividuals(:ib :id)
)";

#[test]
fn a_forced_merge_under_a_disjunct_does_not_hide_the_other_disjunct() {
    for wedge in [false, true] {
        assert!(
            sat_raw(FORCED_UNDER_DISJUNCT, "http://t/A", wedge),
            "wedge={wedge}"
        );
        assert!(
            !subclass_raw(FORCED_UNDER_DISJUNCT, "http://t/A", "http://t/X", wedge),
            "wedge={wedge}: A ⊑ X is not entailed"
        );
        assert!(
            !sat_raw(FORCED_UNCONDITIONAL, "http://t/A", wedge),
            "wedge={wedge}"
        );
    }
}

/// `U ⊑ ≤1 r.C` over k witnesses `C ⊓ Bi`, plus `∀r.(¬B1 ⊔ … ⊔ ¬Bk)`: unsat
/// (HermiT, k=5). Branching over pairs explored every merge order, ~∏ C(m,2)
/// leaves, and ran past 20 s at k=8; enumerating partitions once is linear here.
#[test]
fn many_witnesses_under_le1_do_not_explode() {
    let k = 10;
    let mut src = String::from(
        "Prefix(:=<http://t/>)\nOntology(\nSubClassOf(:A ObjectMaxCardinality(1 :r :C))\n",
    );
    let mut negs = String::new();
    for i in 1..=k {
        let _ = writeln!(
            src,
            "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :B{i})))"
        );
        let _ = write!(negs, " ObjectComplementOf(:B{i})");
    }
    let _ = write!(
        src,
        "SubClassOf(:U ObjectIntersectionOf(:A ObjectAllValuesFrom(:r ObjectUnionOf({negs}))))\n)"
    );
    let start = std::time::Instant::now();
    assert!(!sat_raw(&src, "http://t/U", false));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(20),
        "took {:?}",
        start.elapsed()
    );
}

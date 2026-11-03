//! #429: classify, realize, justify, repair and diagnose report a consistency
//! that rests on the consistency wedge's `Sat` alone, as
//! `relies_on_trusted_consistency` (the #392 / #401 name). A trust decision, not a
//! give-up: no verdict changes, and `incomplete` stays clear.
//!
//! Each surface is checked three ways on #392's fixture (off pure EL, an `ABox`,
//! no nominal): flagged by default; NOT flagged with `RUSTDL_WEDGE_CONSISTENCY=0`;
//! never flagged beside an inconsistency. With the wedge off, classify's
//! "consistent" rests on no consistency evidence at all (no wedge ran, and the
//! consistency probe is not admitted on this fixture), so the flag is clear
//! because its fact (a wedge `Sat`) is absent, not because a model was verified.
//!
//! Every test locks the env mutex, since some set `RUSTDL_WEDGE_CONSISTENCY`.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, Verdict, find_one_justification_reporting};
use owl_dl_reasoner::{FragmentClassification, ReasonError};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// #392's fixture: off pure EL (the union), no nominal, no data counting; the
/// wedge decides it `Sat`.
const TBOX: &str = "SubClassOf(:A ObjectUnionOf(:C :D)) SubClassOf(:C :B) SubClassOf(:D :B)";
const ABOX: &str = "ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)";
/// Makes the fixture inconsistent: `a : A ⊑ B` and `a : ¬B`.
const CLASH: &str = "ClassAssertion(ObjectComplementOf(:B) :a)";

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex/#>)\n\
         Ontology(<http://ex/trusted429>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

fn fixture() -> SetOntology<RcStr> {
    onto(&format!("{TBOX}\n{ABOX}"))
}

fn inconsistent() -> SetOntology<RcStr> {
    onto(&format!("{TBOX}\n{ABOX}\n{CLASH}"))
}

const A: &str = "http://ex/#A";
const B: &str = "http://ex/#B";
const C: &str = "http://ex/#C";

#[test]
fn classify_reports_a_trusted_wedge_sat() {
    let _g = lock();
    let stats = owl_dl_reasoner::classify(&fixture()).unwrap().stats();
    assert!(!stats.inconsistent);
    assert!(stats.relies_on_trusted_consistency, "{stats:?}");
    assert!(!stats.completeness_guaranteed());
    {
        let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        let stats = owl_dl_reasoner::classify(&fixture()).unwrap().stats();
        assert!(!stats.relies_on_trusted_consistency, "wedge off: {stats:?}");
    }
    let stats = owl_dl_reasoner::classify(&inconsistent()).unwrap().stats();
    assert!(stats.inconsistent);
    assert!(!stats.relies_on_trusted_consistency, "{stats:?}");
}

/// The runs on one thread do not leak the flag into each other: a trusted run
/// followed by an `ABox`-free one (no wedge) reads clear.
#[test]
fn the_flag_does_not_carry_over_between_runs() {
    let _g = lock();
    assert!(
        owl_dl_reasoner::classify(&fixture())
            .unwrap()
            .stats()
            .relies_on_trusted_consistency
    );
    assert!(
        !owl_dl_reasoner::classify(&onto(TBOX))
            .unwrap()
            .stats()
            .relies_on_trusted_consistency
    );
}

/// The manager's question on #429: `completeness_guaranteed` and the flag cannot
/// both hold. A `PureEl` ontology names no individual (the gate admits no `ABox`
/// assertion and no nominal), so the consistency wedge never runs on it; an EL
/// `TBox` with an `ABox` is not `PureEl`. And the predicate requires the flag
/// clear anyway (`completeness_contract.rs`).
#[test]
fn pure_el_and_trust_never_meet() {
    let _g = lock();
    let pure = owl_dl_reasoner::classify(&onto("SubClassOf(:C :B) SubClassOf(:A :C)"))
        .unwrap()
        .stats();
    assert_eq!(pure.fragment, FragmentClassification::PureEl);
    assert!(pure.completeness_guaranteed());
    assert!(!pure.relies_on_trusted_consistency);

    let el_with_abox = owl_dl_reasoner::classify(&onto(&format!(
        "SubClassOf(:C :B) SubClassOf(:A :C)\n{ABOX}"
    )))
    .unwrap()
    .stats();
    assert_ne!(el_with_abox.fragment, FragmentClassification::PureEl);
    assert!(
        el_with_abox.relies_on_trusted_consistency,
        "{el_with_abox:?}"
    );
    assert!(!el_with_abox.completeness_guaranteed());
}

#[test]
fn realize_carries_the_classification_flag() {
    let _g = lock();
    let r = owl_dl_reasoner::realize(&fixture()).unwrap();
    assert!(r.relies_on_trusted_consistency());
    assert!(!r.incomplete(), "a trust decision, not a give-up");
    {
        let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        assert!(
            !owl_dl_reasoner::realize(&fixture())
                .unwrap()
                .relies_on_trusted_consistency()
        );
    }
    // The saturation path classifies without a wedge.
    assert!(
        !owl_dl_reasoner::realize_saturation_only(&fixture())
            .unwrap()
            .relies_on_trusted_consistency()
    );
    assert!(matches!(
        owl_dl_reasoner::realize(&inconsistent()),
        Err(ReasonError::Inconsistent)
    ));
}

#[test]
fn justify_reports_a_trusted_not_entailed() {
    let _g = lock();
    // `A ⊑ C` does not hold (`A ⊑ C ⊔ D` only): the check runs without the ABox on
    // the strength of a trusted consistency (#401), so "not entailed" rests on it.
    let not_entailed = Entailment::SubClassOf {
        sub: A.to_owned(),
        sup: C.to_owned(),
    };
    let out = find_one_justification_reporting(&fixture(), &not_entailed).unwrap();
    assert_eq!(out.status, Verdict::NotEntailed);
    assert!(out.relies_on_trusted_consistency);
    assert!(!out.incomplete);
    // `inconsistent` on a consistent KB: "not entailed" IS the wedge's `Sat`.
    let out = find_one_justification_reporting(&fixture(), &Entailment::Inconsistent).unwrap();
    assert_eq!(out.status, Verdict::NotEntailed);
    assert!(out.relies_on_trusted_consistency);
    {
        let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        let out = find_one_justification_reporting(&fixture(), &not_entailed).unwrap();
        assert_eq!(out.status, Verdict::NotEntailed);
        assert!(!out.relies_on_trusted_consistency, "wedge off");
        let out = find_one_justification_reporting(&fixture(), &Entailment::Inconsistent).unwrap();
        assert!(!out.relies_on_trusted_consistency, "wedge off");
    }
    // An inconsistent KB: `inconsistent` is entailed, and its justification found
    // without trusting anything at the top level.
    let out = find_one_justification_reporting(&inconsistent(), &Entailment::Inconsistent).unwrap();
    assert_eq!(out.status, Verdict::Entailed);
}

/// Control: an entailed justification over an `ABox`-free `TBox` (nothing lent,
/// no wedge) is not flagged. That #401's lent trust on an ENTAILED check never
/// marks is pinned by the unit test `justify::oracle_trust_tests::
/// only_a_not_entailed_verdict_marks`, which a mark-on-entailed sabotage turns red.
#[test]
fn an_entailed_check_alone_never_marks() {
    let _g = lock();
    let entailed = Entailment::SubClassOf {
        sub: A.to_owned(),
        sup: B.to_owned(),
    };
    assert_eq!(
        owl_dl_reasoner::justify::entails_reporting(&fixture(), &entailed).unwrap(),
        Verdict::Entailed
    );
    let out = find_one_justification_reporting(&onto(TBOX), &entailed).unwrap();
    assert_eq!(out.status, Verdict::Entailed);
    assert!(!out.relies_on_trusted_consistency);
}

#[test]
fn repair_reports_a_trusted_not_entailed() {
    let _g = lock();
    let not_entailed = Entailment::SubClassOf {
        sub: A.to_owned(),
        sup: C.to_owned(),
    };
    let r = owl_dl_reasoner::find_repairs(&fixture(), &not_entailed, 5).unwrap();
    assert!(!r.entailed);
    assert!(r.relies_on_trusted_consistency);
    let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let r = owl_dl_reasoner::find_repairs(&fixture(), &not_entailed, 5).unwrap();
    assert!(!r.relies_on_trusted_consistency, "wedge off");
}

#[test]
fn diagnose_reports_the_classification_flag() {
    let _g = lock();
    let d = owl_dl_reasoner::diagnose(&fixture()).unwrap();
    assert!(d.consistent);
    assert!(d.relies_on_trusted_consistency);
    assert!(!d.incomplete);
    {
        let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        assert!(
            !owl_dl_reasoner::diagnose(&fixture())
                .unwrap()
                .relies_on_trusted_consistency
        );
    }
    let d = owl_dl_reasoner::diagnose(&inconsistent()).unwrap();
    assert!(!d.consistent);
    assert!(!d.relies_on_trusted_consistency);
}

/// Diagnose's all-classes-unsatisfiable gate is the authority when it runs and
/// does not give up: the flag is what that consistency check rests on, whatever
/// the classification rested on. Every class here is unsatisfiable (`⊑ ⊥`) and
/// the `ABox` names individuals, so the gate runs.
#[test]
fn diagnose_gate_decides_the_flag() {
    let _g = lock();
    let body = "SubClassOf(:A owl:Nothing) SubClassOf(:B owl:Nothing) SubClassOf(:C owl:Nothing) \
                SubClassOf(:D owl:Nothing) ObjectPropertyAssertion(:r :a :b)";
    let ofn = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/gate429>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n{body}\n)\n"
    );
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default())
        .expect("parse")
        .0;
    let d = owl_dl_reasoner::diagnose(&o).unwrap();
    assert!(d.consistent, "{d:?}");
    assert_eq!(
        d.all_unsat.len(),
        4,
        "the gate ran: every class is unsatisfiable"
    );
    let (_, stats) = owl_dl_reasoner::is_consistent_with_stats(&o).unwrap();
    assert!(!stats.incomplete);
    assert_eq!(
        d.relies_on_trusted_consistency, stats.relies_on_trusted_consistency,
        "the gate's check decides the flag when it did not give up"
    );
}

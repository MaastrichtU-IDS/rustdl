//! Issue #70 — `ObjectHasValue` on the LEFT of an equivalence made a two-axiom
//! ontology run for hours.
//!
//! `EquivalentClasses(Q, ObjectHasValue(p, b))` yields the `⊒` half
//! `∃p.{b} ⊑ Q`, whose existential antecedent absorption could not take. It
//! stayed the residual GCI `⊤ ⊑ ∀p.¬{b} ⊔ Q`, applied at EVERY node — and its
//! `Q` disjunct is *generating*: picking it forces `∃p.{b}`, whose fresh witness
//! is itself nominal, gets the same residual, picks `Q`, and generates again.
//! On the deadline-free query paths (`is_class_satisfiable`, `is_consistent`,
//! un-timed `realize`) the main tableau runs anywhere-blocking, and
//! `is_blocked_anywhere` refuses to block a nominal node, so nothing cut the
//! cycle. Measured on the pre-fix binary: 0 blocks in 127,708 `is_blocked`
//! calls, and wall growing ~cubically in the node count toward the default
//! `RUSTDL_MAX_NODES=50000` cap.
//!
//! `RUSTDL_NOMINAL_EXISTS_ABSORPTION` (default ON) rewrites the antecedent to
//! the equivalent `{b} ⊑ ∀p⁻.Q`, a `NominalRule`, so the residual — and with it
//! the branch point — never exists.
//!
//! **These canaries are the entire safety net for the pass**: it fires on 0 of
//! the 7 curated fixtures, so a green corpus closure-diff there demonstrates
//! inertness, not correctness. The FP guards are the two negative controls
//! below, which pin that the rewrite adds no membership that is not entailed.
#![allow(clippy::unwrap_used)]

use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{class_expression_instances, instances_of, is_class_satisfiable};
use std::io::Cursor;

/// Generous relative to the ~0.00 s the fixed path takes, tight relative to the
/// hours the pre-fix path needed. A regression cannot creep under this.
const BOUND: Duration = Duration::from_secs(60);

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

/// Run `f` on its own thread and FAIL rather than hang if it outlives `BOUND`.
/// A plain call would turn a regression into a CI timeout with no attribution;
/// the whole point of this file is that non-termination is an assertable
/// property. The worker thread is left to run — the process is exiting anyway.
fn within_bound<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(BOUND).unwrap_or_else(|e| {
        panic!("{what} did not finish within {BOUND:?} ({e}) — issue #70 has regressed")
    })
}

/// The two-axiom reproducer, verbatim from the report.
const REPRO: &str = r"Ontology(<http://example.org/o>
    Declaration(ObjectProperty(<http://example.org/p>))
    ObjectPropertyAssertion(<http://example.org/p> <http://example.org/a> <http://example.org/b>)
)";

/// The same ontology with the probe axiom `class_expression_instances` would
/// mint written out by hand, so the calculus is exercised without going through
/// the class-expression front end.
const PROBED: &str = r"Ontology(<http://example.org/o>
    Declaration(ObjectProperty(<http://example.org/p>))
    ObjectPropertyAssertion(<http://example.org/p> <http://example.org/a> <http://example.org/b>)
    EquivalentClasses(<http://example.org/Q> ObjectHasValue(<http://example.org/p> <http://example.org/b>))
)";

/// As reported: `instances-expr FILE '(p value b)'`. `a` is asserted directly.
#[test]
fn issue70_object_has_value_instances_terminates() {
    let got = within_bound("class_expression_instances(∃p.{b})", || {
        let o = onto(REPRO);
        let b = Build::<RcStr>::new();
        let ce = ClassExpression::ObjectHasValue {
            ope: b.object_property("http://example.org/p").into(),
            i: b.named_individual("http://example.org/b").into(),
        };
        class_expression_instances(&o, &ce).unwrap()
    });
    assert_eq!(got.individuals(), ["http://example.org/a".to_owned()]);
}

/// The narrower defect underneath: `is_class_satisfiable` was the call that
/// spun. It is the *completeness-signal* probe in `class_expression_instances`,
/// so the query's own answer (`instances_of`, below) was already instant while
/// the diagnostic beside it never returned.
#[test]
fn issue70_named_probe_class_is_satisfiable() {
    let sat = within_bound("is_class_satisfiable(Q)", || {
        let o = onto(PROBED);
        is_class_satisfiable(&o, "http://example.org/Q").unwrap()
    });
    assert!(sat, "Q ≡ ∃p.{{b}} is satisfiable");
}

/// POSITIVE: the rewrite must still derive what the axiom says. `p(a,b)` puts
/// `a` in `∃p.{b}`, hence in `Q`.
#[test]
fn absorbed_rule_still_derives_the_membership() {
    let got = within_bound("instances_of(Q)", || {
        let o = onto(PROBED);
        instances_of(&o, "http://example.org/Q").unwrap()
    });
    assert_eq!(got, ["http://example.org/a".to_owned()]);
}

/// NEGATIVE CONTROL (FP guard): the same shape with the edge pointing at a
/// DIFFERENT individual entails nothing. If the rewrite ever keyed on the role
/// alone — dropping the individual the `NominalRule` exists to carry — this is
/// the test that catches it.
#[test]
fn wrong_target_individual_yields_no_membership() {
    const OTHER: &str = r"Ontology(<http://example.org/o>
    Declaration(ObjectProperty(<http://example.org/p>))
    ObjectPropertyAssertion(<http://example.org/p> <http://example.org/a> <http://example.org/c>)
    EquivalentClasses(<http://example.org/Q> ObjectHasValue(<http://example.org/p> <http://example.org/b>))
)";
    let got = within_bound("instances_of(Q) [wrong target]", || {
        let o = onto(OTHER);
        instances_of(&o, "http://example.org/Q").unwrap()
    });
    assert!(got.is_empty(), "unentailed membership derived: {got:?}");
}

/// NEGATIVE CONTROL (FP guard): the propagation direction must be the
/// R-PREDECESSOR, which is what `role.flip()` encodes. Here the edge runs
/// `b —p→ a`, so `b`, not `a`, is the one in `∃p.{a}`. A rewrite that forgot
/// the flip would put `a` in `Q` here.
#[test]
fn propagation_runs_to_the_predecessor_not_the_successor() {
    const REVERSED: &str = r"Ontology(<http://example.org/o>
    Declaration(ObjectProperty(<http://example.org/p>))
    ObjectPropertyAssertion(<http://example.org/p> <http://example.org/b> <http://example.org/a>)
    EquivalentClasses(<http://example.org/Q> ObjectHasValue(<http://example.org/p> <http://example.org/a>))
)";
    let got = within_bound("instances_of(Q) [reversed edge]", || {
        let o = onto(REVERSED);
        instances_of(&o, "http://example.org/Q").unwrap()
    });
    assert_eq!(got, ["http://example.org/b".to_owned()]);
}

/// A residual-free nominal `∃`-cycle: `Q ⊑ ∃p.{b}` generates a witness, the
/// o-rule merges it into `{b}`'s node, and `{b} ⊑ Q` puts `Q` back. This ontology
/// has **zero residual GCIs**, so this file's absorption pass cannot reach it.
///
/// It was an `#[ignore]`d sentinel recording "anywhere-blocking never blocks a
/// nominal node, so the graph grows to `RUSTDL_MAX_NODES`". That diagnosis was
/// wrong. The growth came from the saturator firing rules on the merged-away
/// witness: its `∃p.{b}` looked unsatisfied once its edges had moved, so it
/// generated again. #284 skips merged-away nodes, and this now answers `sat`
/// with no blocking change.
#[test]
fn residual_free_nominal_cycle_terminates() {
    const CYCLE: &str = r"Ontology(<http://example.org/o>
    Declaration(ObjectProperty(<http://example.org/p>))
    Declaration(Class(<http://example.org/Q>))
    ObjectPropertyAssertion(<http://example.org/p> <http://example.org/a> <http://example.org/b>)
    SubClassOf(<http://example.org/Q> ObjectHasValue(<http://example.org/p> <http://example.org/b>))
    SubClassOf(ObjectOneOf(<http://example.org/b>) <http://example.org/Q>)
)";
    let sat = within_bound("is_class_satisfiable(Q) [nominal cycle]", || {
        let o = onto(CYCLE);
        is_class_satisfiable(&o, "http://example.org/Q").unwrap()
    });
    assert!(sat);
}

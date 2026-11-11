//! #498: per-query false `unsat` / false subsumption from `apply_nominal_assignment`'s
//! merge deps, with #362's nominal block guard on (since #378).
//!
//! The rule loops over a node's `Nominal` labels and merges the node with each
//! individual's existing node. Two dep sets were understated:
//! - **A1 (`c5e.ofn`).** After a *refused* merge (the pair is marked distinct), the
//!   loop moved to the other individual's node as if it had merged. The next nominal
//!   then merged that wrong node, under that label's deps alone.
//! - **A2 (`q5.ofn`).** The label deps were a snapshot taken before the loop. An
//!   earlier, *accepted* merge in the same loop had moved the label and added its own
//!   deps, so the next merge rested on fewer decisions than it did.
//!
//! Either way a clash back-jumped past a decision whose other option is a model. Both
//! fixtures are consistent, with `E` satisfiable and under `owl:Thing` only (`HermiT` and
//! Konclude); a model is `q ∈ E`, `p –r→ b`, `b = a`, `a ∈ Z`, `m ∈ Nz`. `classify`
//! and default `consistent` were right with the wedge on, because the wedge or a
//! per-pair budget decides first; the per-query commands, `consistent` with the
//! consistency wedge off, and `classify` with the wedge off (the whole ontology
//! inconsistent) were not. `min9.ofn` (wine reduced to 9 axioms, #490) is consistent and pins the
//! same loop for #490's precise refused-merge `⊥`.
//!
//! These fixtures depend on label and node order (which `{…}` the loop visits first,
//! which clash is found first), so the unit tests in `owl-dl-tableau`
//! (`nominal_merge_reads_label_deps_after_an_earlier_merge`,
//! `refused_nominal_merge_leaves_the_node_in_place`,
//! `nominal_merge_without_the_label_takes_every_active_decision`) are the primary
//! guards (four: the three above and
//! `nominal_assignment_on_a_merged_away_node_reads_the_representative`). Each case runs with the wedge on and off and with the guard on and off (the
//! guard-off arms were right before the fix and must stay right).
//!
//! The same merges fed two more surfaces (found by the assurance census, both
//! oracles): `individuals` reported non-entailed different pairs (five on c5e,
//! seven on q5; main also put all five in one same-group), and `sat-expr "{m} and {p}"` (and
//! `m`/`q`, `p`/`q`) said unsatisfiable. Only `a≠m` and `b≠m` are entailed, and
//! `a = b` is (`p` holds `∀r.{a}` and `∃r.{b}`).

use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, Individual, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    class_expression_satisfiable, classify_top_down_with_timeout, different_individuals,
    is_class_satisfiable, is_consistent, is_subclass_of, same_individuals,
};

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for its lifetime; callers hold `lock()`.
struct EnvGuard(&'static str);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(k: &'static str, v: &str) -> Self {
        // SAFETY: every env access in this binary holds `lock()`.
        unsafe { std::env::set_var(k, v) };
        Self(k)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe { std::env::remove_var(self.0) };
    }
}

const C5E: &str = include_str!("fixtures/issue498/c5e.ofn");
const Q5: &str = include_str!("fixtures/issue498/q5.ofn");
const MIN9: &str = include_str!("fixtures/issue498/min9.ofn");

const WEDGE: &str = "RUSTDL_HYPERTABLEAU";
const GUARD: &str = "RUSTDL_NOMINAL_BLOCK_GUARD";
const WEDGE_CONSISTENCY: &str = "RUSTDL_WEDGE_CONSISTENCY";

fn parse(src: &str) -> SetOntology<RcStr> {
    let (onto, _) = read_ofn(
        &mut std::io::Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    onto
}

/// Runs `f` on a worker thread and waits at most `secs`. A query that does not
/// return in time fails the test (its thread is left running until exit).
fn within<T: Send + 'static>(secs: u64, what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(secs))
        .unwrap_or_else(|_| panic!("{what}: no answer within {secs} s"))
}

/// Wedge on and off, guard on (the default) and off.
const ARMS: &[(&str, &str)] = &[("1", "1"), ("0", "1"), ("1", "0"), ("0", "0")];

fn each_arm(mut f: impl FnMut(&str)) {
    for &(wedge, guard) in ARMS {
        let _l = lock();
        let _w = EnvGuard::set(WEDGE, wedge);
        let _g = EnvGuard::set(GUARD, guard);
        f(&format!("{WEDGE}={wedge} {GUARD}={guard}"));
    }
}

#[test]
fn e_is_satisfiable_and_not_under_zb() {
    for (name, src, ns) in [
        ("c5e", C5E, "http://example.org/c5e#"),
        ("q5", Q5, "http://example.org/q5#"),
    ] {
        each_arm(|arm| {
            let (e, zb) = (format!("{ns}E"), format!("{ns}Zb"));
            let e2 = e.clone();
            assert!(
                within(60, &format!("{name} sat E ({arm})"), move || {
                    is_class_satisfiable(&parse(src), &e2).expect("decides")
                }),
                "{name}: E is satisfiable ({arm})"
            );
            assert!(
                !within(60, &format!("{name} E ⊑ Zb ({arm})"), move || {
                    is_subclass_of(&parse(src), &e, &zb).expect("decides")
                }),
                "{name}: E ⊑ Zb does not hold ({arm})"
            );
        });
    }
}

#[test]
fn consistent_with_the_consistency_wedge_off() {
    for (name, src) in [("c5e", C5E), ("q5", Q5), ("min9", MIN9)] {
        each_arm(|arm| {
            for wc in ["1", "0"] {
                let _c = EnvGuard::set(WEDGE_CONSISTENCY, wc);
                assert!(
                    within(60, &format!("{name} consistent ({arm})"), move || {
                        is_consistent(&parse(src)).expect("decides")
                    }),
                    "{name} is consistent ({arm} {WEDGE_CONSISTENCY}={wc})"
                );
            }
        });
    }
}

const PAIRS_THAT_MAY_MEET: &[(&str, &str)] = &[("m", "p"), ("m", "q"), ("p", "q")];

#[test]
fn nominal_conjunctions_that_have_a_model_are_satisfiable() {
    for (name, src, ns) in [
        ("c5e", C5E, "http://example.org/c5e#"),
        ("q5", Q5, "http://example.org/q5#"),
    ] {
        each_arm(|arm| {
            for &(x, y) in PAIRS_THAT_MAY_MEET {
                let (x_iri, y_iri) = (format!("{ns}{x}"), format!("{ns}{y}"));
                assert!(
                    within(
                        60,
                        &format!("{name} {{{x}}} ⊓ {{{y}}} ({arm})"),
                        move || {
                            let b = Build::<RcStr>::new();
                            let one = |i: &str| {
                                ClassExpression::ObjectOneOf(vec![Individual::Named(
                                    b.named_individual(i),
                                )])
                            };
                            let ce = ClassExpression::ObjectIntersectionOf(vec![
                                one(&x_iri),
                                one(&y_iri),
                            ]);
                            class_expression_satisfiable(&parse(src), &ce)
                                .expect("decides")
                                .holds()
                        }
                    ),
                    "{name}: {{{x}}} ⊓ {{{y}}} is satisfiable ({arm})"
                );
            }
        });
    }
}

#[test]
fn individuals_reports_only_entailed_identities() {
    for (name, src, ns) in [
        ("c5e", C5E, "http://example.org/c5e#"),
        ("q5", Q5, "http://example.org/q5#"),
    ] {
        each_arm(|arm| {
            let short = |iri: &str| iri.trim_start_matches(ns).to_owned();
            let (same, different) = within(60, &format!("{name} individuals ({arm})"), move || {
                let o = parse(src);
                let same = same_individuals(&o, None).expect("decides");
                let different = different_individuals(&o, None).expect("decides");
                (same.groups().to_vec(), different.pairs().to_vec())
            });
            for (x, y) in &different {
                let mut pair = [short(x), short(y)];
                pair.sort();
                assert!(
                    matches!((pair[0].as_str(), pair[1].as_str()), ("a" | "b", "m")),
                    "{name}: {} ≠ {} is not entailed ({arm})",
                    pair[0],
                    pair[1]
                );
            }
            for group in &same {
                assert!(
                    group.iter().all(|i| matches!(short(i).as_str(), "a" | "b")),
                    "{name}: same-group {group:?} is not entailed; only a = b is ({arm})"
                );
            }
        });
    }
}

#[test]
fn classify_finds_every_class_satisfiable() {
    for (name, src) in [("c5e", C5E), ("q5", Q5)] {
        each_arm(|arm| {
            let unsat = within(60, &format!("{name} classify ({arm})"), move || {
                let c = classify_top_down_with_timeout(&parse(src), Duration::from_secs(5))
                    .expect("classify");
                c.unsatisfiable_classes()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });
            assert!(
                unsat.is_empty(),
                "{name}: classify reports {unsat:?} unsatisfiable; every class has a model ({arm})"
            );
        });
    }
}

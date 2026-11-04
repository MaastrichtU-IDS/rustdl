//! A frozen sample of the #363 `el_guaranteed` generated net, checked in CI.
//!
//! The net needs Konclude and `HermiT`, so it never runs in CI; that is how #435 and #436
//! (pure-EL shapes the gate certified and the saturator silently dropped) reached main.
//! `tests/fixtures/el_generated/` freezes 300 of its cases with the answers both peers
//! agreed on: every case the a5a2e063 control run had as a finding (101), plus a
//! motif-stratified sample of the rest. See its `MANIFEST`, and
//! `scripts/gen/export_fixture.py`, which regenerates it.
//!
//! The comparison follows `scripts/gen/run.py::score`, through the `Classification` API
//! (stricter in one place: a certified miss of an oracle inconsistency fails here, where
//! `score` tiers it `pre-check`):
//! - classes: the classification must list exactly the case's declared classes, so a
//!   parse that drops a declaration is caught here, not as a confusing missed pair;
//! - FPs on every case: an unsat class the oracle does not have, an inconsistency the
//!   oracle does not have, or a pair it does not entail;
//! - MISSED only where rustdl claims completeness (`completeness_guaranteed()`): an
//!   oracle unsat class or pair rustdl lacks. That is the net's `finding` tier;
//! - pairs are irreflexive and taken over the declared classes, with the unsat classes
//!   of either side and the oracle's ⊤-equivalent classes excluded, as `score` does.
//!   (`is_subclass` is reflexive and true for an unsat subject, hence both rules.)
//!   rustdl's own ⊤-equivalent classes are not observable through `is_subclass` on a
//!   consistent KB, so that exclusion is one-sided, exactly as in `score`. The frozen
//!   sample has no inconsistent and no ⊤-equivalent case, so those branches are
//!   present for fidelity, not exercised.
//!
//! Layer 2 rides along: on every case the fast path certifies, the EL lowering must drop
//! nothing (`certified_unlowered`; see `el_unlowered_guard.rs` for the rest of the repo's
//! fixtures). That catches a drop whose answer happens not to change on these cases.
//!
//! Vacuity: a regression that de-certifies cases would turn their misses into skips.
//! So each case records whether it was certified on the build the fixture was frozen
//! against, and a case that WAS certified and is not now fails the test. A floor on
//! the certified count backs that up.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::io::Cursor;

const PREFIX: &str = "http://gen.rustdl/o#";
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/el_generated");
/// The sample has 300 cases, all certified on the frozen build; the per-case check
/// is the real guard, this only stops the whole fixture from going quiet.
const CERTIFIED_FLOOR: usize = 250;

struct Expected {
    case: String,
    certified: bool,
    consistent: bool,
    classes: BTreeSet<String>,
    unsat: BTreeSet<String>,
    thing_equiv: BTreeSet<String>,
    pairs: BTreeSet<(String, String)>,
}

fn strings(v: &serde_json::Value) -> BTreeSet<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_str().expect("string").to_owned())
        .collect()
}

fn load_expected() -> Vec<Expected> {
    let text =
        std::fs::read_to_string(format!("{FIXTURE}/expected.jsonl")).expect("expected.jsonl");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).expect("json line");
            Expected {
                case: v["case"].as_str().expect("case").to_owned(),
                certified: v["certified"].as_bool().expect("certified"),
                consistent: v["consistent"].as_bool().expect("consistent"),
                classes: strings(&v["classes"]),
                unsat: strings(&v["unsat"]),
                thing_equiv: strings(&v["thing_equiv"]),
                pairs: v["pairs"]
                    .as_array()
                    .expect("pairs")
                    .iter()
                    .map(|p| {
                        (
                            p[0].as_str().expect("sub").to_owned(),
                            p[1].as_str().expect("sup").to_owned(),
                        )
                    })
                    .collect(),
            }
        })
        .collect()
}

fn local(iri: &str) -> Option<&str> {
    iri.strip_prefix(PREFIX)
}

/// Problems found on one case, and whether it is certified now.
fn check(e: &Expected) -> (Vec<String>, bool) {
    let path = format!("{FIXTURE}/cases/{}.ofn", e.case);
    let src = std::fs::read_to_string(&path).expect("case file");
    let onto: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap_or_else(|err| panic!("{path}: {err:?}"))
        .0;
    let unlowered = owl_dl_reasoner::certified_unlowered(&onto)
        .unwrap_or_else(|err| panic!("{path}: {err}"))
        .classify;
    let c = owl_dl_reasoner::classify(&onto).unwrap_or_else(|err| panic!("{path}: {err}"));
    let stats = c.stats();
    let certified = stats.completeness_guaranteed();
    let mut problems = Vec::new();
    let mut note = |kind: &str, items: Vec<String>| {
        if !items.is_empty() {
            let shown: Vec<&String> = items.iter().take(5).collect();
            problems.push(format!("{kind} ({}): {shown:?}", items.len()));
        }
    };

    let foreign: Vec<String> = c
        .classes()
        .iter()
        .filter(|i| local(i).is_none())
        .map(ToString::to_string)
        .collect();
    note("classes outside the generator namespace", foreign);
    let classes: BTreeSet<String> = c
        .classes()
        .iter()
        .filter_map(|i| local(i))
        .map(str::to_owned)
        .collect();
    if classes != e.classes {
        note(
            "classes differ from the declarations",
            vec![format!("got {classes:?}, declared {:?}", e.classes)],
        );
    }
    note(
        "UNLOWERED under the fast-path certification",
        unlowered
            .iter()
            .map(|u| format!("axiom {} {}: {}", u.axiom, u.site, u.concept))
            .collect(),
    );
    if e.certified && !certified {
        note(
            "DE-CERTIFIED (was completeness_guaranteed on the frozen build)",
            vec![format!("{stats:?}")],
        );
    }
    if !e.consistent {
        if certified && !stats.inconsistent {
            note("consistency-MISSED", vec!["KB".to_owned()]);
        }
        return (problems, certified);
    }
    if stats.inconsistent {
        note("consistency-FP", vec!["KB".to_owned()]);
        return (problems, certified);
    }

    let r_unsat: BTreeSet<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .filter_map(local)
        .map(str::to_owned)
        .collect();
    note("unsat-FP", r_unsat.difference(&e.unsat).cloned().collect());
    if certified {
        note(
            "unsat-MISSED",
            e.unsat.difference(&r_unsat).cloned().collect(),
        );
    }

    let excl: BTreeSet<&String> = e
        .unsat
        .iter()
        .chain(&r_unsat)
        .chain(&e.thing_equiv)
        .collect();
    let mut r_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    for s in &e.classes {
        for t in &e.classes {
            if s != t
                && !excl.contains(s)
                && !excl.contains(t)
                && c.is_subclass(&format!("{PREFIX}{s}"), &format!("{PREFIX}{t}"))
            {
                r_pairs.insert((s.clone(), t.clone()));
            }
        }
    }
    let o_pairs: BTreeSet<(String, String)> = e
        .pairs
        .iter()
        .filter(|(s, t)| !excl.contains(s) && !excl.contains(t))
        .cloned()
        .collect();
    note(
        "pair-FP",
        r_pairs
            .difference(&o_pairs)
            .map(|(s, t)| format!("{s} ⊑ {t}"))
            .collect(),
    );
    if certified {
        note(
            "pair-MISSED",
            o_pairs
                .difference(&r_pairs)
                .map(|(s, t)| format!("{s} ⊑ {t}"))
                .collect(),
        );
    }
    (problems, certified)
}

#[test]
fn every_frozen_el_guaranteed_case_matches_the_oracle() {
    let expected = load_expected();
    assert_eq!(expected.len(), 300, "the MANIFEST promises 300 cases");
    let run = |e: &Expected| {
        let (problems, certified) = check(e);
        (e.case.clone(), problems, certified)
    };
    // The first case runs serially: `classify` installs rayon's global pool with the
    // reasoner's large worker stacks (`ensure_rayon_pool`), which a `par_iter` issued
    // first would pre-empt with the default-stack pool.
    let mut results: Vec<(String, Vec<String>, bool)> = vec![run(&expected[0])];
    results.extend(expected[1..].par_iter().map(run).collect::<Vec<_>>());
    let certified = results.iter().filter(|(_, _, c)| *c).count();
    let failing: Vec<String> = results
        .iter()
        .filter(|(_, p, _)| !p.is_empty())
        .map(|(case, p, _)| format!("{case}.ofn: {}", p.join("; ")))
        .collect();
    assert!(
        failing.is_empty(),
        "{} of {} frozen el_guaranteed cases disagree with the Konclude ∩ HermiT oracle \
         (see tests/fixtures/el_generated/MANIFEST):\n{}",
        failing.len(),
        results.len(),
        failing.join("\n")
    );
    assert!(
        certified >= CERTIFIED_FLOOR,
        "only {certified} of {} cases are completeness_guaranteed (floor {CERTIFIED_FLOOR}): \
         the guard has gone quiet",
        results.len()
    );
}

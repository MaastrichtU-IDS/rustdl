//! `classify` must agree with rustdl's own per-query surfaces.
//!
//! ## Why this exists
//!
//! Four separate defects have now been found where `classify` answered
//! differently from `subclass` / `sat` / `consistent` **on the same binary**:
//!
//! | | shape | mechanism |
//! |---|---|---|
//! | #66 | dropped subsumption | unanchored naming clause (fixed at root by #78/#83) |
//! | #90 | dropped subsumption, `ObjectHasSelf` under `∀` | wedge `Sat` + `trust_sat` |
//! | #91 | missed unsat class, complex cardinality qualifier | neither — no flag recovers it |
//! | #89 | `consistent: true` on an inconsistent KB | ABox clash the classify pre-check cannot reach |
//!
//! Four different mechanisms, one recurring SHAPE — and nothing was checking that
//! shape. `per_pair_fp_gate.rs` covers the FP direction on one surface; this
//! covers the AGREEMENT direction across all three.
//!
//! ## What it checks
//!
//! For each fixture, per surface:
//!
//! * `is_consistent()` vs `classify`'s `inconsistent` flag — 1 query, catches #89.
//! * `is_class_satisfiable(C)` vs `C ∈ classify.unsatisfiable_classes()` for every
//!   named class — O(n), catches #91.
//! * `is_subclass_of(a, b)` vs `classify.is_subclass(a, b)` for every ordered pair —
//!   O(n²), catches #66/#90. Restricted to fixtures under `MAX_PAIRWISE` classes,
//!   because a single hard SROIQ pair can cost tens of seconds.
//!
//! A disagreement means one of the two surfaces is wrong. Which one needs an
//! oracle to settle — this gate only reports that they diverge, which is the part
//! that was going unnoticed.

#![allow(clippy::unwrap_used)]
#![allow(unsafe_code)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::path::Path;

/// Above this class count the O(n²) pairwise leg is skipped: hard SROIQ pairs
/// (the `InterestingPizza` family) can each cost tens of seconds.
const MAX_PAIRWISE: usize = 40;

/// Per-class satisfiability budget. A hard class can otherwise run for minutes —
/// unbounded, `pizza` alone did not finish in 25 minutes. A timeout yields
/// `None`, which is NO OPINION and is skipped rather than scored: comparing
/// against a verdict we did not obtain would be the vacuous-pass failure this
/// suite exists to avoid. Skips are COUNTED and reported so the coverage loss is
/// visible instead of silent.
const SAT_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);

fn load(path: &Path) -> SetOntology<RcStr> {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    onto
}

/// Every disagreement found on one ontology, plus how many checks were skipped
/// because a probe timed out.
/// Serialises the tests that set a process-global `RUSTDL_*` flag against those
/// whose fixtures would observe it.
static FLAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set an env var for the duration of a scope and restore it on drop. Mirrors
/// `tests/adaptive_budget.rs`'s guard.
struct SetEnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl SetEnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let prev = std::env::var(key).ok();
        // SAFETY: callers hold `FLAG_LOCK`, so no other test in this binary is
        // reading or writing the environment concurrently.
        unsafe { std::env::set_var(key, val) };
        Self { key, prev }
    }
}

impl Drop for SetEnvGuard {
    fn drop(&mut self) {
        // SAFETY: as in `set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn divergences(
    onto: &SetOntology<RcStr>,
    label: &str,
    per_class: bool,
) -> (Vec<String>, usize, usize) {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    let mut compared = 0usize;
    let c = owl_dl_reasoner::classify(onto).unwrap_or_else(|e| panic!("classify {label}: {e:?}"));

    // ── consistency ────────────────────────────────────────────────────────
    let by_probe = owl_dl_reasoner::is_consistent(onto)
        .unwrap_or_else(|e| panic!("is_consistent {label}: {e:?}"));
    let by_classify = !c.stats().inconsistent;
    compared += 1;
    if by_probe != by_classify {
        out.push(format!(
            "{label}: consistency — is_consistent()={by_probe} but classify says {by_classify}"
        ));
    }

    // ── per-class satisfiability ───────────────────────────────────────────
    let unsat: std::collections::BTreeSet<&str> = c.unsatisfiable_classes().into_iter().collect();
    let classes: Vec<String> = c.classes().iter().map(|s| (*s).clone()).collect();
    for cl in classes.iter().filter(|_| per_class) {
        let Some(sat_by_probe) =
            owl_dl_reasoner::is_class_satisfiable_with_timeout(onto, cl, SAT_BUDGET)
                .unwrap_or_else(|e| panic!("is_class_satisfiable {label} {cl}: {e:?}"))
        else {
            skipped += 1;
            continue;
        };
        compared += 1;
        let sat_by_classify = !unsat.contains(cl.as_str());
        if sat_by_probe != sat_by_classify {
            out.push(format!(
                "{label}: satisfiability of <{cl}> — sat()={sat_by_probe} but classify says {sat_by_classify}"
            ));
        }
    }

    // ── pairwise subsumption (small fixtures only) ─────────────────────────
    if per_class && classes.len() <= MAX_PAIRWISE {
        for a in &classes {
            for b in &classes {
                if a == b {
                    continue;
                }
                let by_probe = owl_dl_reasoner::is_subclass_of(onto, a, b)
                    .unwrap_or_else(|e| panic!("is_subclass_of {label}: {e:?}"));
                let by_classify = c.is_subclass(a, b);
                compared += 1;
                if by_probe != by_classify {
                    out.push(format!(
                        "{label}: <{a}> ⊑ <{b}> — subclass()={by_probe} but classify says {by_classify}"
                    ));
                }
            }
        }
    }
    (out, skipped, compared)
}

/// Curated fixtures must show ZERO divergence. This is the regression guard:
/// a future change that makes `classify` disagree with the per-query surfaces on
/// any of these fails here.
#[test]
#[ignore = "needs the gitignored ontologies/real/*.ofn; the pairwise leg is O(n²) per fixture"]
fn classify_agrees_with_the_per_query_surfaces() {
    let base = Path::new("../../ontologies/real");
    let mut found: Vec<String> = Vec::new();
    let mut checked: Vec<String> = Vec::new();
    // (fixture, run the O(n) per-class satisfiability leg?)
    //
    // The consistency leg is ONE query and runs everywhere. The per-class leg is
    // restricted to fixtures where the probes actually return inside SAT_BUDGET:
    // measured, `ro`'s classes cost ~20 s EACH unbounded (58 of them), and `pizza`
    // did not finish its per-class leg in 25 minutes. Including them would either
    // blow the runtime or — worse — skip every probe and report a vacuous 0.
    // The `compared > 0` assertion below is what turned that from a silent green
    // into a failure when it was first tried.
    for (name, per_class) in [
        ("bibtex", true),
        ("sulo", true),
        ("ro", false),
        ("pizza", false),
    ] {
        let p = base.join(format!("{name}.ofn"));
        assert!(
            p.exists(),
            "[agree] REQUIRED fixture `{name}` missing ({}). Fetch with \
             ./scripts/fetch-real-ontologies.sh — do NOT skip: a vacuous pass here reads \
             identically to a real one.",
            p.display()
        );
        let onto = load(&p);
        let (d, skipped, compared) = divergences(&onto, name, per_class);
        eprintln!(
            "[agree] {name}: {} divergence(s), {compared} comparisons, {skipped} skipped (probe timeout)",
            d.len()
        );
        for line in &d {
            eprintln!("[agree]   {line}");
        }
        assert!(
            compared > 0,
            "[agree] {name}: ZERO comparisons made — every probe timed out, so a green \
             result here would be vacuous"
        );
        checked.push(format!("{name}({compared})"));
        found.extend(d);
    }
    eprintln!("[agree] checked: {}", checked.join(", "));
    assert!(
        found.is_empty(),
        "[agree] classify disagrees with rustdl's own per-query surfaces:\n{}",
        found.join("\n")
    );
}

/// Shapes that USED to diverge and now must agree. A row arrives here when
/// `the_gate_detects_the_known_divergences` fails because the defect was fixed —
/// so the same fixture keeps working, on the other side of the ledger.
#[test]
fn previously_divergent_shapes_now_agree() {
    let _lock = FLAG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // #89 — classify reported `consistent: true` on a KB `rustdl consistent`,
    // Konclude and Kobayashi-MaRust all call inconsistent. Fixed by consulting the
    // wedge consistency route in classify's pre-check.
    let body = r#"Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
                  DataPropertyRange(:p xsd:double)
                  DataPropertyAssertion(:p :a "1.0"^^xsd:float)"#;
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(\n{body}\n)"
    );
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).unwrap();
    let (d, _skipped, compared) = divergences(&onto, "#89", true);
    assert!(
        compared > 0,
        "no comparison made — a green result would be vacuous"
    );
    assert!(
        d.is_empty(),
        "#89 regressed: classify and the per-query surfaces disagree again:\n{}",
        d.join("\n")
    );

    // #91 — classify reported `unsatisfiable: []` for `A ⊑ ≤1 r.(B⊓C)` plus
    // `A ⊑ ≥2 r.(B⊓C)`, which both oracles and `rustdl sat A` call unsatisfiable.
    // Fixed by not trusting a wedge `Sat` for a class counting over a COMPLEX
    // qualifier. This shape produced THREE divergences, not one: the missed unsat
    // also made `A ⊑ B` and `A ⊑ C` wrong, because classify did not know `A` was
    // unsatisfiable. All three must stay fixed.
    let body91 = r"Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
                   Declaration(ObjectProperty(:r))
                   SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
                   SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))";
    let src91 = format!("Prefix(:=<http://ex#>)\nOntology(\n{body91}\n)");
    let mut r91 = Cursor::new(src91);
    let (onto91, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r91, ParserConfiguration::default()).unwrap();
    let (d91, _s91, c91) = divergences(&onto91, "#91", true);
    assert!(
        c91 > 0,
        "no comparison made — a green result would be vacuous"
    );
    assert!(
        d91.is_empty(),
        "#91 regressed: classify and the per-query surfaces disagree again:\n{}",
        d91.join("\n")
    );

    // #90 — classify dropped `T ⊑ S` whenever `ObjectHasSelf` appeared in the
    // filler, in either polarity. Fixed by teaching `eval_order` that a role atom
    // targeting an already-bound variable is a FILTER rather than an unsupported
    // shape: refusing it discarded the whole clause, so the constraint went
    // silently unenforced. The disjunctive filler below is the row this gate used
    // to carry as a live defect.
    let body90 = r"Declaration(Class(:S)) Declaration(Class(:T)) Declaration(Class(:Z))
              Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
              EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)) ObjectComplementOf(:Z))))
              SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)) ObjectComplementOf(:Z))))";
    let src90 = format!("Prefix(:=<http://ex#>)\nOntology(\n{body90}\n)");
    let mut r90 = Cursor::new(src90);
    let (onto90, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r90, ParserConfiguration::default()).unwrap();
    let (d90, _s90, c90) = divergences(&onto90, "#90", true);
    assert!(
        c90 > 0,
        "no comparison made — a green result would be vacuous"
    );
    assert!(
        d90.is_empty(),
        "#90 regressed: classify and the per-query surfaces disagree again:\n{}",
        d90.join("\n")
    );
}

/// The gate must DETECT divergence, not merely fail to find it — otherwise the
/// passing gate above is vacuous and nobody can tell.
///
/// **This happened THREE times, on consecutive days: #89, #91, then #90.** Each
/// fix made this test fail with `the gate did NOT detect [...]`, and each fixture
/// moved into `previously_divergent_shapes_now_agree`. An `#[ignore]`d sentinel
/// would have gone on passing silently; see
/// `docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`.
///
/// **#90's fix emptied the roster, and an empty roster passes VACUOUSLY** — the
/// `undetected` list would be empty because there was nothing to look for. So this
/// no longer depends on a live defect at all: it MANUFACTURES one by reverting a
/// shipped fix through its own flag (`RUSTDL_COMPLEX_QUALIFIER_VERIFY=0` restores
/// #91), which proves the detection leg fires and keeps proving it after every
/// future fix. `assert!(!cases.is_empty())` guards the roster itself.
///
/// The flag is process-global and `previously_divergent_shapes_now_agree` runs
/// #91's fixture, so both take [`FLAG_LOCK`]. Every other fixture in this file is
/// inert for that flag (none carries a cardinality over a non-named filler).
#[test]
fn the_gate_detects_the_known_divergences() {
    let _lock = FLAG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: single-threaded within this lock; every other test in this binary
    // that could observe the flag takes the same lock, and the guard restores it.
    let _flag = SetEnvGuard::set("RUSTDL_COMPLEX_QUALIFIER_VERIFY", "0");
    // (issue, leg exercised, ontology body). MANUFACTURED, not live: the flag set
    // above reverts #91, so classify misses the unsatisfiable class while the
    // per-pair surface still finds it — the divergence this gate must see.
    let cases: [(&str, &str, &str); 1] = [(
        "#91 (manufactured via RUSTDL_COMPLEX_QUALIFIER_VERIFY=0)",
        "unsatisfiability",
        r"Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
              Declaration(ObjectProperty(:r))
              SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
              SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))",
    )];
    let mut undetected: Vec<&str> = Vec::new();
    // Non-vacuity is counted, not asserted on the array literal: an empty roster
    // (or a roster whose fixtures compare nothing) would otherwise leave
    // `undetected` empty and pass without looking for anything.
    let mut total_compared = 0usize;
    for (issue, leg, body) in cases {
        let src = format!(
            "Prefix(:=<http://ex#>)\n\
             Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
             Ontology(\n{body}\n)"
        );
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut reader, ParserConfiguration::default()).unwrap();
        let (d, _skipped, compared) = divergences(&onto, issue, true);
        eprintln!(
            "[agree] {issue} ({leg} leg): {} divergence(s) in {compared} comparisons",
            d.len()
        );
        for line in &d {
            eprintln!("[agree]   {line}");
        }
        total_compared += compared;
        if d.is_empty() {
            undetected.push(issue);
        }
    }
    assert!(
        total_compared > 0,
        "the detection roster compared nothing, so a green result would be vacuous"
    );
    assert!(
        undetected.is_empty(),
        "[agree] the gate did NOT detect {undetected:?}. Either those defects were fixed — \
         in which case delete the row and move the fixture into \
         `classify_agrees_with_the_per_query_surfaces` — or the corresponding leg of \
         `divergences` has stopped working, which would make the passing gate above vacuous."
    );
}

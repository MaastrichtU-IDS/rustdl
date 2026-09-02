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

/// The gate must DETECT divergence, not merely fail to find it. Each case below
/// is a known live defect, one per leg, so all three legs are proven to fire.
///
/// **This test asserts the CURRENT (broken) state deliberately.** When one of
/// these is fixed it will FAIL, which is the signal to delete that row and — if
/// the fixture is cheap — move it into the passing gate above. That is the
/// opposite of an `#[ignore]`d sentinel, which goes stale silently: a fix here is
/// loud. See `docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md` for the
/// failure mode this avoids.
#[test]
fn the_gate_detects_the_known_divergences() {
    // (issue, leg exercised, ontology body)
    let cases: [(&str, &str, &str); 3] = [
        (
            "#89",
            "consistency",
            r#"Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
               DataPropertyRange(:p xsd:double)
               DataPropertyAssertion(:p :a "1.0"^^xsd:float)"#,
        ),
        (
            "#91",
            "satisfiability",
            r"Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
              Declaration(ObjectProperty(:r))
              SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
              SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))",
        ),
        (
            "#90",
            "subsumption",
            r"Declaration(Class(:S)) Declaration(Class(:T)) Declaration(Class(:Z))
              Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
              EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)) ObjectComplementOf(:Z))))
              SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)) ObjectComplementOf(:Z))))",
        ),
    ];
    let mut undetected: Vec<&str> = Vec::new();
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
        if d.is_empty() {
            undetected.push(issue);
        }
    }
    assert!(
        undetected.is_empty(),
        "[agree] the gate did NOT detect {undetected:?}. Either those defects were fixed — \
         in which case delete the row and move the fixture into \
         `classify_agrees_with_the_per_query_surfaces` — or the corresponding leg of \
         `divergences` has stopped working, which would make the passing gate above vacuous."
    );
}

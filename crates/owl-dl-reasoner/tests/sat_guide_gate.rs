//! THROWAWAY SP-B viability gate (does NOT merge; only the verdict doc lands).
//!
//! Measures whether feeding the B1–B2c saturation forcing into the wedge's ⊔
//! choice (the `RUSTDL_SAT_GUIDE` live-disjunct filter) collapses wine's branch
//! count toward Konclude's regime. Reads `RUSTDL_SAT_GUIDE` from the environment,
//! so run it TWICE and diff the printed tables:
//!
//!   # OFF baseline (raw branch explosion):
//!   cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture
//!   # ON (saturation-guided):
//!   RUSTDL_SAT_GUIDE=1 cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture
//!
//! Adaptive budget is forced OFF so the divergence early-cut does not mask the
//! raw OFF branch count. Runs on a 2 GiB-stack thread (deep disjunctive recursion).
//! GO/NO-GO bar: near-total Konclude-class collapse (hundreds, not tens of
//! thousands) on ≥2/3 pairs + single-digit-second wall + verdict-preserved.

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const WINE: &str = "../../ontologies/real/wine.ofn";
const NS: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(WINE).unwrap_or_else(|e| panic!("read {WINE}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse wine.ofn");
    ont
}

/// Guide-population sanity: rules out the "no-op guide" reading of `pruned=0`.
/// Wine has 39 DisjointClasses, so the told-disjoint table the guide is built
/// from MUST be non-empty, and the saturation closure must give real subsumers.
/// If this fails, `pruned=0` is a bug (empty/misaligned table), not a true
/// negative. Run: `cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture wine_guide_disjoint_table`.
#[test]
#[ignore = "throwaway SP-B gate sanity check"]
fn wine_guide_disjoint_table_is_populated() {
    let ont = load();
    let internal = owl_dl_core::convert::convert_ontology(&ont).expect("convert wine");
    let told = owl_dl_core::told::build_told_tables(&internal);
    let closure = owl_dl_saturation::saturate(&internal);
    let mut classes_with_disjoints = 0usize;
    let mut total_disjoint_entries = 0usize;
    let mut max_subsumers = 0usize;
    for (id, _) in internal.vocabulary.classes() {
        let dj = told.disjoints_of(id);
        if !dj.is_empty() {
            classes_with_disjoints += 1;
            total_disjoint_entries += dj.len();
        }
        max_subsumers = max_subsumers.max(closure.subsumers_of(id).len());
    }
    println!(
        "wine guide source: classes_with_disjoints={classes_with_disjoints} \
         total_disjoint_entries={total_disjoint_entries} max_subsumers_of_any_class={max_subsumers}"
    );
    assert!(
        classes_with_disjoints > 0 && total_disjoint_entries > 0,
        "told-disjoint table is EMPTY — the guide is a no-op by construction, \
         so pruned=0 would be a bug, not a true negative"
    );
    assert!(
        max_subsumers > 1,
        "saturation closure gave no real subsumers — guide subsumers side is empty"
    );
}

#[test]
#[ignore = "throwaway SP-B viability gate; run explicitly with/without RUSTDL_SAT_GUIDE"]
fn sat_guide_wine_branch_collapse() {
    // SAFETY: set before any reasoning thread is spawned; single-threaded here.
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let depth = 256usize;
            let dl = Some(Duration::from_secs(
                std::env::var("GATE_DEADLINE_S")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(60),
            ));
            let flag_on = std::env::var("RUSTDL_SAT_GUIDE").as_deref() == Ok("1");
            println!("\n##### SP-B GATE  flag_on={flag_on}  depth={depth}  deadline=60s #####");
            // (sub, Some(sup)) => sat(sub ⊓ ¬sup); (sub, None) => sat(sub) alone.
            let pairs: &[(&str, Option<&str>)] = &[
                ("AlsatianWine", Some("AmericanWine")),
                ("SweetWine", None),
                ("Zinfandel", None),
                ("RedWine", None),
            ];
            for (sub, sup) in pairs {
                let s_iri = format!("{NS}{sub}");
                let out = match sup {
                    Some(p) => owl_dl_reasoner::decide_pair_probe(
                        &ont,
                        &s_iri,
                        &format!("{NS}{p}"),
                        depth,
                        dl,
                    ),
                    None => owl_dl_reasoner::sat_class_probe(&ont, &s_iri, depth, dl),
                }
                .expect("probe ok");
                match out {
                    Some((res, s, ms)) => println!(
                        "{:>14} {sub_sup:24} verdict={res:?} wall_ms={ms:.0} \
                         branches={} disj={} merge={} restores={} max_depth={} \
                         | guide: points_seen={} pruned={} forced_single={} \
                         class_atoms={} nonclass_atoms={}",
                        "",
                        s.branches_taken,
                        s.disj_branches,
                        s.merge_branches,
                        s.restores,
                        s.max_branch_depth,
                        s.disj_points_seen,
                        s.disj_disjuncts_pruned,
                        s.disj_forced_single,
                        s.disj_class_atoms,
                        s.disj_nonclass_atoms,
                        sub_sup = match sup {
                            Some(p) => format!("{sub}⊓¬{p}"),
                            None => format!("sat({sub})"),
                        },
                    ),
                    None => println!("{sub:>14}  NOT A NAMED CLASS (check IRI)"),
                }
            }
            println!("##### END GATE flag_on={flag_on} #####\n");
        })
        .expect("spawn big-stack thread");
    child.join().expect("gate thread");
}

//! GATE: SP-0 shadow precise-dependency probe (deep nominal rearch).
//!
//! Read-only measurement: on wine's hard classes, does precise per-fact merge
//! causation make the dense clash-dependency chains SPARSE (bjgap grows, nogoods
//! become reusable → GO build CMERGED*) or do they stay dense (genuine semantic
//! structure → NO-GO)? The probe is read-only; flag-on must equal flag-off on
//! verdict/branches (the closure spot-check in konclude_closure_diff proves it at
//! scale). Here we only read `clash_records` and run `analyze`.
//!
//! Run:
//!   RUSTDL_ADAPTIVE_BUDGET=0 RUSTDL_SHADOW_DEP_PROBE=1 cargo test -p owl-dl-reasoner \
//!     --release --test shadow_dep_gate -- --ignored --nocapture

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_tableau::hyper::ClashRecord;
use owl_dl_tableau::shadow_measures::{ShadowReport, analyze};
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/data/dumontier/rustdl/ontologies/real/wine.ofn";
const WINE: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn iri(local: &str) -> String {
    format!("{WINE}{local}")
}

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH)
        .unwrap_or_else(|e| panic!("read {OFN_PATH}: {e} (run ./scripts/fetch-real-ontologies.sh)"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn print_report(label: &str, verdict: &str, n_branches: u64, records: &[ClashRecord]) {
    let r: ShadowReport = analyze(records);
    // Distinguish: (a) taint→ALL never fires on the hot clash path vs (b) it fires
    // but precise deps are ALSO dense. `real=ALL` ⇒ taint fired; `shadow≠real` ⇒
    // the precise layer recovered something different from the real (collapsed) set.
    let real_all = records
        .iter()
        .filter(|c| c.real.highest == Some(127) && c.real.count == 0)
        .count();
    let shadow_differs = records
        .iter()
        .filter(|c| c.real.levels != c.shadow.levels || c.real.highest != c.shadow.highest)
        .count();
    println!("\n===== {label}  (verdict={verdict}, branches={n_branches}) =====");
    println!("  clashes recorded = {}", r.n_clashes);
    println!(
        "  real=ALL (taint fired) = {real_all}   shadow≠real (precise recovered) = {shadow_differs}"
    );
    println!(
        "  bjgap REAL   : min={} median={} p90={} max={} mean={:.2}",
        r.bjgap_real.min, r.bjgap_real.median, r.bjgap_real.p90, r.bjgap_real.max, r.bjgap_real.mean
    );
    println!(
        "  bjgap SHADOW : min={} median={} p90={} max={} mean={:.2}",
        r.bjgap_shadow.min,
        r.bjgap_shadow.median,
        r.bjgap_shadow.p90,
        r.bjgap_shadow.max,
        r.bjgap_shadow.mean
    );
    println!(
        "  reusable_nogood_frac={:.4} (distinct_nogoods={})  revisit_frac={:.4}  revisit_ctx_shared_frac={:.4}",
        r.reusable_nogood_frac, r.distinct_nogoods, r.revisit_frac, r.revisit_context_shared_frac
    );
}

fn probe_sat(label: &str, local: &str, depth: usize, t: Option<Duration>) {
    let ont = load();
    let (result, stats, wall_ms) = owl_dl_reasoner::sat_class_probe(&ont, &iri(local), depth, t)
        .expect("probe ok")
        .expect("IRI resolves");
    println!("  [{label}] wall_ms={wall_ms:.0}");
    print_report(label, &format!("{result:?}"), stats.branches_taken, &stats.clash_records);
}

fn probe_pair(label: &str, sub: &str, sup: &str, depth: usize, t: Option<Duration>) {
    let ont = load();
    let (result, stats, wall_ms) =
        owl_dl_reasoner::decide_pair_probe(&ont, &iri(sub), &iri(sup), depth, t)
            .expect("probe ok")
            .expect("IRIs resolve");
    println!("  [{label}] wall_ms={wall_ms:.0}");
    print_report(label, &format!("{result:?}"), stats.branches_taken, &stats.clash_records);
}

/// The gate. Set RUSTDL_SHADOW_DEP_PROBE=1 and RUSTDL_ADAPTIVE_BUDGET=0 externally.
#[test]
#[ignore = "gate measurement; run with RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 --ignored --nocapture"]
fn wine_shadow_dep_report() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let t = Some(Duration::from_secs(60));
            let depth = 256usize;
            probe_pair(
                "sat(AlsatianWine ⊓ ¬AmericanWine)",
                "AlsatianWine",
                "AmericanWine",
                depth,
                t,
            );
            probe_sat("sat(SweetWine)", "SweetWine", depth, t);
            probe_sat("sat(Zinfandel)", "Zinfandel", depth, t);
            probe_sat("sat(WhiteNonSweetWine)", "WhiteNonSweetWine", depth, t);
            probe_sat("sat(RedTableWine)", "RedTableWine", depth, t);
        })
        .expect("spawn big-stack thread");
    child.join().expect("gate thread");
}

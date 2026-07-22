//! bjgap / conflict-structure probe for the DL-tail CDCL go/no-go (2026-07-22).
//!
//! Decides whether 1-UIP conflict learning is viable on a given ontology's
//! *stalling* wedge search. CDCL/1-UIP prunes subtrees only when a clash depends
//! on FEW, SHALLOW decisions (big bjgap). The discriminator, per the reopened
//! conflict-learning finding:
//!   - deps≈1 at deep clash depth (e.g. ore_ont_778 ∀+complement) → big bjgap →
//!     learned nogoods prune ~hundreds of levels → CDCL ALIVE.
//!   - dense deps / highest≈clash-depth (wine nominal-merge; ore_ont_10019 /
//!     3215 shared-conjunct defined classes) → bjgap≈1 → CDCL DEAD (reuse-trap).
//!
//! Reads (env):
//!   RUSTDL_BJGAP_ONT   = path to an .owl/.ofn ontology (required)
//!   RUSTDL_BJGAP_CLASS = full class IRI to sat-probe (required)
//!   RUSTDL_BJGAP_DEPTH = branch-depth cap (default 256)
//!   RUSTDL_BJGAP_SECS  = per-probe timeout seconds (default 30)
//!
//! Uses the shadow dep probe (never collapses to ALL via taints), so `shadow`
//! popcount/highest is the SOUND over-set — the honest bjgap ceiling.
//!
//! Run: RUSTDL_BJGAP_ONT=... RUSTDL_BJGAP_CLASS=... \
//!   cargo test -p owl-dl-reasoner --test bjgap_dl_tail -- --ignored --nocapture

#![allow(
    clippy::unwrap_used,
    clippy::doc_markdown,
    unsafe_code,
    clippy::cast_precision_loss
)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

fn load(path: &str) -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn");
    ont
}

/// Find GENUINE hard pairs on an ontology: classify under a per-pair budget +
/// moderate global cap, then print the timed-out pairs where `c != d` (real
/// per-pair tableau timeouts from `subsumes_via_tableau`, distinct from the
/// `(c,c)` global-deadline bail markers). Feed the printed pairs to
/// `decide_pair_probe` (bjgap_pair_histogram) to read their conflict structure.
///
/// Env: RUSTDL_BJGAP_ONT (path), RUSTDL_HARD_PAIR_MS (per-pair ms, default 1500),
///      RUSTDL_HARD_GLOBAL_MS (global cap ms, default 180000).
#[test]
#[ignore = "diagnostic; needs RUSTDL_BJGAP_ONT"]
fn find_hard_pairs() {
    let ont_path = std::env::var("RUSTDL_BJGAP_ONT").expect("set RUSTDL_BJGAP_ONT");
    let pp: u64 = std::env::var("RUSTDL_HARD_PAIR_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let gl: u64 = std::env::var("RUSTDL_HARD_GLOBAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180_000);
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(move || {
            let ont = load(&ont_path);
            let h = owl_dl_reasoner::classify_with_budget(
                &ont,
                Some(Duration::from_millis(pp)),
                Some(Duration::from_millis(gl)),
            )
            .expect("classify");
            let mut hard: Vec<(&str, &str)> = h
                .undecided_pairs()
                .into_iter()
                .filter(|(a, b)| a != b)
                .collect();
            hard.sort_unstable();
            hard.dedup();
            println!("\n===== hard pairs (c != d) for {ont_path} =====");
            println!(
                "  per_pair={pp}ms global={gl}ms  total_timed_out={} genuine_hard={}",
                h.stats().timed_out_pairs,
                hard.len()
            );
            for (a, b) in hard.iter().take(25) {
                println!("  HARDPAIR\t{a}\t{b}");
            }
        })
        .expect("spawn");
    child.join().expect("thread");
}

/// bjgap histogram for a specific SUB/SUP PAIR (`decide_pair_probe`), the granularity
/// where 3215-class stalls actually live (single-class sat is trivial there).
/// Env: RUSTDL_BJGAP_ONT, RUSTDL_BJGAP_SUB, RUSTDL_BJGAP_SUP, RUSTDL_BJGAP_DEPTH,
///      RUSTDL_BJGAP_SECS.
#[test]
#[ignore = "diagnostic; needs RUSTDL_BJGAP_ONT + RUSTDL_BJGAP_SUB + RUSTDL_BJGAP_SUP"]
fn bjgap_pair_histogram() {
    let ont_path = std::env::var("RUSTDL_BJGAP_ONT").expect("set RUSTDL_BJGAP_ONT");
    let sub = std::env::var("RUSTDL_BJGAP_SUB").expect("set RUSTDL_BJGAP_SUB");
    let sup = std::env::var("RUSTDL_BJGAP_SUP").expect("set RUSTDL_BJGAP_SUP");
    let depth: usize = std::env::var("RUSTDL_BJGAP_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256);
    let secs: u64 = std::env::var("RUSTDL_BJGAP_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
        std::env::set_var("RUSTDL_SHADOW_DEP_PROBE", "1");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(move || {
            let ont = load(&ont_path);
            let (result, s, wall_ms) = owl_dl_reasoner::decide_pair_probe(
                &ont,
                &sub,
                &sup,
                depth,
                Some(Duration::from_secs(secs)),
            )
            .expect("probe ok")
            .expect("both IRIs resolve");
            println!("\n===== bjgap PAIR probe: {sub}  ⊑?  {sup} =====");
            println!("  result={result:?}  wall_ms={wall_ms:.0}");
            println!(
                "  branches={} (disj={} merge={})  restores={}  max_depth={}/{}",
                s.branches_taken,
                s.disj_branches,
                s.merge_branches,
                s.restores,
                s.max_branch_depth,
                depth
            );
            report_clash_records(&s, u32::try_from(depth).unwrap_or(u32::MAX));
        })
        .expect("spawn");
    child.join().expect("thread");
}

fn report_clash_records(s: &owl_dl_tableau::hyper::SearchStats, cap: u32) {
    let recs = &s.clash_records;
    println!("  total clashes recorded: {}", recs.len());
    if recs.is_empty() {
        println!("  (no clashes — not a conflict stall)");
        return;
    }
    let deep_thresh = cap.saturating_sub(16);
    let deep: Vec<_> = recs
        .iter()
        .filter(|r| r.branch_depth >= deep_thresh)
        .collect();
    println!(
        "  deep clashes (branch_depth >= {deep_thresh}): {}",
        deep.len()
    );
    let sample: Vec<_> = if deep.is_empty() {
        recs.iter().collect()
    } else {
        deep
    };
    let label = if sample.len() == recs.len() {
        "ALL clashes"
    } else {
        "DEEP clashes"
    };
    let n = sample.len() as f64;
    let mut cnt_hist = std::collections::BTreeMap::<u32, usize>::new();
    let (mut sum_cnt, mut sum_gap, mut overflow, mut gap_big) = (0u64, 0i64, 0usize, 0usize);
    for r in &sample {
        let sh = &r.shadow;
        *cnt_hist.entry(sh.count.min(20)).or_default() += 1;
        sum_cnt += u64::from(sh.count);
        if sh.highest == Some(127) && sh.count == 0 {
            overflow += 1;
        } else if let Some(h) = sh.highest {
            let gap = i64::from(r.branch_depth) - i64::from(h);
            sum_gap += gap;
            if gap >= 8 {
                gap_big += 1;
            }
        }
    }
    println!("  --- {label} (shadow dep-set = sound over-set) ---");
    println!(
        "  mean deps/clash: {:.2}   mean bjgap: {:.1}",
        sum_cnt as f64 / n,
        sum_gap as f64 / n
    );
    println!(
        "  bjgap>=8 (CDCL prunes a subtree): {}/{} = {:.0}%",
        gap_big,
        sample.len(),
        100.0 * gap_big as f64 / n
    );
    println!(
        "  shadow=ALL/overflow (>127 levels; CDCL-dead): {}/{} = {:.0}%",
        overflow,
        sample.len(),
        100.0 * overflow as f64 / n
    );
    println!("  dep-count histogram (capped 20):");
    for (k, v) in &cnt_hist {
        println!("    deps={k:>2}: {v}");
    }
    println!(
        "  HINT: deps~1-3 + bjgap>=8 high => CDCL-ALIVE; dense deps / overflow high => CDCL-DEAD."
    );
}

#[test]
#[ignore = "diagnostic; needs RUSTDL_BJGAP_ONT + RUSTDL_BJGAP_CLASS"]
fn bjgap_histogram() {
    let ont_path = std::env::var("RUSTDL_BJGAP_ONT").expect("set RUSTDL_BJGAP_ONT");
    let class_iri = std::env::var("RUSTDL_BJGAP_CLASS").expect("set RUSTDL_BJGAP_CLASS");
    let depth: usize = std::env::var("RUSTDL_BJGAP_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256);
    let secs: u64 = std::env::var("RUSTDL_BJGAP_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    // Adaptive budget OFF (don't mask the raw search); shadow probe ON.
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
        std::env::set_var("RUSTDL_SHADOW_DEP_PROBE", "1");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(move || {
            let ont = load(&ont_path);
            let (result, s, wall_ms) = owl_dl_reasoner::sat_class_probe(
                &ont,
                &class_iri,
                depth,
                Some(Duration::from_secs(secs)),
            )
            .expect("probe ok")
            .expect("class IRI resolves");
            println!("\n===== bjgap probe: {class_iri} =====");
            println!("  ont={ont_path}");
            println!("  result={result:?}  wall_ms={wall_ms:.0}");
            println!(
                "  branches={} (disj={} merge={})  restores={}  max_depth={}/{}",
                s.branches_taken,
                s.disj_branches,
                s.merge_branches,
                s.restores,
                s.max_branch_depth,
                depth
            );
            report_clash_records(&s, u32::try_from(depth).unwrap_or(u32::MAX));
        })
        .expect("spawn big-stack thread");
    child.join().expect("probe thread");
}

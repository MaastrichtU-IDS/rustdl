//! Root-cause diagnostic for the ore-15672 classify stall (the rewrite R0).
//!
//! Runs the REAL classify per-pair oracle (`HyperCache::decide` via
//! `decide_pair_probe`) on the single timed-out pair `epistemic-workflow-enactment`
//! (sub) vs `task` (sup) — the textbook subsumption test `sat(ewe ⊓ ¬task)`,
//! which is correctly NOT a subsumption (so a sound engine returns `Sat`).
//!
//! Prints the engine's `SearchStats` + raw `HyperResult` so we can read the
//! literal Stalled-cause, and SWEEPS the branch-depth cap to settle the central
//! question: is the stall DEPTH-bound (the real model needs >256 nested
//! disjunctions → cheap fix: raise cap + iterative solve) or STRATEGY-bound (bad
//! branch ordering nests forever → expensive: ordering/learning)?
//!   - reaches `Sat` at some depth D  → DEPTH-bound (cheap)
//!   - never `Sat`, just deeper Stalls → STRATEGY-bound (expensive)
//!
//! Adaptive budget is forced OFF here so the divergence early-cut does not mask
//! the raw search. Each depth runs on a 2 GiB-stack thread so deep recursion
//! does not stack-overflow the sweep.
//!
//! Run: `cargo test -p owl-dl-reasoner --test decide_pair_probe -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/tmp/ore15672_probe.ofn";
const SUB: &str =
    "http://www.loa-cnr.it/ontologies/OD/OntologyDesign.owl#epistemic-workflow-enactment";
const SUP: &str = "http://www.loa-cnr.it/ontologies/ExtendedDnS.owl#task";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH)
        .unwrap_or_else(|e| panic!("read {OFN_PATH}: {e} (create with the R0 strip step)"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn run(depth: usize, timeout: Option<Duration>) {
    let ont = load();
    let out = owl_dl_reasoner::decide_pair_probe(&ont, SUB, SUP, depth, timeout)
        .expect("probe ok")
        .expect("both IRIs resolve to named classes");
    let (result, s, wall_ms) = out;
    println!("\n----- depth={depth} timeout={timeout:?} -----");
    println!("  result={result:?}  wall_ms={wall_ms:.0}");
    println!(
        "  branches={} (disj={} merge={})  restores={}  max_depth={}/{}",
        s.branches_taken, s.disj_branches, s.merge_branches, s.restores, s.max_branch_depth, depth
    );
    println!(
        "  fixpoint_passes={}  match_attempts={}  node_clones={}",
        s.fixpoint_passes, s.match_attempts, s.node_clones
    );
    println!(
        "  is_blocked={} (eligible={} fired={} compares={})",
        s.is_blocked_calls, s.block_eligible, s.blocks_fired, s.block_compares
    );
}

/// Does `sat(ewe)` ALONE thrash (→ shared expansion problem) or terminate fast
/// (→ the per-pair `¬sup` interaction is the problem)? Adaptive OFF, 30s.
#[test]
#[ignore = "diagnostic; needs /tmp/ore15672_probe.ofn"]
fn probe_ewe_alone() {
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let t = Some(Duration::from_secs(30));
            for depth in [256usize, 2048] {
                let (result, s, wall_ms) = owl_dl_reasoner::sat_class_probe(&ont, SUB, depth, t)
                    .expect("probe ok")
                    .expect("IRI resolves");
                println!("\n===== sat(ewe) ALONE depth={depth} =====");
                println!("  result={result:?}  wall_ms={wall_ms:.0}");
                println!(
                    "  branches={} (disj={})  restores={}  max_depth={}/{}",
                    s.branches_taken, s.disj_branches, s.restores, s.max_branch_depth, depth
                );
            }
        })
        .expect("spawn");
    child.join().expect("thread");
}

/// Sweep the depth cap on a big-stack thread with adaptive budget OFF.
#[test]
#[ignore = "diagnostic; needs /tmp/ore15672_probe.ofn"]
fn probe_ewe_task_depth_sweep() {
    // SAFETY: single-threaded test (`--test-threads` irrelevant here; we spawn
    // our own thread and join). Setting before any reasoning runs.
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            // Generous 30 s/pair so a depth-bound search has room to FIND the model.
            let t = Some(Duration::from_secs(30));
            for depth in [256usize, 512, 1024, 2048, 4096, 8192, 32768] {
                run(depth, t);
            }
        })
        .expect("spawn big-stack thread");
    child.join().expect("sweep thread");
}

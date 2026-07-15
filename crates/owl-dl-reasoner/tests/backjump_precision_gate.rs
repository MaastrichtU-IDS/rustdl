//! GATE: H3b backjump-precision probe on `ore_ont_10019`'s disjunctive-DFS stall.
//!
//! Read-only measurement (Phase 1 of the wedge backjump-precision R&D): does
//! `ore_ont_10019`'s stall come from **backjump degradation** — the real
//! `clash_deps` collapsing to `DepSet::ALL` at a clash where a precise shadow
//! dep-set exists (which would let dependency-directed backjumping skip most
//! of the disjunctive search) — or is the search intrinsically wide (real deps
//! already precise, so no backjump repair would help)? This decides between
//! Phase-2's **Fix #1** (backjump-precision repair: tighten the widening
//! site(s), FP-critical sound over-approximation only) and **Fix #2**
//! (absorption/BCP or bound-the-tail).
//!
//! This is a **fix-selecting probe, not a fix**: it makes NO engine or
//! behaviour change. It reuses the already-shipped shadow-dep probe
//! (`RUSTDL_SHADOW_DEP_PROBE`, see `owl_dl_tableau::hyper::HyperEngine::
//! with_shadow_dep_probe`), which is read-only by its own invariant (enabling
//! it must not change any verdict / `branches_taken` / `restores` /
//! `max_branch_depth` — see `shadow_dep_gate.rs`'s sibling gate for that
//! cross-check). Here we only read `stats.clash_records` and run
//! `shadow_measures::analyze`.
//!
//! Run (probe on, adaptive budget off so the search runs to its natural
//! per-class timeout instead of an early divergence cut):
//!   RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner \
//!     --release --test backjump_precision_gate -- --ignored --nocapture

#![allow(
    clippy::unwrap_used,
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    unsafe_code
)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_tableau::hyper::ClashRecord;
use owl_dl_tableau::shadow_measures::analyze;
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/Users/micheldumontier/data/ore-run/input/ore_ont_10019.ofn";
const NS: &str = "http://ontology.dumontierlab.com/";

/// The five deepest stalled classes from
/// `rustdl hyper-sat ore_ont_10019.ofn --per-class-timeout-ms 300` (branch
/// depth 137-138 of the 512 cap; `HydroxylGroup` confirmed present per the
/// task brief). All five show `merge=0` in the hyper-sat branch split (pure
/// `⊔`-disjunctive branching, no `≤n` merge branches) — a deliberate contrast
/// point: if `real_ALL` turns out high on these, it is NOT explained by
/// merge-taint (there is no merge to taint) and must come from some other
/// widening site (e.g. cardinality-lowered disjunctive clauses / `DepSet::ALL`
/// fallback on the `⊔` branch rule itself).
const STALLED_CLASSES: &[&str] = &[
    "HydroxylGroup",
    "MethylGroup",
    "OxygenAtom",
    "KetoneGroup",
    "SecondaryAmineGroup",
];

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH).unwrap_or_else(|e| {
        panic!("read {OFN_PATH}: {e} (ore corpus tier — see docs/benchmarks/2026-07-12-ore/)")
    });
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

/// Per the brief's Step 2: for a stalled class, probe its wedge satisfiability
/// and print the real-`ALL` frequency, the "crippled backjump" count (real
/// deps collapsed to `ALL` where the shadow/precise dep-set would have allowed
/// a real jump), and the `bjgap_real` vs `bjgap_shadow` histograms.
fn probe(class: &str) {
    let ont = load();
    let depth = 256usize;
    let t = Some(Duration::from_secs(30));
    let (result, stats, wall_ms) = owl_dl_reasoner::sat_class_probe(&ont, &iri(class), depth, t)
        .expect("probe ok")
        .expect("IRI resolves");
    let records: &[ClashRecord] = &stats.clash_records;
    let r = analyze(records);
    let n = records.len().max(1);
    let real_all = records
        .iter()
        .filter(|c| c.real.highest == Some(127) && c.real.count == 0)
        .count();
    // Disjunctive clashes where the PRECISE (shadow) dep would have allowed a
    // real backjump (bjgap_shadow > 1) but the real dep-set is ALL (so the
    // engine actually backjumped only 1 level — a "crippled" backjump).
    let crippled = records
        .iter()
        .filter(|c| {
            let real_all = c.real.highest == Some(127) && c.real.count == 0;
            let shadow_bjgap = c.shadow.highest.map_or(c.branch_depth + 1, |h| {
                c.branch_depth.saturating_sub(h).saturating_add(1)
            });
            real_all && shadow_bjgap > 1
        })
        .count();
    println!(
        "[{class}] result={result:?} wall_ms={wall_ms:.0} branches={} disj={} merge={} max_depth={} \
clashes={} real_ALL={}/{} ({:.1}%)  crippled_backjumps={}  \
bjgap_real(med={} p90={} max={})  bjgap_shadow(med={} p90={} max={})",
        stats.branches_taken,
        stats.disj_branches,
        stats.merge_branches,
        stats.max_branch_depth,
        r.n_clashes,
        real_all,
        n,
        100.0 * real_all as f64 / n as f64,
        crippled,
        r.bjgap_real.median,
        r.bjgap_real.p90,
        r.bjgap_real.max,
        r.bjgap_shadow.median,
        r.bjgap_shadow.p90,
        r.bjgap_shadow.max,
    );
}

/// The gate. Set `RUSTDL_SHADOW_DEP_PROBE=1` and `RUSTDL_ADAPTIVE_BUDGET=0`
/// externally (see module doc for the full invocation).
#[test]
#[ignore = "gate measurement; run with RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 --ignored --nocapture"]
fn ore_10019_backjump_precision_report() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            for class in STALLED_CLASSES {
                probe(class);
            }
        })
        .expect("spawn big-stack thread");
    child.join().expect("gate thread");
}

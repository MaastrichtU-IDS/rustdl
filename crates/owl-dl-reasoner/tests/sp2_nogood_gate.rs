//! GATE: SP2 Phase A, Task A2 — depth-binned shadow-dependency smoke test on
//! `ore_ont_10019`'s stalled classes.
//!
//! ## Necessary-not-sufficient smoke test — READ BEFORE INTERPRETING THE NUMBERS
//!
//! This harness is read-only measurement, modeled on `shadow_dep_gate.rs`. It
//! answers exactly one narrow question — the "empty-tail kill gate" from
//! `.superpowers/sdd/task-A2-brief.md`: do `ore_ont_10019`'s stalled classes
//! even have a non-trivial *deep* clash tail (`branch_depth >= split_depth`)?
//! If `n_deep` is essentially zero across the stalled classes, there is
//! nothing for a Phase B deep-tail subset-core prune to act on — that is the
//! one sound kill Phase A can make from this data alone.
//!
//! **What this harness CANNOT do:** confirm SP2's subset-core no-good
//! mechanism is viable, or soundly kill it on its own terms. A high
//! `deep.reusable_nogood_frac` / `deep.revisit_frac` would be corroborating
//! evidence, but a LOW one is NOT a kill — these `analyze()` metrics
//! summarize the *label-set-hash* revisit/reuse pattern of clashes recorded
//! by the existing shadow-dep probe; they do not lower-bound whether a
//! node-local, per-solve-scoped core-keyed prune (Phase B's actual mechanism)
//! would fire, because Phase B's mechanism scopes and keys nogoods
//! differently than this coarse aggregate. Likewise a large `bjgap_shadow`
//! (backjumping already jumps far) and a low `revisit_context_shared_frac`
//! are WARNINGS about the reuse-trap that Phase B's per-solve scope +
//! node-local oracle are specifically designed to neutralize — not kills.
//! The real go/no-go verdict on the mechanism itself is Phase B, not this
//! harness. See `docs/2026-07-14-sp2-nogood-findings.md` for the recorded
//! numbers and applied verdict.
//!
//! Run (both ways — the first disables the adaptive-budget early-cut so the
//! search runs closer to the per-class 30s deadline; the second leaves the
//! adaptive budget default-ON, i.e. the shipping behavior):
//!
//! ```sh
//! RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable \
//!   cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture
//! RUSTDL_SHADOW_DEP_PROBE=1 RUSTUP_TOOLCHAIN=stable \
//!   cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_tableau::hyper::ClashRecord;
use owl_dl_tableau::shadow_measures::{DepthBinnedReport, ShadowReport, analyze, analyze_by_depth};
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/Users/micheldumontier/data/ore-run/input/ore_ont_10019.ofn";
const NS: &str = "http://ontology.dumontierlab.com/";

/// `split_depth` for `analyze_by_depth`: the SP0 `hyper-sat --per-class-timeout-ms
/// 300` probe (`docs/2026-07-13-ore_ont_10019-stall-findings.md`, re-measured here
/// with the now-default-ON incremental-fixpoint SP1 change) shows these 33 stalled
/// classes branching from depth 0 up to an observed max of ~137-142. We pick the
/// midpoint between "shallow" (near the root, depth ~0) and that observed cap:
/// `(0 + 137) / 2 ≈ 68`, rounded to **70**.
const SPLIT_DEPTH: u32 = 70;

/// The 15 classes `hyper-sat --per-class-timeout-ms 300` reports as the top
/// stalled-by-branching tail (see Step 1 raw output archived in
/// `docs/2026-07-14-sp2-nogood-findings.md`) — a representative sample of
/// `ore_ont_10019`'s 33 `Stalled` classes, not an exhaustive list.
const STALLED_CLASSES: &[&str] = &[
    "SecondaryAmineGroup",
    "PrimaryAmineGroup",
    "MethylGroup",
    "CarbonAtom",
    "SulfinicAcidGeneralGroup",
    "SulfonicAcidGroup",
    "SulfoxideGroup",
    "KetoneGroup",
    "OxygenAtom",
    "Alkyl",
    "EtherGroup",
    "SulfonylHalideGroup",
    "SulfonicAcidDerivativeGroup",
    "AldehydeGroup",
    "AcylBromideGroup",
];

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH)
        .unwrap_or_else(|e| panic!("read {OFN_PATH}: {e} (fetch ~/data/ore-run/input)"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn print_aggregate(r: &ShadowReport) {
    println!("  clashes recorded = {}", r.n_clashes);
    println!(
        "  bjgap REAL   : min={} median={} p90={} max={} mean={:.2}",
        r.bjgap_real.min,
        r.bjgap_real.median,
        r.bjgap_real.p90,
        r.bjgap_real.max,
        r.bjgap_real.mean
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

fn print_depth_binned(b: &DepthBinnedReport) {
    println!(
        "  --- depth-binned (split_depth={}) : n_shallow={} n_deep={} ---",
        b.split_depth, b.n_shallow, b.n_deep
    );
    if b.n_deep == 0 {
        println!("  DEEP: (empty — no clash recorded at branch_depth >= split_depth)");
    } else {
        println!(
            "  DEEP  reusable_nogood_frac={:.4} (distinct_nogoods={})  revisit_frac={:.4}  revisit_ctx_shared_frac={:.4}",
            b.deep.reusable_nogood_frac,
            b.deep.distinct_nogoods,
            b.deep.revisit_frac,
            b.deep.revisit_context_shared_frac
        );
        println!(
            "  DEEP  bjgap_shadow: min={} median={} p90={} max={} mean={:.2}",
            b.deep.bjgap_shadow.min,
            b.deep.bjgap_shadow.median,
            b.deep.bjgap_shadow.p90,
            b.deep.bjgap_shadow.max,
            b.deep.bjgap_shadow.mean
        );
    }
    if b.n_shallow > 0 {
        println!(
            "  SHALLOW reusable_nogood_frac={:.4}  revisit_frac={:.4}  revisit_ctx_shared_frac={:.4}",
            b.shallow.reusable_nogood_frac,
            b.shallow.revisit_frac,
            b.shallow.revisit_context_shared_frac
        );
    }
}

fn print_report(label: &str, verdict: &str, n_branches: u64, records: &[ClashRecord]) {
    println!("\n===== {label}  (verdict={verdict}, branches={n_branches}) =====");
    print_aggregate(&analyze(records));
    print_depth_binned(&analyze_by_depth(records, SPLIT_DEPTH));
}

fn probe_class(local: &str) {
    let ont = load();
    let (result, stats, wall_ms) =
        owl_dl_reasoner::sat_class_probe(&ont, &iri(local), 256, Some(Duration::from_secs(30)))
            .expect("probe ok")
            .unwrap_or_else(|| panic!("IRI does not resolve: {}", iri(local)));
    println!("  [{local}] wall_ms={wall_ms:.0}");
    println!(
        "  BRANCH SPLIT: total={} disj={} merge={} max_depth={}",
        stats.branches_taken, stats.disj_branches, stats.merge_branches, stats.max_branch_depth
    );
    print_report(
        local,
        &format!("{result:?}"),
        stats.branches_taken,
        &stats.clash_records,
    );
}

/// The gate. Set `RUSTDL_SHADOW_DEP_PROBE=1` externally (and optionally
/// `RUSTDL_ADAPTIVE_BUDGET=0` for the asymptotic run); see the module doc for
/// both invocations.
#[test]
#[ignore = "gate measurement; run with RUSTDL_SHADOW_DEP_PROBE=1 [RUSTDL_ADAPTIVE_BUDGET=0] --ignored --nocapture"]
fn ore_ont_10019_stalled_depth_binned_report() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            for local in STALLED_CLASSES {
                probe_class(local);
            }
        })
        .expect("spawn big-stack thread");
    child.join().expect("gate thread");
}

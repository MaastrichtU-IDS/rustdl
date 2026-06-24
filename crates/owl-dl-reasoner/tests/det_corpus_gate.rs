//! THROWAWAY option-1 corpus gate (does NOT merge). Measures the deterministic
//! ⊔-resolution collapse ratio on the *terminating* SROIQ fixtures (sio / ore)
//! to decide whether a build-once deterministic-expansion cache is worth
//! prototyping as a corpus perf mechanism. Reuses the RUSTDL_DET_LOOKAHEAD_PROBE
//! look-ahead; sweeps a fixture's named classes, aggregates the 3 det counters.
//!
//!   RUSTDL_DET_LOOKAHEAD_PROBE=1 DET_ONT=ontologies/external/ore-15672-shoin.ofn \
//!     cargo test -p owl-dl-reasoner --test det_corpus_gate -- --ignored --nocapture
//!
//! DET_ONT = ontology path (relative to crate dir, so ../../<path>); DET_DEADLINE_S
//! = per-class deadline (default 5); DET_MAX_CLASSES = cap the sweep (default 0 = all).

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
use std::io::Cursor;
use std::time::Duration;

fn load(path: &str) -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

#[test]
#[ignore = "throwaway option-1 corpus det-resolution gate; set DET_ONT + RUSTDL_DET_LOOKAHEAD_PROBE=1"]
fn det_corpus_collapse_ratio() {
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let rel = std::env::var("DET_ONT")
                .unwrap_or_else(|_| "ontologies/external/ore-15672-shoin.ofn".to_string());
            let path = format!("../../{rel}");
            let secs: u64 = std::env::var("DET_DEADLINE_S")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5);
            let cap: usize = std::env::var("DET_MAX_CLASSES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let on = std::env::var("RUSTDL_DET_LOOKAHEAD_PROBE").as_deref() == Ok("1");
            let ont = load(&path);
            let internal = owl_dl_core::convert::convert_ontology(&ont).expect("convert");
            let mut iris: Vec<String> = internal
                .vocabulary
                .classes()
                .map(|(_, iri)| iri.to_string())
                .filter(|i| !i.starts_with("urn:rustdl-") && !i.contains("rustdl-dkey"))
                .collect();
            iris.sort();
            iris.dedup();
            if cap > 0 && iris.len() > cap {
                iris.truncate(cap);
            }
            println!("##### DET-CORPUS GATE ont={rel} probe_on={on} classes={} deadline={secs}s #####", iris.len());
            let dl = Some(Duration::from_secs(secs));
            let (mut tot_pts, mut tot_killed, mut tot_collapsed) = (0u64, 0u64, 0u64);
            let (mut sat, mut unsat, mut stalled) = (0u32, 0u32, 0u32);
            for iri in &iris {
                if let Ok(Some((res, s, _ms))) =
                    owl_dl_reasoner::sat_class_probe(&ont, iri, 256, dl)
                {
                    match res {
                        owl_dl_tableau::hyper::HyperResult::Sat => sat += 1,
                        owl_dl_tableau::hyper::HyperResult::Unsat => unsat += 1,
                        owl_dl_tableau::hyper::HyperResult::Stalled => stalled += 1,
                    }
                    tot_pts += s.det_or_points;
                    tot_killed += s.det_disjuncts_killed;
                    tot_collapsed += s.det_or_points_collapsed;
                }
            }
            let ratio = if tot_pts > 0 {
                tot_collapsed as f64 / tot_pts as f64
            } else {
                0.0
            };
            println!(
                "AGG ont={rel} sat={sat} unsat={unsat} stalled={stalled} \
                 or_points={tot_pts} killed={tot_killed} collapsed={tot_collapsed} collapse_ratio={ratio:.3}"
            );
            println!("##### END ont={rel} probe_on={on} #####");
        })
        .expect("spawn");
    child.join().expect("thread");
}

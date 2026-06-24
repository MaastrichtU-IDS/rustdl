//! THROWAWAY diagnostic for the precise-merge-deps wine FP (does NOT merge).
//! Sweeps every wine named class with `sat_class_probe` and prints `<verdict>
//! <iri>` per class. Run TWICE and diff the Unsat sets to find the classes that
//! flip Sat(flag-OFF) → Unsat(flag-ON) — the spurious-unsat cluster:
//!
//!   cargo test -p owl-dl-reasoner --test precise_merge_fp_diag -- --ignored --nocapture > /tmp/off.txt
//!   RUSTDL_PRECISE_MERGE_DEPS=1 cargo test ... > /tmp/on.txt
//!   # then: comm -13 <(grep '^Unsat' off|sort) <(grep '^Unsat' on|sort)
//!
//! Adaptive budget OFF + 30s/class so a true verdict is reached (a Stalled
//! means neither — reported as Stalled). 2 GiB stack for deep recursion.

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const WINE: &str = "../../ontologies/real/wine.ofn";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(WINE).unwrap_or_else(|e| panic!("read {WINE}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse wine.ofn");
    ont
}

#[test]
#[ignore = "throwaway precise-merge-deps FP diagnostic; run explicitly with/without RUSTDL_PRECISE_MERGE_DEPS"]
fn precise_merge_fp_wine_class_verdicts() {
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let internal = owl_dl_core::convert::convert_ontology(&ont).expect("convert wine");
            // Collect reportable (non-synthetic) named-class IRIs.
            let mut iris: Vec<String> = internal
                .vocabulary
                .classes()
                .map(|(_, iri)| iri.to_string())
                .filter(|iri| !iri.starts_with("urn:rustdl-") && !iri.contains("rustdl-dkey"))
                .collect();
            iris.sort();
            iris.dedup();
            let flag_on = std::env::var("RUSTDL_PRECISE_MERGE_DEPS").as_deref() == Ok("1");
            eprintln!("# DIAG flag_on={flag_on} classes={}", iris.len());
            let dl = Some(Duration::from_secs(30));
            for iri in &iris {
                match owl_dl_reasoner::sat_class_probe(&ont, iri, 256, dl) {
                    Ok(Some((res, _s, _ms))) => println!("{res:?}\t{iri}"),
                    Ok(None) => println!("NotNamed\t{iri}"),
                    Err(e) => println!("Err({e:?})\t{iri}"),
                }
            }
        })
        .expect("spawn");
    child.join().expect("diag thread");
}

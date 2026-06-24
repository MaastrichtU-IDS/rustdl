//! THROWAWAY det-lookahead viability gate (does NOT merge). Run TWICE:
//!   cargo test -p owl-dl-reasoner --test det_lookahead_gate -- --ignored --nocapture
//!   RUSTDL_DET_LOOKAHEAD_PROBE=1 cargo test -p owl-dl-reasoner --test det_lookahead_gate -- --ignored --nocapture
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

const WINE: &str = "../../ontologies/real/wine.ofn";
const NS: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(WINE).unwrap_or_else(|e| panic!("read {WINE}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse wine.ofn")
        .0
}

#[test]
#[ignore = "throwaway det-lookahead viability gate; run with/without RUSTDL_DET_LOOKAHEAD_PROBE"]
fn det_lookahead_wine_collapse_ratio() {
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let on = std::env::var("RUSTDL_DET_LOOKAHEAD_PROBE").as_deref() == Ok("1");
            println!("##### DET-LOOKAHEAD GATE probe_on={on} #####");
            let dl = Some(Duration::from_secs(60));
            for c in ["Wine", "AlsatianWine", "SweetWine", "Zinfandel"] {
                let iri = format!("{NS}{c}");
                match owl_dl_reasoner::sat_class_probe(&ont, &iri, 256, dl).expect("probe") {
                    Some((res, s, ms)) => println!(
                        "{c:14} verdict={res:?} wall_ms={ms:.0} or_points={} killed={} collapsed={} ratio={:.2}",
                        s.det_or_points, s.det_disjuncts_killed, s.det_or_points_collapsed,
                        if s.det_or_points > 0 { s.det_or_points_collapsed as f64 / s.det_or_points as f64 } else { 0.0 },
                    ),
                    None => println!("{c:14} NOT A NAMED CLASS"),
                }
            }
            println!("##### END probe_on={on} #####");
        })
        .expect("spawn");
    child.join().expect("thread");
}

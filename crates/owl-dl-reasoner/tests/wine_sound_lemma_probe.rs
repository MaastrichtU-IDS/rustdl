#![allow(clippy::unwrap_used, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

#[test]
#[ignore]
fn wine_cf_sound_lemma() {
    unsafe { std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0"); }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let src = std::fs::read_to_string("../../ontologies/real/wine.ofn").unwrap();
            let mut r = Cursor::new(src.into_bytes());
            let ont: SetOntology<RcStr> = read_ofn(&mut r, ParserConfiguration::default()).unwrap().0;
            let cf = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#CabernetFranc";
            let (res, s, wall) =
                owl_dl_reasoner::sat_class_probe(&ont, cf, 256, Some(Duration::from_secs(60)))
                    .unwrap().unwrap();
            println!("WINE CF: {res:?} wall={wall:.0}ms branches={}", s.branches_taken);
            let (te, ci, dci) = (s.total_unsat_exhaust, s.total_ci_unsat, s.distinct_ci_unsat);
            println!("SOUND UNSAT-LEMMA reuse:");
            println!("  total disjunction-exhaust UNSAT  = {te}");
            println!("  context-INDEPENDENT (empty deps) = {ci}  ({:.1}% of exhausts = sound coverage)",
                if te>0 {100.0*ci as f64/te as f64} else {0.0});
            println!("  distinct CI-unsat label sets     = {dci}");
            println!("  SOUND REUSE FACTOR (ci/distinct) = {:.1}x", if dci>0 {ci as f64/dci as f64} else {0.0});
        })
        .unwrap();
    child.join().unwrap();
}

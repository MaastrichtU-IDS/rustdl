#![allow(clippy::unwrap_used, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

#[test]
#[ignore]
fn l1_wine_fold_onoff() {
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
            let mode = if std::env::var_os("RUSTDL_NN_TAINT_DISABLED").is_some() { "FOLD-OFF (nn_taint_disabled)" } else { "FOLD-ON (baseline)" };
            println!("L1 {mode}: {res:?} wall={wall:.0}ms branches={} backjumps={}", s.branches_taken, s.backjumps);
        })
        .unwrap();
    child.join().unwrap();
}

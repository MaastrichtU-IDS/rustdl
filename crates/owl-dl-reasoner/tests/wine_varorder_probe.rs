#![allow(clippy::unwrap_used, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

#[test]
#[ignore]
fn wine_cf_varorder() {
    // SAFETY: serialized single test.
    unsafe {
        std::env::set_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let src = std::fs::read_to_string("../../ontologies/real/wine.ofn").unwrap();
            let mut r = Cursor::new(src.into_bytes());
            let ont: SetOntology<RcStr> = read_ofn(&mut r, ParserConfiguration::default()).unwrap().0;
            let cf = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#CabernetFranc";
            // 30s sample is enough to characterize the branch-decision distribution.
            let (res, s, wall) =
                owl_dl_reasoner::sat_class_probe(&ont, cf, 256, Some(Duration::from_secs(30)))
                    .unwrap()
                    .unwrap();
            println!("WINE CF: {res:?} wall={wall:.0}ms branches={}", s.branches_taken);
            println!("VIABLE HISTOGRAM (v => #decisions branching a disjunction with v viable disjuncts):");
            for (v, c) in s.branch_viable_hist.iter().enumerate() {
                if *c > 0 {
                    println!("  viable={v:<2} : {c}");
                }
            }
        })
        .unwrap();
    child.join().unwrap();
}

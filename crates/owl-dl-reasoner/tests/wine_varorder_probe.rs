#![allow(clippy::unwrap_used, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

#[test]
#[ignore]
fn wine_cf_label_repetition() {
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
            let (res, s, wall) =
                owl_dl_reasoner::sat_class_probe(&ont, cf, 256, Some(Duration::from_secs(60)))
                    .unwrap()
                    .unwrap();
            let total = s.total_branch_labels;
            let distinct = s.distinct_branch_labels;
            let rep = if distinct > 0 { total as f64 / distinct as f64 } else { 0.0 };
            println!("WINE CF: {res:?} wall={wall:.0}ms branches={}", s.branches_taken);
            println!("LABEL REPETITION at branch points:");
            println!("  total branch decisions   = {total}");
            println!("  DISTINCT node-label sets = {distinct}");
            println!("  avg reuse (total/distinct) = {rep:.1}x  (a per-node-label cache would hit ~{:.1}% of the time)",
                if total > 0 { 100.0 * (1.0 - distinct as f64 / total as f64) } else { 0.0 });
        })
        .unwrap();
    child.join().unwrap();
}

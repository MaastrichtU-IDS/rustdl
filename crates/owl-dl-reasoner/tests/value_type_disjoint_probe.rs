//! Throwaway: measure whether value-derived type-disjointness (`RUSTDL_VALUE_TYPE_DISJOINT`)
//! collapses sat(Gamay). Run with `--ignored --nocapture`.
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::doc_markdown,
    clippy::default_trait_access
)]

#[test]
#[ignore = "value-type-disjoint Gamay probe; --ignored --nocapture"]
fn value_type_disjoint_gamay() {
    use horned_owl::io::ofn::reader::read as read_ofn;
    // SAFETY: single ignored test, sets its own diagnostic env var.
    unsafe { std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0") };
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let src =
                std::fs::read_to_string("/data/dumontier/rustdl/ontologies/real/wine.ofn").unwrap();
            let (ont, _): (
                horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
                _,
            ) = read_ofn(
                &mut std::io::Cursor::new(src.into_bytes()),
                Default::default(),
            )
            .unwrap();
            let iri = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#Gamay";
            let t = Some(std::time::Duration::from_secs(30));
            let out = owl_dl_reasoner::sat_class_probe(&ont, iri, 256, t)
                .expect("probe ok")
                .expect("IRI resolves");
            let (result, stats, wall_ms) = out;
            println!(
                "sat(Gamay): {result:?} wall={wall_ms:.0}ms branches={} (disj={} merge={})",
                stats.branches_taken, stats.disj_branches, stats.merge_branches
            );
        })
        .expect("spawn");
    child.join().expect("thread");
}

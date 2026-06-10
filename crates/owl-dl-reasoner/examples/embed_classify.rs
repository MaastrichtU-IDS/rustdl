//! Minimal **in-process** embedding of the rustdl reasoner — classification as
//! a function call, with no subprocess, no JVM, no external Konclude process,
//! and no license. This is the embeddability demo: link `owl-dl-reasoner`, call
//! `classify`, read the hierarchy back, all in the host process.
//!
//! Run:  `cargo run --release -p owl-dl-reasoner --example embed_classify -- <ontology.ofn> [SUB SUP]`
//!
//! Prints time-to-first-result (the in-process latency — no process-spawn or
//! JVM-init overhead at all), the class count, the engine mode, and the
//! soundness/completeness signal (`timed_out_pairs`), plus an optional
//! single-subsumption query.

use std::time::{Duration, Instant};

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: embed_classify <ont.ofn> [SUB SUP]");
    let probe = (args.next(), args.next());

    // Parse (horned-owl). Kept outside the timed region so the number below is
    // the reasoning + result-extraction cost, in-process.
    let src = std::fs::read_to_string(&path).expect("read ontology");
    let mut reader = std::io::Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");

    // THE embedding call: a plain function, in the host process.
    let t = Instant::now();
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(60)).expect("classify");
    let elapsed = t.elapsed();

    let stats = result.stats();
    println!(
        "in-process classify: {:.1} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    println!("classes: {}", result.classes().len());
    println!(
        "complete: {}  (timed_out_pairs = {})",
        stats.timed_out_pairs == 0,
        stats.timed_out_pairs
    );
    if let (Some(sub), Some(sup)) = probe {
        println!("{sub} ⊑ {sup} ? {}", result.is_subclass(&sub, &sup));
    }
}

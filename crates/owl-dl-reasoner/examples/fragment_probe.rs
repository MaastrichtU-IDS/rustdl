//! Print `<path>\t<fragment>` for one ontology — parse + convert + fragment gate
//! only, **no reasoning**.
//!
//! Why this exists: the `# fragment:` banner in `classify` output is written by
//! `write_classification` only *after* classification completes, so diffing it
//! across two binaries costs a full two-arm classify of the corpus, and any DNF
//! yields an empty string — a both-arm DNF then reads as "not a mover" and a
//! one-sided DNF as a false mover. `analyze_fragment` needs no reasoning (cf.
//! the pre-classify call in the CLI), so this probe answers the routing question
//! directly and terminates on ontologies that stall in the engine.
//!
//! ```sh
//! cargo run --release --example fragment_probe -- <file>
//! ```

#![allow(clippy::unwrap_used, clippy::print_stdout)]

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: fragment_probe <file>");
    let src = std::fs::read_to_string(&path).expect("read ontology");
    let onto = read_any(&src);
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
    println!("{path}\t{:?}", owl_dl_reasoner::analyze_fragment(&internal));
}

/// Content-sniffing reader: ORE pool files carry a `.owl` extension but are
/// functional syntax, so dispatching on the extension is wrong here.
fn read_any(src: &str) -> horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr> {
    let first = src
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    let cfg = horned_owl::io::ParserConfiguration::default();
    let mut cursor = std::io::Cursor::new(src.as_bytes());
    if first.starts_with("Prefix(") || first.starts_with("Ontology(") {
        horned_owl::io::ofn::reader::read(&mut cursor, cfg)
            .expect("parse OFN")
            .0
    } else {
        let (o, _) = horned_owl::io::rdf::reader::read(&mut cursor, cfg).expect("parse RDF/XML");
        o.into()
    }
}

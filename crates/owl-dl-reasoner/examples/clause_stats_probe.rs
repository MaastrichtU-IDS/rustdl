//! Print the wedge clausifier's `ClauseStats` for an ontology — in particular
//! **`deferred`**, the count of axioms the clausifier could not represent and
//! silently dropped from the wedge theory.
//!
//! Why this exists: nothing else surfaces it. `# fragment:` folds `deferred`
//! into a three-way verdict that reads `out-of-EL` whether the count is 0 or
//! 50, and `classify --json` does not report it at all — so a wedge theory
//! missing an axiom is indistinguishable from a complete one. A dropped clause
//! only ever weakens the theory, so it cannot cause a false `Unsat`; it causes a
//! `Sat` that `trust_sat` then converts into a silent MISS with
//! `incomplete: false`. That is the D10 shape, and this probe is how you see it.
//!
//! ```sh
//! cargo run --release --example clause_stats_probe -- <file>
//! ```

#![allow(clippy::unwrap_used, clippy::print_stdout)]

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: clause_stats_probe <file>");
    let src = std::fs::read_to_string(&path).expect("read ontology");
    let onto = read_any(&src);
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
    let (clauses, stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    println!(
        "{path}\ttotal={}\thorn={}\tdisjunctive={}\tbottom_headed={}\twith_exists_head={}\tDEFERRED={}\tclauses={}",
        stats.total,
        stats.horn,
        stats.disjunctive,
        stats.bottom_headed,
        stats.with_exists_head,
        stats.deferred,
        clauses.len()
    );
}

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

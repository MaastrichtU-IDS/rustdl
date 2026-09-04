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
    // Match-plan refusal census, computed WITHOUT running the engine (#90).
    //
    // The refusal is a pure function of the clause bodies, so parse+convert+clausify
    // is sufficient — which is the whole point: the engine-side probe
    // (`RUSTDL_TRACE_BODY_VARS=1` on `classify`) cannot report on an ontology that
    // stalls in reasoning, and those are exactly the ones left unmeasured.
    //
    // This REPLICATES `hyper::eval_order`'s greedy tree walk, so it is a drift
    // hazard. Validate it against a known value before trusting a population run:
    // `ro.ofn` must report `filters=6`, matching the 6 `NotTree` refusals the engine
    // probe reports on the pre-fix binary.
    let (filters, disconnected) = refusal_census(&clauses);
    println!("{path}\tFILTERS={filters}\tDISCONNECTED={disconnected}");
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

/// For each clause body, run the same greedy tree walk `hyper::eval_order` runs and
/// count (a) role atoms whose endpoints are BOTH bound when reached — the atoms the
/// #90 fix turns from a whole-clause refusal into a filter — and (b) bodies with an
/// endpoint unreachable from `X`.
fn refusal_census(clauses: &[owl_dl_core::clause::DlClause]) -> (usize, usize) {
    use owl_dl_core::clause::{Atom, Var, X};
    let mut filters = 0usize;
    let mut disconnected = 0usize;
    for cl in clauses {
        let roles: Vec<(Var, Var)> = cl
            .body
            .iter()
            .filter_map(|a| match a {
                Atom::Role(_, u, v) => Some((*u, *v)),
                _ => None,
            })
            .collect();
        let mut bound: Vec<Var> = vec![X];
        let mut used = vec![false; roles.len()];
        let mut placed = 0usize;
        loop {
            let mut progressed = false;
            for (i, (u, v)) in roles.iter().enumerate() {
                if used[i] || !bound.contains(u) {
                    continue;
                }
                used[i] = true;
                placed += 1;
                progressed = true;
                if bound.contains(v) {
                    filters += 1; // both endpoints bound: a CHECK, not a tree edge
                } else {
                    bound.push(*v);
                }
            }
            if placed == roles.len() {
                break;
            }
            if !progressed {
                disconnected += 1;
                break;
            }
        }
    }
    (filters, disconnected)
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

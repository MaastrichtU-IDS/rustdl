//! Measure the `ABox`-saturation inconsistency pre-check **in isolation**, and
//! print the cheap structural predictors alongside it.
//!
//! Why this exists rather than a subtraction of two classify walls: the
//! pre-check **short-circuits** the rest of classify (a clash returns
//! `classify_inconsistent` immediately), so `with − without` measures
//! "pre-check − the classify it replaced", not the pre-check. That subtraction
//! is what produced the retracted "`family.ofn` needs ~2.0 s" figure; the real
//! cost is ~2.65 s. See `docs/2026-08-03-adaptive-inconsistency-budget.md`.
//!
//! ```sh
//! cargo run --release --example abox_precheck_probe -- <file> [budget_ms]
//! ```
//!
//! One CSV row per invocation on stdout (header with `--header`):
//! `file,individuals,class_assertions,opa,chain_rules,transitive_roles,`
//! `functional_roles,axioms,convert_ms,precheck_ms,clash,timed_out,edges,type_adds,edge_adds`
//!
//! `budget_ms` (default 0 = unbounded) bounds the pre-check exactly the way the
//! classify path does, so a probe on a runaway `ABox` can still be capped.

#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

use owl_dl_core::ontology::{Axiom, InternalOntology, SubRolePath};
use std::time::{Duration, Instant};

/// The cheap structural predictors, all readable from the lowered
/// `InternalOntology` without running anything.
struct Predictors {
    individuals: usize,
    class_assertions: usize,
    opa: usize,
    chain_rules: usize,
    transitive_roles: usize,
    functional_roles: usize,
    /// TOTAL lowered axiom count. Added after the first scan: the fixpoint has a
    /// pre-indexing prelude that walks EVERY axiom, so an ontology can be
    /// expensive with an entirely trivial `ABox` (`ore_ont_5368`: 0 type and 0
    /// edge additions, ≥5.9 s).
    axioms: usize,
}

fn predictors(internal: &InternalOntology) -> Predictors {
    let mut inds = std::collections::HashSet::new();
    let mut class_assertions = 0usize;
    let mut opa = 0usize;
    let mut chain_rules = 0usize;
    let mut transitive_roles = 0usize;
    let mut functional_roles = 0usize;

    for ax in &internal.axioms {
        match ax {
            Axiom::ClassAssertion { individual, .. } => {
                class_assertions += 1;
                inds.insert(*individual);
            }
            Axiom::ObjectPropertyAssertion {
                subject, object, ..
            }
            | Axiom::NegativeObjectPropertyAssertion {
                subject, object, ..
            } => {
                opa += 1;
                inds.insert(*subject);
                inds.insert(*object);
            }
            Axiom::SameIndividual(v) | Axiom::DifferentIndividuals(v) => {
                for i in v {
                    inds.insert(*i);
                }
            }
            Axiom::DeclareNamedIndividual(i) => {
                inds.insert(*i);
            }
            Axiom::SubObjectPropertyOf { sub, .. } => {
                if matches!(sub, SubRolePath::Chain(_)) {
                    chain_rules += 1;
                }
            }
            Axiom::TransitiveRole(_) => transitive_roles += 1,
            Axiom::FunctionalRole(_) | Axiom::InverseFunctionalRole(_) => functional_roles += 1,
            _ => {}
        }
    }

    Predictors {
        individuals: inds.len(),
        class_assertions,
        opa,
        chain_rules,
        transitive_roles,
        functional_roles,
        axioms: internal.axioms.len(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--header") {
        println!(
            "file,individuals,class_assertions,opa,chain_rules,transitive_roles,\
             functional_roles,axioms,convert_ms,precheck_ms,clash,timed_out,edges,type_adds,edge_adds"
        );
        if args.len() == 1 {
            return;
        }
    }
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .expect("usage: abox_precheck_probe <file> [budget_ms]");
    let budget_ms: u64 = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let src = std::fs::read_to_string(path).expect("read ontology");
    let t0 = Instant::now();
    let onto = read_any(&src);
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
    let convert_ms = t0.elapsed().as_millis();

    let p = predictors(&internal);

    let deadline = (budget_ms > 0).then(|| Instant::now() + Duration::from_millis(budget_ms));
    let t1 = Instant::now();
    let res =
        owl_dl_reasoner::abox_saturation::saturate_abox_consistency_bounded(&internal, deadline);
    let precheck_ms = t1.elapsed().as_millis();

    let name = std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        name,
        p.individuals,
        p.class_assertions,
        p.opa,
        p.chain_rules,
        p.transitive_roles,
        p.functional_roles,
        p.axioms,
        convert_ms,
        precheck_ms,
        res.clash,
        res.timed_out,
        res.edges.len(),
        res.type_additions,
        res.edge_additions
    );
}

/// Minimal content sniff — OFN if the first meaningful line opens with
/// `Prefix(` / `Ontology(`, else RDF/XML. Mirrors the CLI's `detect_format`
/// for the two formats this corpus uses.
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

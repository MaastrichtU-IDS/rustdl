//! Benchmark harness for rustdl.
//!
//! Two modes:
//!
//! - `bench classify FILE`: parse `FILE` as an OWL functional-syntax
//!   ontology, run `classify`, print the orchestrator stats plus
//!   wall-clock timing.
//! - `bench synthetic-el [--classes N] [--chain-depth D]`: generate
//!   a synthetic EL partonomy of `N` classes connected by a
//!   transitive `partOf` chain of depth `D`, run `classify`, print
//!   stats + timing. Useful as a baseline for the saturation
//!   engine without leaning on any external corpus.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

#[derive(Parser, Debug)]
#[command(name = "owl-dl-bench", version, about = "rustdl benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Classify the given .ofn ontology and print stats + timing.
    Classify {
        /// Path to the OWL functional-syntax ontology.
        file: PathBuf,
    },
    /// Generate a synthetic EL chain ontology in memory and classify
    /// it. Useful as a baseline for the saturation engine on inputs
    /// of controlled shape and size.
    SyntheticEl {
        /// Total number of leaf classes in the chain.
        #[arg(long, default_value = "50")]
        classes: usize,
        /// Anchor class with a trigger axiom at the end of the chain.
        /// Always 1 currently; here for forward-compat with multi-tag
        /// variants.
        #[arg(long, default_value = "1")]
        anchors: usize,
    },
    /// Walk a directory of .ofn ontologies, classify each, report
    /// per-file and aggregate stats. Used to see how the
    /// saturation/tableau orchestrator behaves on a real fixture
    /// corpus.
    Corpus {
        /// Directory containing .ofn ontologies.
        dir: PathBuf,
        /// Suppress per-file output; only print the aggregate
        /// summary at the end.
        #[arg(long)]
        quiet: bool,
    },
}

fn parse_ofn(path: &Path) -> Result<SetOntology<RcStr>> {
    let file =
        File::open(path).with_context(|| format!("opening ontology file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let (ontology, _prefixes) = read(&mut reader, ParserConfiguration::default())
        .map_err(|e| anyhow::anyhow!("parsing OFN ontology {}: {e}", path.display()))?;
    Ok(ontology)
}

/// Generate a synthetic EL partonomy:
///
/// ```text
/// Declaration(Class(:C0)), …, Declaration(Class(:C{N-1}))
/// Declaration(Class(:Anchor))
/// Declaration(ObjectProperty(:partOf))
/// TransitiveObjectProperty(:partOf)
/// SubClassOf(:C{i} ObjectSomeValuesFrom(:partOf :C{i+1}))    for i = 0..N-2
/// SubClassOf(ObjectSomeValuesFrom(:partOf :C{N-1}) :Anchor)
/// ```
///
/// This is the canonical "partOf chain" shape — every Cᵢ should
/// classify as `Anchor` purely via saturation (chain rule + range
/// trigger). Tableau calls measure the orchestrator's overhead on
/// the non-subsumed pairs.
fn synthetic_el_ontology(num_classes: usize, _anchors: usize) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    s.push_str("Prefix(:=<http://bench.test/>)\n");
    s.push_str("Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n");
    s.push_str("Ontology(<http://bench.test/synth>\n");
    s.push_str("    Declaration(ObjectProperty(:partOf))\n");
    s.push_str("    TransitiveObjectProperty(:partOf)\n");
    s.push_str("    Declaration(Class(:Anchor))\n");
    for i in 0..num_classes {
        let _ = writeln!(s, "    Declaration(Class(:C{i}))");
    }
    for i in 0..num_classes - 1 {
        let j = i + 1;
        let _ = writeln!(
            s,
            "    SubClassOf(:C{i} ObjectSomeValuesFrom(:partOf :C{j}))"
        );
    }
    let last = num_classes - 1;
    let _ = writeln!(
        s,
        "    SubClassOf(ObjectSomeValuesFrom(:partOf :C{last}) :Anchor)"
    );
    s.push_str(")\n");
    s
}

fn run_classify(ontology: &SetOntology<RcStr>) -> Result<()> {
    let start = Instant::now();
    let h = classify(ontology).context("classify")?;
    let elapsed = start.elapsed();
    let stats = h.stats();
    println!("classes: {}", h.classes().len());
    println!(
        "subsumption: saturation={} tableau={}",
        stats.saturation_subsumption_hits, stats.tableau_subsumption_calls
    );
    println!(
        "satisfiability probes: saturation={} tableau={}",
        stats.saturation_unsat_hits, stats.tableau_unsat_calls
    );
    println!("unsat classes: {}", h.unsatisfiable_classes().len());
    println!("elapsed: {elapsed:.3?}");
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Classify { file } => {
            let onto = parse_ofn(&file)?;
            run_classify(&onto)?;
        }
        Command::SyntheticEl { classes, anchors } => {
            anyhow::ensure!(classes >= 2, "need at least 2 classes for the chain");
            let src = synthetic_el_ontology(classes, anchors);
            let mut reader = std::io::Cursor::new(src);
            let (onto, _): (SetOntology<RcStr>, _) =
                read(&mut reader, ParserConfiguration::default())
                    .map_err(|e| anyhow::anyhow!("synthesised ontology failed to parse: {e}"))?;
            run_classify(&onto)?;
        }
        Command::Corpus { dir, quiet } => run_corpus(&dir, quiet)?,
    }
    Ok(())
}

fn run_corpus(dir: &Path, quiet: bool) -> Result<()> {
    let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().and_then(|s| s.to_str()) == Some("ofn")
        })
        .map(|e| e.path().to_owned())
        .collect();
    paths.sort();
    let total = paths.len();
    let mut total_classes = 0usize;
    let mut total_pure_el = 0usize;
    let mut total_sat_sub = 0usize;
    let mut total_tab_sub = 0usize;
    let mut total_sat_unsat = 0usize;
    let mut total_tab_unsat = 0usize;
    let mut total_elapsed = std::time::Duration::ZERO;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    for path in &paths {
        match parse_ofn(path).and_then(|onto| {
            let start = Instant::now();
            let h = classify(&onto).context("classify")?;
            Ok((h, start.elapsed()))
        }) {
            Ok((h, elapsed)) => {
                let stats = h.stats();
                if !quiet {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                    println!(
                        "{:50} classes={:4} mode={:6} sat-sub={:5} tab-sub={:5} sat-unsat={:3} tab-unsat={:3} {:>9.3?}",
                        name,
                        h.classes().len(),
                        if stats.pure_el_mode { "EL" } else { "hybrid" },
                        stats.saturation_subsumption_hits,
                        stats.tableau_subsumption_calls,
                        stats.saturation_unsat_hits,
                        stats.tableau_unsat_calls,
                        elapsed,
                    );
                }
                total_classes += h.classes().len();
                if stats.pure_el_mode {
                    total_pure_el += 1;
                }
                total_sat_sub += stats.saturation_subsumption_hits;
                total_tab_sub += stats.tableau_subsumption_calls;
                total_sat_unsat += stats.saturation_unsat_hits;
                total_tab_unsat += stats.tableau_unsat_calls;
                total_elapsed += elapsed;
            }
            Err(e) => failures.push((path.clone(), format!("{e:#}"))),
        }
    }
    println!();
    println!("# corpus summary");
    println!(
        "  files: {total}   successful: {ok}   failures: {fail}",
        ok = total - failures.len(),
        fail = failures.len()
    );
    println!("  classes (sum): {total_classes}");
    println!("  pure-EL files: {total_pure_el} / {total}");
    println!("  subsumption queries: saturation={total_sat_sub} tableau={total_tab_sub}");
    println!("  satisfiability probes: saturation={total_sat_unsat} tableau={total_tab_unsat}");
    println!("  wall clock (sum): {total_elapsed:.3?}");
    if !failures.is_empty() {
        println!();
        println!("# failures");
        for (path, msg) in failures {
            println!("  {}: {msg}", path.display());
        }
    }
    Ok(())
}

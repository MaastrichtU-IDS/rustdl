//! `rustdl` command-line interface.
//!
//! Subcommands map 1:1 to the public reasoner API:
//! - `consistent FILE`                — `is_consistent`
//! - `sat FILE CLASS_IRI`             — `is_class_satisfiable`
//! - `subclass FILE SUB SUP`          — `is_subclass_of`
//! - `classify FILE`                  — `classify`
//!
//! All commands parse one OWL functional-syntax (`.ofn`) ontology
//! from disk via horned-owl. Verdicts go to stdout; tracing/logging
//! goes to stderr.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    Classification, Realization, classify, classify_n2, classify_n2_with_timeout,
    classify_saturation_only, classify_with_timeout, instances_of, is_class_satisfiable,
    is_consistent, is_instance_of, is_subclass_of, is_subclass_of_with_stats, realize,
    realize_saturation_only,
};

#[derive(Parser, Debug)]
#[command(name = "rustdl", version, about = "OWL DL reasoner (rustdl)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Print version information and exit.
    #[arg(long)]
    info: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Decide whether the input ontology is consistent (has any model).
    Consistent {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Decide whether a named class is satisfiable in the ontology.
    Sat {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the class to test.
        class_iri: String,
    },
    /// Decide whether SUB ⊑ SUP is entailed by the ontology.
    Subclass {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the sub-class.
        sub: String,
        /// Full IRI of the super-class.
        sup: String,
    },
    /// Compute the full class hierarchy of the ontology.
    Classify {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Optional per-pair tableau timeout in milliseconds. Pairs
        /// exceeding the budget default to `not subsumed` (sound
        /// under-approximation) and are counted in the output stats.
        /// Useful for diagnosing pathological tableau queries on
        /// SROIQ-heavy ontologies.
        #[arg(long)]
        pair_timeout_ms: Option<u64>,
        /// Deprecated no-op: top-down classification is now the
        /// default. Flag is retained so existing scripts keep
        /// working. To get the legacy `n²` pair-loop behaviour
        /// (useful for benchmarking only), pass `--n2-classify`.
        #[arg(long, hide = true)]
        top_down: bool,
        /// Use the legacy `n²` pairwise classifier instead of the
        /// default top-down path. Strictly slower on every workload
        /// measured (pizza, family, RO, SIO, GO); kept available for
        /// benchmarking and regression cross-checks.
        #[arg(long)]
        n2_classify: bool,
        /// Skip every tableau probe and report only the hierarchy
        /// derivable from the EL saturation closure. Returns a
        /// sound under-approximation — every reported subsumption
        /// is real, but subsumptions that need tableau reasoning
        /// (cardinality, disjunction-with-clash, nominal merges,
        /// …) are missed. On large mostly-EL workloads (SIO, GO,
        /// SULO) this is dramatically faster — SIO drops from
        /// ~270 s to a few seconds while losing < 0.1% of
        /// subsumptions. Not recommended on SROIQ-heavy inputs
        /// (pizza loses ~20 %).
        #[arg(long)]
        saturation_only: bool,
    },
    /// Decide whether INDIVIDUAL is provably an instance of CLASS.
    Instance {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the class.
        class_iri: String,
        /// Full IRI of the individual.
        individual_iri: String,
    },
    /// List every individual provably in CLASS.
    Instances {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the class.
        class_iri: String,
    },
    /// Realize the ontology: per-individual most-specific entailed types.
    Realize {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Skip every tableau probe (both classify and per-individual
        /// instance check) and report only the type assignments
        /// derivable from the EL saturation closure + told class
        /// assertions. Sound under-approximation — symmetric to the
        /// `classify --saturation-only` flag.
        #[arg(long)]
        saturation_only: bool,
    },
    /// Decide SUB ⊑ SUP and report which engine (EL saturation or
    /// tableau) produced the verdict. Useful for understanding
    /// orchestrator behaviour on real ontologies.
    Explain {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the sub-class.
        sub: String,
        /// Full IRI of the super-class.
        sup: String,
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

fn print_classification(h: &Classification) {
    let stdout = std::io::stdout();
    let mut out = BufWriter::with_capacity(1 << 16, stdout.lock());
    let _ = write_classification(&mut out, h);
    let _ = out.flush();
}

fn write_classification<W: Write>(out: &mut W, h: &Classification) -> std::io::Result<()> {
    let classes = h.classes();
    let stats = h.stats();
    writeln!(out, "# classes: {}", classes.len())?;
    writeln!(
        out,
        "# mode: {}",
        if stats.pure_el_mode {
            "pure EL (saturation-only)"
        } else {
            "hybrid (saturation + tableau)"
        }
    )?;
    writeln!(
        out,
        "# subsumption: saturation={} tableau={}",
        stats.saturation_subsumption_hits, stats.tableau_subsumption_calls
    )?;
    writeln!(
        out,
        "# satisfiability probes: saturation={} tableau={}",
        stats.saturation_unsat_hits, stats.tableau_unsat_calls
    )?;
    if stats.timed_out_pairs > 0 {
        writeln!(
            out,
            "# timed-out pairs: {} (defaulted to not-subsumed)",
            stats.timed_out_pairs
        )?;
    }
    let unsat = h.unsatisfiable_classes();
    if !unsat.is_empty() {
        writeln!(out, "# unsatisfiable: {}", unsat.len())?;
        for iri in unsat {
            writeln!(out, "unsat\t{iri}")?;
        }
    }
    // Equivalence groups: print each non-trivial group once.
    let mut printed: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in classes {
        if printed.contains(c.as_str()) {
            continue;
        }
        let equivs = h.equivalent_classes(c);
        if equivs.len() > 1 {
            writeln!(out, "equiv\t{}", equivs.join("\t"))?;
            for iri in &equivs {
                printed.insert(iri);
            }
        }
    }
    // Direct edges.
    for c in classes {
        let directs = h.direct_subsumers(c);
        for sup in directs {
            writeln!(out, "direct\t{c}\t{sup}")?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    if cli.info {
        println!("rustdl {}", env!("CARGO_PKG_VERSION"));
        println!("OWL 2 DL reasoner; SROIQ surface implemented, EL saturation pending.");
        return Ok(());
    }

    let Some(command) = cli.command else {
        println!("rustdl — use --help to see commands, or --info for build info.");
        return Ok(());
    };

    match command {
        Command::Consistent { file } => {
            let onto = parse_ofn(&file)?;
            let verdict = is_consistent(&onto).context("is_consistent")?;
            println!(
                "{}",
                if verdict {
                    "consistent"
                } else {
                    "inconsistent"
                }
            );
        }
        Command::Sat { file, class_iri } => {
            let onto = parse_ofn(&file)?;
            let verdict =
                is_class_satisfiable(&onto, &class_iri).context("is_class_satisfiable")?;
            println!("{}", if verdict { "sat" } else { "unsat" });
        }
        Command::Subclass { file, sub, sup } => {
            let onto = parse_ofn(&file)?;
            let verdict = is_subclass_of(&onto, &sub, &sup).context("is_subclass_of")?;
            println!("{}", if verdict { "yes" } else { "no" });
        }
        Command::Classify {
            file,
            pair_timeout_ms,
            top_down: _,
            n2_classify,
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            let timeout = pair_timeout_ms.map(std::time::Duration::from_millis);
            let h = if saturation_only {
                classify_saturation_only(&onto).context("classify_saturation_only")?
            } else {
                match (n2_classify, timeout) {
                    (true, Some(t)) => {
                        classify_n2_with_timeout(&onto, t).context("classify_n2_with_timeout")?
                    }
                    (true, None) => classify_n2(&onto).context("classify_n2")?,
                    (false, Some(t)) => {
                        classify_with_timeout(&onto, t).context("classify_with_timeout")?
                    }
                    (false, None) => classify(&onto).context("classify")?,
                }
            };
            print_classification(&h);
        }
        Command::Instance {
            file,
            class_iri,
            individual_iri,
        } => {
            let onto = parse_ofn(&file)?;
            let verdict =
                is_instance_of(&onto, &class_iri, &individual_iri).context("is_instance_of")?;
            println!("{}", if verdict { "yes" } else { "no" });
        }
        Command::Instances { file, class_iri } => {
            let onto = parse_ofn(&file)?;
            let members = instances_of(&onto, &class_iri).context("instances_of")?;
            for iri in members {
                println!("{iri}");
            }
        }
        Command::Realize {
            file,
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            let r = if saturation_only {
                realize_saturation_only(&onto).context("realize_saturation_only")?
            } else {
                realize(&onto).context("realize")?
            };
            print_realization(&r);
        }
        Command::Explain { file, sub, sup } => {
            let onto = parse_ofn(&file)?;
            let (verdict, stats) = is_subclass_of_with_stats(&onto, &sub, &sup)
                .context("is_subclass_of_with_stats")?;
            let answered_by = if stats.answered_by_saturation {
                "saturation"
            } else {
                "tableau"
            };
            let completeness = if stats.pure_el_mode {
                " (input is pure EL; closure is complete)"
            } else if stats.answered_by_saturation {
                " (closure produced a positive witness)"
            } else {
                " (closure didn't witness it; tableau adjudicated)"
            };
            println!(
                "{sub} ⊑ {sup} : {answer} — answered by {answered_by}{completeness}",
                answer = if verdict { "yes" } else { "no" },
            );
        }
    }
    Ok(())
}

fn print_realization(r: &Realization) {
    for individual in r.individuals() {
        let leaves = r.most_specific_types(individual);
        if leaves.is_empty() {
            continue;
        }
        println!("{individual}\t{}", leaves.join("\t"));
    }
}

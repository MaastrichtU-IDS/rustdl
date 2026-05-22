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
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    Classification, Realization, classify, instances_of, is_class_satisfiable, is_consistent,
    is_instance_of, is_subclass_of, realize,
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
    let classes = h.classes();
    let stats = h.stats();
    println!("# classes: {}", classes.len());
    println!(
        "# mode: {}",
        if stats.pure_el_mode {
            "pure EL (saturation-only)"
        } else {
            "hybrid (saturation + tableau)"
        }
    );
    println!(
        "# subsumption: saturation={} tableau={}",
        stats.saturation_subsumption_hits, stats.tableau_subsumption_calls
    );
    println!(
        "# satisfiability probes: saturation={} tableau={}",
        stats.saturation_unsat_hits, stats.tableau_unsat_calls
    );
    let unsat = h.unsatisfiable_classes();
    if !unsat.is_empty() {
        println!("# unsatisfiable: {}", unsat.len());
        for iri in unsat {
            println!("unsat\t{iri}");
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
            println!("equiv\t{}", equivs.join("\t"));
            for iri in &equivs {
                printed.insert(iri);
            }
        }
    }
    // Direct edges.
    for c in classes {
        let directs = h.direct_subsumers(c);
        for sup in directs {
            println!("direct\t{c}\t{sup}");
        }
    }
}

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
        Command::Classify { file } => {
            let onto = parse_ofn(&file)?;
            let h = classify(&onto).context("classify")?;
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
        Command::Realize { file } => {
            let onto = parse_ofn(&file)?;
            let r = realize(&onto).context("realize")?;
            print_realization(&r);
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

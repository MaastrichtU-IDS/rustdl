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
    classify_saturation_only, classify_with_timeout, instances_of, instances_of_saturation_only,
    is_class_satisfiable, is_consistent, is_instance_of, is_instance_of_saturation_only,
    is_subclass_of, is_subclass_of_saturation_only, is_subclass_of_with_stats, realize,
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
        /// Skip the `sub ⊓ ¬sup` tableau probe and answer only from
        /// the EL closure. Sound under-approximation: a `yes` is
        /// genuine; `no` may be a missed positive that the full
        /// classifier would detect.
        #[arg(long)]
        saturation_only: bool,
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
        /// Skip the `{a} ⊓ ¬C` tableau probe and answer only from
        /// told class assertions + the EL saturation closure.
        /// Sound under-approximation: a `yes` is genuine; `no` may
        /// be a missed positive that the full classifier would
        /// detect.
        #[arg(long)]
        saturation_only: bool,
    },
    /// List every individual provably in CLASS.
    Instances {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Full IRI of the class.
        class_iri: String,
        /// Skip every per-individual tableau probe; list only the
        /// individuals the EL closure proves are members. Sound
        /// under-approximation. Counterpart to
        /// `classify --saturation-only` for `ABox` queries.
        #[arg(long)]
        saturation_only: bool,
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
    /// Print signature-locality statistics: number of classes,
    /// number of connected components in the co-occurrence graph,
    /// and the size of the largest component. Diagnostic for the
    /// module-extraction pre-filter (see
    /// `docs/module-extraction-plan.md`).
    LocalityStats {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Print absorbed-TBox statistics: rule counts and the
    /// residual-GCI shape breakdown. Diagnostic for the lazy-
    /// unfolding plan (see `docs/lazy-unfolding-plan.md`).
    TboxStats {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Classify each residual GCI by its lazy-unfolding trigger
    /// (`Eager` / `DeferOr` / `DeferNot` / `DeferAll` / `EagerExists`) and
    /// print the histogram. Bounds the expected win from
    /// lazy-unfolding Phase 2 — see `docs/lazy-unfolding-plan.md`.
    ResidualTriggers {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Print DL-clause shape statistics (hypertableau Phase H0):
    /// total clauses, Horn vs disjunctive, ⊥-headed, ∃-headed,
    /// and deferred (constructs the H0 clausifier doesn't yet
    /// handle). See `docs/hypertableau-scoping.md`.
    ClauseStats {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Hypertableau Phase H2b wall probe: run the hyperresolution
    /// engine's concept-satisfiability decision once per named class
    /// and report timing + branching. NOTE: a *performance probe*,
    /// not a correctness claim — the clausifier defers
    /// cardinality/nominals, so `Sat` is not sound for the full
    /// ontology (`Unsat` is). See `docs/hypertableau-scoping.md`.
    HyperSat {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Max branching-recursion depth.
        #[arg(long, default_value_t = 256)]
        depth: usize,
        /// Per-class wall budget in ms (0 = unbounded).
        #[arg(long, default_value_t = 5000)]
        per_class_timeout_ms: u64,
    },
    /// Hypertableau Phase H2c wall probe: decide every ordered
    /// class-pair subsumption via the hyperresolution engine (¬B
    /// injection) and report timing + branching. This reaches the
    /// pizza wall that bare `hyper-sat` does not. Same probe caveat:
    /// an entailed (`Unsat`) verdict is sound for the full ontology,
    /// "not subsumed" is not. See `docs/hypertableau-scoping.md` §H2c.
    HyperClassifyProbe {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
        /// Max branching-recursion depth.
        #[arg(long, default_value_t = 256)]
        depth: usize,
        /// Per-pair wall budget in ms (0 = unbounded).
        #[arg(long, default_value_t = 5000)]
        per_pair_timeout_ms: u64,
        /// Print every entailed (`Unsat`) subsumption as a `sub\tsup`
        /// TSV line (prefixed `S\t`) for set comparison against a
        /// reference reasoner's hierarchy closure.
        #[arg(long)]
        dump_subsumptions: bool,
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
        Command::Subclass {
            file,
            sub,
            sup,
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            let verdict = if saturation_only {
                is_subclass_of_saturation_only(&onto, &sub, &sup)
                    .context("is_subclass_of_saturation_only")?
            } else {
                is_subclass_of(&onto, &sub, &sup).context("is_subclass_of")?
            };
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
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            let verdict = if saturation_only {
                is_instance_of_saturation_only(&onto, &class_iri, &individual_iri)
                    .context("is_instance_of_saturation_only")?
            } else {
                is_instance_of(&onto, &class_iri, &individual_iri).context("is_instance_of")?
            };
            println!("{}", if verdict { "yes" } else { "no" });
        }
        Command::Instances {
            file,
            class_iri,
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            let members = if saturation_only {
                instances_of_saturation_only(&onto, &class_iri)
                    .context("instances_of_saturation_only")?
            } else {
                instances_of(&onto, &class_iri).context("instances_of")?
            };
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
        Command::TboxStats { file } => {
            let onto = parse_ofn(&file)?;
            let stats = owl_dl_reasoner::tbox_stats(&onto).context("tbox_stats")?;
            println!("# concept_rules:        {}", stats.concept_rules);
            println!("# nominal_rules:        {}", stats.nominal_rules);
            println!("# role_rules_guarded:   {}", stats.role_rules_guarded);
            println!("# role_rules_unguarded: {}", stats.role_rules_unguarded);
            println!("# residual_gcis:        {}", stats.residual_gcis);
            println!("#   residual_or:        {}", stats.residual_or_count);
            println!("#   residual_atomic:    {}", stats.residual_atomic_count);
            println!("#   residual_other:     {}", stats.residual_other_count);
            println!("# concept_rule_or:      {}", stats.concept_rule_or_count);
        }
        Command::ResidualTriggers { file } => {
            let onto = parse_ofn(&file)?;
            let stats =
                owl_dl_reasoner::residual_trigger_stats(&onto).context("residual_trigger_stats")?;
            println!("# residuals_total:    {}", stats.total);
            println!("# eager:              {}", stats.eager);
            println!("# defer_or:           {}", stats.defer_or);
            println!("# defer_not:          {}", stats.defer_not);
            println!("# defer_all:          {}", stats.defer_all);
            println!(
                "# eager_∃_cardinal:   {}",
                stats.eager_exists_or_cardinality
            );
            println!("# deferred_total:     {}", stats.deferred());
            #[allow(clippy::cast_precision_loss)]
            let frac = if stats.total == 0 {
                0.0
            } else {
                stats.deferred() as f64 / stats.total as f64
            };
            println!("# deferred_fraction:  {:.1}%", frac * 100.0);
        }
        Command::ClauseStats { file } => {
            let onto = parse_ofn(&file)?;
            let stats = owl_dl_reasoner::clause_stats(&onto).context("clause_stats")?;
            println!("# clauses_total:    {}", stats.total);
            println!("# horn:             {}", stats.horn);
            println!("# disjunctive:      {}", stats.disjunctive);
            println!("# bottom_headed:    {}", stats.bottom_headed);
            println!("# with_exists_head: {}", stats.with_exists_head);
            println!("# deferred:         {}", stats.deferred);
        }
        Command::HyperSat {
            file,
            depth,
            per_class_timeout_ms,
        } => {
            use owl_dl_reasoner::HyperResult;
            let onto = parse_ofn(&file)?;
            let timeout = (per_class_timeout_ms > 0)
                .then(|| std::time::Duration::from_millis(per_class_timeout_ms));
            let probe = owl_dl_reasoner::hyper_sat_probe(&onto, depth, timeout)
                .context("hyper_sat_probe")?;
            let cs = &probe.clause_stats;
            println!("# PERFORMANCE PROBE (not a soundness claim):");
            println!(
                "#   clausifier defers {} axiom(s); dropping them only",
                cs.deferred
            );
            println!("#   removes constraints, so Unsat is sound for the full");
            println!("#   ontology but Sat is NOT. See hypertableau-scoping.md §H2b.");
            println!("# clauses_total:    {}", cs.total);
            println!("# disjunctive:      {}", cs.disjunctive);
            println!("# deferred:         {}", cs.deferred);
            println!("# depth_cap:        {depth}");
            println!(
                "# per_class_timeout: {}",
                if per_class_timeout_ms == 0 {
                    "none".to_string()
                } else {
                    format!("{per_class_timeout_ms}ms")
                }
            );

            let n = probe.results.len();
            let (mut sat, mut unsat, mut stalled) = (0u64, 0u64, 0u64);
            // "branched" = a class whose decision actually exercised
            // hypertableau branching (the only ones that say anything
            // about the engine vs. the default's per-class sat).
            let mut branched = 0u64;
            let mut branched_walls: Vec<f64> = Vec::new();
            let mut total_wall = 0.0f64;
            let mut max_depth_reached = 0u32;
            let mut total_branches = 0u64;
            let mut total_match_attempts = 0u64;
            let mut total_node_clones = 0u64;
            let mut total_fixpoint_passes = 0u64;
            for r in &probe.results {
                match r.result {
                    HyperResult::Sat => sat += 1,
                    HyperResult::Unsat => unsat += 1,
                    HyperResult::Stalled => stalled += 1,
                }
                total_wall += r.wall_ms;
                total_branches += r.stats.branches_taken;
                total_match_attempts += r.stats.match_attempts;
                total_node_clones += r.stats.node_clones;
                total_fixpoint_passes += r.stats.fixpoint_passes;
                max_depth_reached = max_depth_reached.max(r.stats.max_branch_depth);
                if r.stats.branches_taken > 0 {
                    branched += 1;
                    branched_walls.push(r.wall_ms);
                }
            }
            println!("# classes:          {n}");
            println!("# sat:              {sat}");
            println!("# unsat:            {unsat}");
            println!("# stalled:          {stalled}");
            println!("# total_wall_ms:    {total_wall:.1}");
            println!("# total_branches:   {total_branches}");
            println!("# max_depth_reached:{max_depth_reached}");
            println!("# --- profiling counters (search-quality work) ---");
            println!("# match_attempts:   {total_match_attempts}  (clause×node Horn match tries)");
            println!("# node_clones:      {total_node_clones}  (save/restore — trail target)");
            println!("# fixpoint_passes:  {total_fixpoint_passes}");
            println!("# classes_branched: {branched}   <-- HEADLINE: only these probe the engine");
            if branched > 0 {
                branched_walls
                    .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let max = branched_walls.last().copied().unwrap_or(0.0);
                let sum: f64 = branched_walls.iter().sum();
                #[allow(clippy::cast_precision_loss)]
                let mean = sum / branched_walls.len() as f64;
                println!("# branched_wall_ms_mean: {mean:.2}");
                println!("# branched_wall_ms_max:  {max:.2}");
            }
            // The slowest / branchiest classes — the interesting tail.
            let mut by_interest: Vec<&owl_dl_reasoner::HyperSatClassResult> =
                probe.results.iter().collect();
            by_interest.sort_by(|a, b| {
                (b.stats.branches_taken, b.wall_ms.to_bits())
                    .cmp(&(a.stats.branches_taken, a.wall_ms.to_bits()))
            });
            println!("# --- top classes by branching ---");
            for r in by_interest
                .iter()
                .take(15)
                .filter(|r| r.stats.branches_taken > 0)
            {
                println!(
                    "#   {:?} wall={:.2}ms branches={} restores={} depth={}  {}",
                    r.result,
                    r.wall_ms,
                    r.stats.branches_taken,
                    r.stats.restores,
                    r.stats.max_branch_depth,
                    r.iri,
                );
            }
        }
        Command::HyperClassifyProbe {
            file,
            depth,
            per_pair_timeout_ms,
            dump_subsumptions,
        } => {
            let onto = parse_ofn(&file)?;
            let timeout = (per_pair_timeout_ms > 0)
                .then(|| std::time::Duration::from_millis(per_pair_timeout_ms));
            let probe = owl_dl_reasoner::hyper_subsumption_probe(&onto, depth, timeout)
                .context("hyper_subsumption_probe")?;
            let cs = &probe.clause_stats;
            println!("# PERFORMANCE PROBE (not a complete classifier):");
            println!(
                "#   clausifier defers {} axiom(s); Unsat (subsumption",
                cs.deferred
            );
            println!("#   holds) is sound for the full ontology, 'not subsumed'");
            println!("#   is NOT. subsumptions is a sound LOWER BOUND on the");
            println!("#   true hierarchy. See hypertableau-scoping.md §H2c.");
            println!("# clauses_total:    {}", cs.total);
            println!("# disjunctive:      {}", cs.disjunctive);
            println!("# deferred:         {}", cs.deferred);
            println!("# depth_cap:        {depth}");
            println!(
                "# per_pair_timeout: {}",
                if per_pair_timeout_ms == 0 {
                    "none".to_string()
                } else {
                    format!("{per_pair_timeout_ms}ms")
                }
            );
            println!("# complements:      {}", probe.complements_introduced);
            println!("# pairs_tested:     {}", probe.pairs_tested);
            println!(
                "# pairs_via_expansion: {}   (H3b ¬sup; rest used bare-complement fallback)",
                probe.pairs_via_expansion
            );
            println!(
                "# subsumptions:     {}   (sound lower bound)",
                probe.subsumptions
            );
            println!(
                "# pairs_branched:   {}   <-- HEADLINE: only these probe the engine",
                probe.pairs_branched
            );
            println!("# stalled:          {}", probe.stalled);
            println!("# max_depth_reached:{}", probe.max_branch_depth);
            println!("# total_wall_ms:    {:.1}", probe.total_wall_ms);
            // Slowest / branchiest pairs — the interesting tail.
            let mut by_interest: Vec<&owl_dl_reasoner::HyperSubResult> = probe
                .results
                .iter()
                .filter(|r| r.stats.branches_taken > 0)
                .collect();
            by_interest.sort_by(|a, b| {
                (b.stats.branches_taken, b.wall_ms.to_bits())
                    .cmp(&(a.stats.branches_taken, a.wall_ms.to_bits()))
            });
            if dump_subsumptions {
                for r in &probe.results {
                    if r.result == owl_dl_reasoner::HyperResult::Unsat {
                        println!("S\t{}\t{}", r.sub, r.sup);
                    }
                }
            }
            println!("# --- top pairs by branching ---");
            for r in by_interest.iter().take(15) {
                println!(
                    "#   {:?} wall={:.2}ms branches={} restores={} depth={}  {} <= {}",
                    r.result,
                    r.wall_ms,
                    r.stats.branches_taken,
                    r.stats.restores,
                    r.stats.max_branch_depth,
                    r.sub,
                    r.sup,
                );
            }
        }
        Command::LocalityStats { file } => {
            let onto = parse_ofn(&file)?;
            let stats = owl_dl_reasoner::locality_stats(&onto).context("locality_stats")?;
            println!("# classes:    {}", stats.num_classes);
            println!("# components: {}", stats.num_components);
            println!("# largest:    {}", stats.largest_component);
            println!("# singletons: {}", stats.singleton_components);
            // Class counts fit comfortably in f64 mantissa (52 bits)
            // for any realistic ontology; the cast is fine here.
            #[allow(clippy::cast_precision_loss)]
            let dominance = if stats.num_classes == 0 {
                0.0
            } else {
                stats.largest_component as f64 / stats.num_classes as f64
            };
            println!("# dominance:  {:.1}%", dominance * 100.0);
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

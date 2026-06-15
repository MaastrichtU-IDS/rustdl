//! Benchmark harness for rustdl.
//!
//! Modes:
//!
//! - `bench classify FILE`: parse `FILE` as an OWL functional-syntax
//!   ontology, run `classify`, print the orchestrator stats plus
//!   wall-clock timing.
//! - `bench synthetic-el [--classes N] [--chain-depth D]`: generate
//!   a synthetic EL partonomy of `N` classes connected by a
//!   transitive `partOf` chain of depth `D`, run `classify`, print
//!   stats + timing. Useful as a baseline for the saturation
//!   engine without leaning on any external corpus.
//! - `bench cb-diff FILE`: run the consequence-based (CB) engine and the
//!   current hybrid engine side-by-side, diff the hierarchies, and print
//!   timing + agreement summary. If the ontology is out of ALCH the CB
//!   engine reports `OutOfFragment` and the diff is skipped.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};

/// Wrap `body` in a pprof CPU profiler when the `profile` feature is
/// enabled and `RUSTDL_PROFILE=path/to/flamegraph.svg` is set. Sampling
/// is signal-based (SIGPROF), so it works without `perf_event_paranoid`
/// changes.
///
/// `RUSTDL_PROFILE_SECONDS` (default 60) bounds the wall-clock budget:
/// after that many seconds the flamegraph is written and the process
/// exits with code 0, killing any still-running work. Useful for
/// workloads like pizza.ofn that don't finish naturally.
///
/// No-op when the env var is unset or the feature is off.
#[cfg(feature = "profile")]
fn with_profile<F: FnOnce() -> Result<()> + Send + 'static>(body: F) -> Result<()> {
    use anyhow::anyhow;
    let Ok(path) = std::env::var("RUSTDL_PROFILE") else {
        return body();
    };
    let secs: u64 = std::env::var("RUSTDL_PROFILE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(199)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|e| anyhow!("pprof start: {e}"))?;
    let handle = std::thread::spawn(body);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    let report = guard
        .report()
        .build()
        .map_err(|e| anyhow!("pprof report: {e}"))?;
    let file =
        std::fs::File::create(&path).with_context(|| format!("creating flamegraph at {path}"))?;
    report
        .flamegraph(file)
        .map_err(|e| anyhow!("flamegraph write: {e}"))?;
    eprintln!("flamegraph written to {path} after {secs}s wall");
    // If the body finished before the timer, prefer its result.
    if handle.is_finished() {
        return handle.join().expect("classify thread panicked");
    }
    // Body still running — write the report and exit cleanly.
    std::process::exit(0);
}

#[cfg(not(feature = "profile"))]
fn with_profile<F: FnOnce() -> Result<()> + Send + 'static>(body: F) -> Result<()> {
    body()
}
use clap::{Parser, Subcommand};
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::CbOutcome;
use owl_dl_core::InternalOntology;
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
    /// Single-class satisfiability probe — useful with
    /// `--features profile` to get a flamegraph of one `decide()` call
    /// without the parallel pair-loop noise.
    Sat {
        /// Path to the OWL functional-syntax ontology.
        file: PathBuf,
        /// Full IRI of the class to probe.
        iri: String,
        /// Optional cooperative wall-clock deadline in milliseconds.
        /// Without it, the probe runs unbounded; with it, the tableau
        /// returns `None` after the budget and we report
        /// `verdict: timeout`. Letting the bench return cleanly is
        /// also what lets the `counters` feature dump its histogram
        /// on Drop — `timeout(1)` SIGTERM kills the process before
        /// the Drop runs, so use this instead.
        #[arg(long)]
        deadline_ms: Option<u64>,
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
        /// Re-run the classification this many times; report `med`,
        /// `min`, and `max` wall-clock instead of a single sample.
        /// Defaults to 1 (single run). Pair with `--repeats 10` to
        /// drown trial-to-trial system noise (~30 % at the ms scale
        /// on a shared 16-core box).
        #[arg(long, default_value = "1")]
        repeats: usize,
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
        /// Re-run each fixture this many times and report the median
        /// elapsed time per file (plus min/max). The aggregate
        /// `wall clock (sum)` line sums medians. Use `--repeats 5`
        /// or higher to drown out trial-to-trial system noise when
        /// bisecting perf changes — a single run on a shared 16-core
        /// machine has ~30% variance at the millisecond scale.
        #[arg(long, default_value = "1")]
        repeats: usize,
    },
    /// In-process comparison against `whelk-rs` (another EL
    /// reasoner in Rust), running both engines on the same input.
    /// Available only when built with `--features whelk-compare`.
    #[cfg(feature = "whelk-compare")]
    CompareWhelk {
        /// Path to the OWL functional-syntax ontology.
        file: PathBuf,
        /// Number of timing iterations (default 5; first is warmup
        /// and dropped).
        #[arg(long, default_value = "5")]
        iters: usize,
    },
    /// Run the CB (consequence-based ALCH) engine and the current hybrid
    /// side-by-side on the same ontology, diff the hierarchies, and print
    /// a timing + agreement summary.
    ///
    /// If the input is outside ALCH (uses `≤n`/`≥n`, inverse roles,
    /// nominals, datatypes, role chains, transitivity) the CB engine
    /// reports `OutOfFragment` and the diff is skipped.
    ///
    /// NOTE: the CB engine is currently a stub (`todo!()` — Task B not yet
    /// integrated). The command compiles and runs the harness; the engine
    /// call will panic until Task B lands.
    CbDiff {
        /// Path to the OWL functional-syntax ontology.
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

/// Like [`run_classify`] but classifies `repeats` times and reports
/// `med` / `min` / `max` wall-clock. Stats are read from the first
/// run since they're deterministic per the convert+sort guarantee.
fn run_classify_repeated(ontology: &SetOntology<RcStr>, repeats: usize) -> Result<()> {
    if repeats <= 1 {
        return run_classify(ontology);
    }
    let mut times: Vec<std::time::Duration> = Vec::with_capacity(repeats);
    let mut first: Option<owl_dl_reasoner::Classification> = None;
    for _ in 0..repeats {
        let start = Instant::now();
        let h = classify(ontology).context("classify")?;
        times.push(start.elapsed());
        if first.is_none() {
            first = Some(h);
        }
    }
    times.sort();
    let median = times[times.len() / 2];
    let min = *times.first().expect("repeats >= 2 ⇒ times non-empty");
    let max = *times.last().expect("repeats >= 2 ⇒ times non-empty");
    let h = first.expect("first present after success loop");
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
    println!("elapsed: med={median:.3?} min={min:.3?} max={max:.3?} (n={repeats})");
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    with_profile(|| run(cli))
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Classify { file } => {
            let onto = parse_ofn(&file)?;
            run_classify(&onto)?;
        }
        Command::Sat {
            file,
            iri,
            deadline_ms,
        } => {
            let onto = parse_ofn(&file)?;
            let start = Instant::now();
            let verdict = match deadline_ms {
                None => Some(
                    owl_dl_reasoner::is_class_satisfiable(&onto, &iri)
                        .context("rustdl sat probe")?,
                ),
                Some(ms) => owl_dl_reasoner::is_class_satisfiable_with_timeout(
                    &onto,
                    &iri,
                    std::time::Duration::from_millis(ms),
                )
                .context("rustdl sat probe")?,
            };
            let elapsed = start.elapsed();
            println!("class: {iri}");
            match verdict {
                Some(true) => println!("verdict: sat"),
                Some(false) => println!("verdict: unsat"),
                None => println!("verdict: timeout"),
            }
            println!("wall: {:.3} ms", elapsed.as_secs_f64() * 1000.0);
        }
        Command::SyntheticEl {
            classes,
            anchors,
            repeats,
        } => {
            anyhow::ensure!(classes >= 2, "need at least 2 classes for the chain");
            let src = synthetic_el_ontology(classes, anchors);
            let mut reader = std::io::Cursor::new(src);
            let (onto, _): (SetOntology<RcStr>, _) =
                read(&mut reader, ParserConfiguration::default())
                    .map_err(|e| anyhow::anyhow!("synthesised ontology failed to parse: {e}"))?;
            run_classify_repeated(&onto, repeats.max(1))?;
        }
        Command::Corpus {
            dir,
            quiet,
            repeats,
        } => run_corpus(&dir, quiet, repeats)?,
        #[cfg(feature = "whelk-compare")]
        Command::CompareWhelk { file, iters } => run_compare_whelk(&file, iters)?,
        Command::CbDiff { file } => {
            let onto = parse_ofn(&file)?;
            let internal = owl_dl_core::convert::convert_ontology(&onto)
                .context("convert_ontology for cb-diff")?;
            let report = cb_diff(&internal).context("cb_diff")?;
            print_diff_report(&report);
        }
    }
    Ok(())
}

#[cfg(feature = "whelk-compare")]
fn run_compare_whelk(path: &Path, iters: usize) -> Result<()> {
    use whelk::whelk::owl::translate_ontology;
    use whelk::whelk::reasoner::assert as whelk_assert;

    anyhow::ensure!(iters >= 2, "need at least 2 iters (first is warmup)");
    let ontology = parse_ofn(path)?;
    println!("file: {}", path.display());
    println!("iters: {iters} (first iteration discarded as warmup)\n");

    // Time rustdl `classify`.
    let mut rustdl_samples: Vec<std::time::Duration> = Vec::with_capacity(iters);
    let mut last_rustdl_stats = None;
    for _ in 0..iters {
        let start = Instant::now();
        let h = classify(&ontology).context("rustdl classify")?;
        let elapsed = start.elapsed();
        last_rustdl_stats = Some((h.classes().len(), h.stats()));
        rustdl_samples.push(elapsed);
    }

    // Time whelk: translate_ontology + assert (the saturation step).
    let mut whelk_samples: Vec<std::time::Duration> = Vec::with_capacity(iters);
    let mut last_whelk_subsumptions: usize = 0;
    for _ in 0..iters {
        let start = Instant::now();
        let translated = translate_ontology(&ontology);
        let state = whelk_assert(&translated);
        let elapsed = start.elapsed();
        last_whelk_subsumptions = state.named_subsumptions().len();
        whelk_samples.push(elapsed);
    }

    // Drop the warmup iteration (first) and report.
    let summary = |label: &str, samples: &[std::time::Duration]| {
        let trimmed = &samples[1..];
        let total: std::time::Duration = trimmed.iter().sum();
        let mean = total / u32::try_from(trimmed.len()).expect("iter count fits");
        let min = trimmed.iter().min().copied().unwrap_or_default();
        let max = trimmed.iter().max().copied().unwrap_or_default();
        println!("{label:<10} mean={mean:>9.3?}  min={min:>9.3?}  max={max:>9.3?}");
    };
    summary("rustdl", &rustdl_samples);
    summary("whelk", &whelk_samples);
    if let Some((classes, stats)) = last_rustdl_stats {
        println!();
        println!(
            "rustdl: classes={classes} pure_el_mode={} sat_sub={} tab_sub={} sat_unsat={} tab_unsat={}",
            stats.pure_el_mode,
            stats.saturation_subsumption_hits,
            stats.tableau_subsumption_calls,
            stats.saturation_unsat_hits,
            stats.tableau_unsat_calls,
        );
    }
    println!("whelk: derived {last_whelk_subsumptions} named subsumptions");
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_corpus(dir: &Path, quiet: bool, repeats: usize) -> Result<()> {
    let repeats = repeats.max(1);
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
        let onto = match parse_ofn(path) {
            Ok(o) => o,
            Err(e) => {
                failures.push((path.clone(), format!("{e:#}")));
                continue;
            }
        };
        // Run `repeats` times, keep stats from the first run (they're
        // deterministic per the convert+sort guarantee), report
        // median wall clock. Min/max are surfaced in per-file output
        // when more than one repeat is requested.
        let mut times: Vec<std::time::Duration> = Vec::with_capacity(repeats);
        let mut first_stats: Option<owl_dl_reasoner::ClassificationStats> = None;
        let mut classes_len = 0usize;
        let mut err: Option<String> = None;
        for _ in 0..repeats {
            let start = Instant::now();
            match classify(&onto).context("classify") {
                Ok(h) => {
                    times.push(start.elapsed());
                    if first_stats.is_none() {
                        first_stats = Some(h.stats());
                        classes_len = h.classes().len();
                    }
                }
                Err(e) => {
                    err = Some(format!("{e:#}"));
                    break;
                }
            }
        }
        if let Some(e) = err {
            failures.push((path.clone(), e));
            continue;
        }
        times.sort();
        // `times` is non-empty here: the inner loop pushed at least
        // one element before any `err` path could break out, and
        // failures are handled by the `if let Some(e)` above.
        let median = times[times.len() / 2];
        let min = *times.first().expect("times non-empty after success");
        let max = *times.last().expect("times non-empty after success");
        let stats = first_stats.expect("first stats present on success");
        if !quiet {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            if repeats > 1 {
                println!(
                    "{:50} classes={:4} mode={:6} sat-sub={:5} tab-sub={:5} sat-unsat={:3} tab-unsat={:3} med={:>9.3?} min={:>9.3?} max={:>9.3?}",
                    name,
                    classes_len,
                    if stats.pure_el_mode { "EL" } else { "hybrid" },
                    stats.saturation_subsumption_hits,
                    stats.tableau_subsumption_calls,
                    stats.saturation_unsat_hits,
                    stats.tableau_unsat_calls,
                    median,
                    min,
                    max,
                );
            } else {
                println!(
                    "{:50} classes={:4} mode={:6} sat-sub={:5} tab-sub={:5} sat-unsat={:3} tab-unsat={:3} {:>9.3?}",
                    name,
                    classes_len,
                    if stats.pure_el_mode { "EL" } else { "hybrid" },
                    stats.saturation_subsumption_hits,
                    stats.tableau_subsumption_calls,
                    stats.saturation_unsat_hits,
                    stats.tableau_unsat_calls,
                    median,
                );
            }
        }
        total_classes += classes_len;
        if stats.pure_el_mode {
            total_pure_el += 1;
        }
        total_sat_sub += stats.saturation_subsumption_hits;
        total_tab_sub += stats.tableau_subsumption_calls;
        total_sat_unsat += stats.saturation_unsat_hits;
        total_tab_unsat += stats.tableau_unsat_calls;
        total_elapsed += median;
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
    if repeats > 1 {
        println!("  wall clock (sum of medians, {repeats} repeats each): {total_elapsed:.3?}");
    } else {
        println!("  wall clock (sum): {total_elapsed:.3?}");
    }
    if !failures.is_empty() {
        println!();
        println!("# failures");
        for (path, msg) in failures {
            println!("  {}: {msg}", path.display());
        }
    }
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// CB-diff harness
// ──────────────────────────────────────────────────────────────────────────────

/// Side-by-side comparison result from [`cb_diff`].
#[derive(Debug)]
pub struct DiffReport {
    /// Human-readable summary of the CB engine outcome:
    /// `"Classified(N subsumptions)"` or `"OutOfFragment(<reason>)"`.
    pub cb_outcome: String,
    /// `true` iff both engines produced the same set of (sub, sup) IRI pairs
    /// (after normalizing both to: transitive closure, reflexive-free,
    /// no owl:Thing/owl:Nothing on either side). Always `false` when CB
    /// reports `OutOfFragment`.
    pub identical: bool,
    /// Pairs in the CB hierarchy **not** in the current engine's hierarchy
    /// (CB over-derivations / potential FPs if this is non-empty).
    pub only_in_cb: Vec<(String, String)>,
    /// Pairs in the current engine's hierarchy **not** in the CB hierarchy
    /// (CB misses / incompleteness if this is non-empty).
    pub only_in_current: Vec<(String, String)>,
    /// Wall-clock time for the CB engine in milliseconds.
    pub cb_wall_ms: u128,
    /// Wall-clock time for the current (hybrid) engine in milliseconds.
    pub cur_wall_ms: u128,
}

/// Run the CB engine and the current hybrid side-by-side on `internal`, diff
/// their atomic-subsumption relations, and return a [`DiffReport`].
///
/// Normalisation applied to both sides before diffing:
/// 1. Full transitive closure (CB already provides this; the current engine's
///    `is_subclass` matrix is already transitively closed).
/// 2. Reflexive pairs (`A ⊑ A`) excluded.
/// 3. Pairs where either side is `owl:Thing` or `owl:Nothing` excluded
///    (the current engine emits `(X, owl:Thing)` for every class; CB omits these).
/// 4. Equivalences: the current engine encodes `A ≡ B` as `entailed[A][B] &&
///    entailed[B][A]`; we include both `(A,B)` and `(B,A)` when both hold
///    (non-reflexive).
///
/// # Errors
/// Returns an error if the current hybrid engine fails.
pub fn cb_diff(internal: &InternalOntology) -> Result<DiffReport> {
    // ── Run the CB engine ────────────────────────────────────────────────────
    let cb_start = Instant::now();
    let cb_result = owl_dl_cb::classify(internal);
    let cb_wall_ms = cb_start.elapsed().as_millis();

    // ── Run the current (hybrid) engine ─────────────────────────────────────
    let cur_start = Instant::now();
    let cur_h = owl_dl_reasoner::classify_internal(internal).context("classify_internal")?;
    let cur_wall_ms = cur_start.elapsed().as_millis();

    // ── Extract the CB hierarchy as (IRI, IRI) pairs ─────────────────────────
    let cb_pairs: Option<BTreeSet<(String, String)>> = match &cb_result {
        CbOutcome::OutOfFragment(_) => None,
        CbOutcome::Classified(hier) => {
            let vocab = &internal.vocabulary;
            let pairs: BTreeSet<(String, String)> = hier
                .subsumptions
                .iter()
                .map(|(sub, sup)| {
                    (
                        vocab.class_iri(*sub).to_owned(),
                        vocab.class_iri(*sup).to_owned(),
                    )
                })
                .filter(|(sub, sup)| sub != sup) // no reflexive
                .filter(|(sub, sup)| !is_thing_or_nothing(sub) && !is_thing_or_nothing(sup))
                .collect();
            Some(pairs)
        }
    };

    // ── Extract the current engine's hierarchy as (IRI, IRI) pairs ───────────
    let cur_pairs: BTreeSet<(String, String)> = {
        let classes = cur_h.classes();
        let mut pairs = BTreeSet::new();
        for sub in classes {
            if is_thing_or_nothing(sub) {
                continue;
            }
            for sup in classes {
                if is_thing_or_nothing(sup) {
                    continue;
                }
                if sub == sup {
                    continue;
                }
                if cur_h.is_subclass(sub, sup) {
                    pairs.insert((sub.to_owned(), sup.to_owned()));
                }
            }
        }
        pairs
    };

    // ── Build the report ─────────────────────────────────────────────────────
    let cb_outcome = match &cb_result {
        CbOutcome::OutOfFragment(reason) => format!("OutOfFragment({reason})"),
        CbOutcome::Classified(hier) => {
            format!("Classified({} subsumptions)", hier.subsumptions.len())
        }
    };

    let Some(cb_pairs) = cb_pairs else {
        return Ok(DiffReport {
            cb_outcome,
            identical: false,
            only_in_cb: Vec::new(),
            only_in_current: cur_pairs.into_iter().collect(),
            cb_wall_ms,
            cur_wall_ms,
        });
    };

    let only_in_cb: Vec<(String, String)> = cb_pairs.difference(&cur_pairs).cloned().collect();
    let only_in_current: Vec<(String, String)> = cur_pairs.difference(&cb_pairs).cloned().collect();
    let identical = only_in_cb.is_empty() && only_in_current.is_empty();

    Ok(DiffReport {
        cb_outcome,
        identical,
        only_in_cb,
        only_in_current,
        cb_wall_ms,
        cur_wall_ms,
    })
}

/// Returns `true` for the two OWL built-ins that the diff normalisation excludes.
fn is_thing_or_nothing(iri: &str) -> bool {
    iri == "http://www.w3.org/2002/07/owl#Thing" || iri == "http://www.w3.org/2002/07/owl#Nothing"
}

/// Print a [`DiffReport`] to stdout in a human-readable format.
pub fn print_diff_report(r: &DiffReport) {
    println!("# cb-diff report");
    println!("cb_outcome:   {}", r.cb_outcome);
    println!("cb_wall_ms:   {}", r.cb_wall_ms);
    println!("cur_wall_ms:  {}", r.cur_wall_ms);
    println!("identical:    {}", r.identical);
    println!("only_in_cb:   {} pair(s)", r.only_in_cb.len());
    println!("only_in_cur:  {} pair(s)", r.only_in_current.len());
    if !r.only_in_cb.is_empty() {
        println!("# --- only in CB (potential FP) ---");
        for (sub, sup) in &r.only_in_cb {
            println!("  CB+ {sub} ⊑ {sup}");
        }
    }
    if !r.only_in_current.is_empty() {
        println!("# --- only in current engine (CB miss) ---");
        for (sub, sup) in &r.only_in_current {
            println!("  CUR+ {sub} ⊑ {sup}");
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod cb_diff_tests {
    use super::cb_diff;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use owl_dl_core::convert::convert_ontology;

    /// Build a tiny ALC ontology from OFN source and run `cb_diff`.
    ///
    /// This test encodes the contract: on `A⊑B, B⊑C` both engines must agree
    /// (A⊑B, A⊑C, B⊑C). Un-ignored at Task E integration — the CB engine is
    /// now wired (no longer a `todo!()` stub).
    #[test]
    fn cb_diff_simple_chain_is_identical() {
        let src = "Prefix(:=<http://test.example/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://test.example/chain>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n";
        let mut cursor = std::io::Cursor::new(src);
        let (onto, _): (
            horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
            _,
        ) = read(&mut cursor, ParserConfiguration::default()).expect("test ontology parses");
        let internal = convert_ontology(&onto).expect("convert_ontology");
        let report = cb_diff(&internal).expect("cb_diff");
        assert!(
            report.identical,
            "CB and current engine must agree on a simple A⊑B,B⊑C chain; \
             only_in_cb={:?} only_in_current={:?}",
            report.only_in_cb, report.only_in_current,
        );
    }
}

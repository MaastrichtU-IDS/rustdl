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

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use horned_owl::curie::PrefixMapping;
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::omn::AsManchester;
use horned_owl::io::omn::reader::read as read_omn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::io::rdf::reader::read as read_rdf;
use horned_owl::model::{AnnotationSubject, AnnotationValue, Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    Classification, Realization, classify_n2, classify_n2_with_timeout, classify_saturation_only,
    classify_with_budget, inferred_data_property_values, inferred_object_property_values,
    instances_of, instances_of_saturation_only, is_class_satisfiable, is_consistent,
    is_instance_of, is_instance_of_saturation_only, is_subclass_of, is_subclass_of_saturation_only,
    is_subclass_of_with_stats, realize, realize_saturation_only,
};
use owl_dl_reasoner::{ProveEntailmentResult, prove_entailment_rcstr, render_proof_with_defs};

mod json_out;
mod report;

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
        /// Emit a single machine-readable JSON object on stdout (schema v1);
        /// diagnostics stay on stderr. The stable contract for tooling.
        #[arg(long)]
        json: bool,
    },
    /// Inferred disjointness: disjoint class pairs (entailment) + disjoint
    /// object/data property pairs (structural). `--json` for tooling.
    Disjoint {
        /// Path to an ontology file.
        file: PathBuf,
        /// Per-pair `C ⊓ D` probe deadline in ms (0 = unbounded). Default 1000.
        #[arg(long, default_value_t = 1000)]
        pair_timeout_ms: u64,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
    /// Object + data property hierarchies (structural subsumption over
    /// declared properties). `--json` for tooling.
    PropertyHierarchy {
        /// Path to an ontology file.
        file: PathBuf,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
    /// Inferred individual (in)equality: entailed same-individual groups
    /// (`same_individuals`) + entailed different-individual pairs
    /// (`different_individuals`). `--json` for tooling.
    Individuals {
        /// Path to an ontology file.
        file: PathBuf,
        /// Per-pair probe deadline in ms (0 = unbounded). Default 1000.
        #[arg(long, default_value_t = 1000)]
        pair_timeout_ms: u64,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
    /// Inferred property values over named individuals: entailed OBJECT
    /// property triples (`inferred_object_property_values`, sound seed +
    /// bounded entailment extension) + entailed DATA property quads
    /// (`inferred_data_property_values`, structural passthrough). `--json`
    /// for tooling.
    PropertyValues {
        /// Path to an ontology file.
        file: PathBuf,
        /// Per-pair entailment-probe deadline in ms (0 = unbounded).
        /// Default 1000. Data values are structural (no deadline).
        #[arg(long, default_value_t = 1000)]
        pair_timeout_ms: u64,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
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
        /// Path to an OWL ontology (.ofn / .owx / .owl / .rdf / .omn —
        /// format auto-detected from the extension).
        file: PathBuf,
        /// Per-pair tableau timeout in milliseconds; `0` = unbounded.
        /// Pairs exceeding the budget default to `not subsumed` (a
        /// sound under-approximation — never a false subsumption, but
        /// real subsumptions may be missed). When any pair times out,
        /// the run prints a prominent INCOMPLETE warning to stderr.
        /// Default **5 ms** (was 1000 until v0.4.19); pass
        /// `--pair-timeout-ms 0` for the complete (unbounded) result and
        /// `--pair-timeout-ms 1000` to restore the pre-v0.4.19 behaviour
        /// exactly.
        ///
        /// The old default was "the empirical knee on pizza" — tuned on
        /// one ontology. A 1,920-ontology two-arm sweep at 5 ms found
        /// **16 recoveries, 0 regressions and −15.9% wall**, with
        /// ΔMISSED **+8 (+1.04%)** and FP=0 over a 400-ontology
        /// Konclude ∪ `HermiT` net. On the hard tail the engines never
        /// terminate on a pair — they only burn the whole budget and
        /// time out having found nothing — so a large budget buys no
        /// completeness and starves the phases that do the work
        /// (`ore_ont_934` spent ~108 s in `unsat_probe` and never
        /// reached `tier_walk`; at 5 ms it classifies in 50 s with
        /// FP=0/MISSED=0 against an adjudicated oracle).
        ///
        /// **1 ms was screened and rejected**: ΔMISSED +360 (+46.75%),
        /// ~9× the gate. The completeness cliff sits between 1 and 5.
        ///
        /// This default is coupled to `RUSTDL_CLASSIFY_PROBE_ON_INCOMPLETE`,
        /// which must stay ON: a small budget empties `unsatisfiable_idxs`
        /// and would otherwise disable the classify inconsistency detector,
        /// reporting `consistent: true` on `ore_ont_16372`, which three
        /// reasoners call inconsistent.
        #[arg(long, default_value_t = 5)]
        pair_timeout_ms: u64,
        /// Global wall-clock budget for the WHOLE classify, in
        /// milliseconds; `0` = unbounded (the default). Bounds total time
        /// regardless of pair count — pairs still undecided when the
        /// deadline passes default to `not subsumed` (a sound
        /// under-approximation: FP=0, real subsumptions may be missed).
        /// Composes with `--pair-timeout-ms`: each probe is cut at the
        /// smaller of the per-pair budget and the time left on the global
        /// deadline. Use it for a hard "give me whatever you have in N ms"
        /// bound on large or hard ontologies.
        #[arg(long, default_value_t = 0)]
        global_timeout_ms: u64,
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
        ///
        /// To opt into the hypertableau sound-accelerator wedge (H4),
        /// set the `RUSTDL_HYPERTABLEAU=1` environment variable — it
        /// tries the hyperresolution engine before each tableau
        /// subsumption probe and trusts its sound `Unsat` verdicts.
        /// Default off. (Env var, not a flag, to avoid an `unsafe`
        /// `set_var` under the crate's `unsafe_code` deny.)
        #[arg(long)]
        saturation_only: bool,
        /// Emit a single machine-readable JSON object on stdout (schema v1);
        /// diagnostics stay on stderr. The stable contract for tooling.
        #[arg(long)]
        json: bool,
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
    /// Satisfiability of a Manchester class expression.
    SatExpr {
        /// Path to an ontology file.
        file: PathBuf,
        /// Manchester-syntax class expression, e.g. `:A and not :B`.
        ce: String,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
    /// Whether `SubClassOf(sub-ce, sup-ce)` is entailed, for two Manchester
    /// class expressions.
    SubclassExpr {
        /// Path to an ontology file.
        file: PathBuf,
        /// Manchester-syntax sub-class expression.
        sub_ce: String,
        /// Manchester-syntax super-class expression.
        sup_ce: String,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
    },
    /// Named individuals entailed to be instances of a Manchester class
    /// expression.
    InstancesExpr {
        /// Path to an ontology file.
        file: PathBuf,
        /// Manchester-syntax class expression.
        ce: String,
        /// Emit a single machine-readable JSON object on stdout (schema v1).
        #[arg(long)]
        json: bool,
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
        /// Also print inferred object property assertions (subject<TAB>property<TAB>object).
        #[arg(long)]
        properties: bool,
        /// Emit a single machine-readable JSON object on stdout (schema v1);
        /// diagnostics stay on stderr. The stable contract for tooling.
        #[arg(long)]
        json: bool,
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
    /// Classify each residual GCI by **which absorption technique would
    /// remove it** (`domain_absorbable` / `binary_absorbable` /
    /// `nominal_absorbable` / `card_antecedent_n_gt_1` /
    /// `qualified_exists_antecedent` / `genuinely_disjunctive`) and print the
    /// histogram. Report-only — changes no reasoning behaviour. See
    /// `docs/2026-08-01-absorption-is-the-bottleneck.md`.
    ResidualAbsorbability {
        /// Path to an OWL ontology (.ofn/.owl/.owx/.omn — format sniffed).
        file: PathBuf,
        /// Emit one machine-readable `tsv:` line instead of the histogram
        /// (for the population census).
        #[arg(long)]
        tsv: bool,
    },
    /// Print DL-clause shape statistics (hypertableau Phase H0):
    /// total clauses, Horn vs disjunctive, ⊥-headed, ∃-headed,
    /// and deferred (constructs the H0 clausifier doesn't yet
    /// handle). See `docs/hypertableau-scoping.md`.
    ClauseStats {
        /// Path to an OWL functional-syntax (.ofn) ontology.
        file: PathBuf,
    },
    /// Explain WHY an entailment holds: print a minimal responsible-axiom set.
    Justify {
        /// Path to the ontology (.ofn).
        file: PathBuf,
        /// Query (full IRIs): `subclass S T` | `unsat C` | `instance I C` |
        /// `equivalent A B` | `disjoint A B` | `inconsistent` |
        /// `subproperty P Q` | `equiv-property P Q` | `disjoint-property P Q` |
        /// `property A P B` | `same A B` | `different A B` |
        /// `subdata-property DP DQ` | `equiv-data-property DP DQ` |
        /// `disjoint-data-property DP DQ` |
        /// `data-value A DP V` (V = `"lex"^^xsd:type` or `"lex"`).
        #[arg(num_args = 1..)]
        query: Vec<String>,
        /// Print ALL minimal justifications (capped by --max), not just one.
        #[arg(long)]
        all: bool,
        /// Cap on the number of justifications printed with --all.
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
        /// Weaken each justification axiom to its responsible fragment (laconic).
        #[arg(long)]
        laconic: bool,
        /// Emit machine-readable JSON (schema v1) instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Suggest minimal axiom removals to break an unwanted entailment.
    Repair {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf / .omn).
        file: PathBuf,
        /// Query (same forms as `justify`): `unsat C` | `subclass S T` |
        /// `inconsistent` | `instance I C` | … (see `justify --help`).
        #[arg(num_args = 1..)]
        query: Vec<String>,
        /// Cap on the number of repairs printed (smallest first).
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
    },
    /// Print a step-level DL proof tree for `SUB ⊑ SUP`.
    ///
    /// For entailments in the EL saturation fragment, prints a complete
    /// step-level proof with one line per rule application.
    /// For SROIQ entailments (tableau-only), prints the axiom-level
    /// justification with a note that step proofs are unavailable.
    Prove {
        /// Path to an OWL ontology (.ofn / .owx / .owl / .rdf / .omn).
        file: PathBuf,
        /// Full IRI of the sub-class.
        sub: String,
        /// Full IRI of the super-class.
        sup: String,
        /// Re-verify each recorded proof step against its rule's
        /// semantic definition (slower; for debugging).
        #[arg(long)]
        verify_proof: bool,
        /// Emit machine-readable JSON (schema v1) instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Diagnose a broken ontology: partition unsatisfiable classes into ROOT
    /// (genuine causes) and DERIVED (collateral), justify the roots, and on an
    /// inconsistent ontology report the responsible axioms.
    Diagnose {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf / .omn).
        file: PathBuf,
        /// Print ALL minimal justifications per root (capped by --max), not just one.
        #[arg(long)]
        all: bool,
        /// Cap on the number of justifications printed with --all.
        #[arg(long, default_value_t = 10)]
        max: usize,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
    },
    /// Generate a self-contained HTML debugging report (consistency, root/derived
    /// unsatisfiable classes, justifications, and repair suggestions).
    Report {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf / .omn).
        file: PathBuf,
        /// Write the HTML to this file (default: stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
        /// Maximum number of root classes given full justify+repair detail.
        #[arg(long, default_value_t = 50)]
        max_roots: usize,
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

/// The ontology serializations the CLI can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OntFormat {
    /// OWL Functional Syntax (`.ofn`).
    Ofn,
    /// OWL/XML (`.owx`).
    Owx,
    /// RDF/XML (`.owl`, `.rdf`).
    RdfXml,
    /// OWL Manchester Syntax (`.omn`).
    Omn,
}

/// Detect the ontology serialization from a content sniff, falling back
/// to the file extension when the content is inconclusive.
///
/// **Content wins over extension** when it unambiguously identifies the
/// format. This is deliberate: real-world corpora (e.g. the ORE 2015
/// pool) ship OWL-functional-syntax files with a `.owl` extension, and
/// the pure-extension router fed those to the RDF/XML reader, which
/// **panics** (an `unwrap` on the oxrdf parse error deep inside
/// horned-owl) instead of erroring. Sniffing routes such a file to the
/// functional-syntax reader, which parses it correctly.
fn detect_format(src: &str, ext: Option<&str>) -> OntFormat {
    // First meaningful line: skip a leading BOM, blank lines, and
    // OFN/Turtle-style `#` comments.
    let first = src
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");

    // OWL Functional Syntax begins with `Prefix(` or `Ontology(`.
    if first.starts_with("Prefix(") || first.starts_with("Ontology(") {
        return OntFormat::Ofn;
    }

    // OWL Manchester Syntax uses colon-form keywords — `Prefix:` /
    // `Ontology:` document headers or a top-level frame keyword. This is
    // unambiguous against OFN (which uses the paren form `Prefix(`).
    if first.starts_with("Prefix:")
        || first.starts_with("Ontology:")
        || first.starts_with("Class:")
        || first.starts_with("ObjectProperty:")
        || first.starts_with("DataProperty:")
        || first.starts_with("AnnotationProperty:")
        || first.starts_with("Individual:")
        || first.starts_with("Datatype:")
    {
        return OntFormat::Omn;
    }

    // XML family: distinguish OWL/XML (`<Ontology>` root) from RDF/XML
    // (`<rdf:RDF>` root) by scanning a short prefix; fall back to the
    // extension for ambiguous XML.
    if first.starts_with('<') {
        let head: String = src.chars().take(4096).collect();
        if head.contains("<rdf:RDF") || head.contains("<RDF") {
            return OntFormat::RdfXml;
        }
        if head.contains("<Ontology") {
            return OntFormat::Owx;
        }
        return match ext {
            Some("owx") => OntFormat::Owx,
            _ => OntFormat::RdfXml,
        };
    }

    // Inconclusive content: trust the extension, defaulting to OFN
    // (backward-compatible with the historical behaviour).
    match ext {
        Some("owx") => OntFormat::Owx,
        Some("owl" | "rdf") => OntFormat::RdfXml,
        Some("omn") => OntFormat::Omn,
        _ => OntFormat::Ofn,
    }
}

/// Parse an ontology. The serialization is detected from a content sniff
/// ([`detect_format`]), falling back to the file extension — so a file
/// whose extension misrepresents its content (e.g. OWL-functional syntax
/// in a `.owl` file) is still read by the correct reader rather than
/// panicking in the RDF/XML reader.
fn parse_ofn(path: &Path) -> Result<SetOntology<RcStr>> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("opening ontology file: {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let format = detect_format(&src, ext.as_deref());
    let mut reader = std::io::Cursor::new(src);
    let cfg = ParserConfiguration::default();
    let ontology: SetOntology<RcStr> = match format {
        OntFormat::Owx => read_owx(&mut reader, cfg)
            .map(|(o, _)| o)
            .map_err(|e| anyhow::anyhow!("parsing OWX ontology {}: {e}", path.display()))?,
        OntFormat::RdfXml => read_rdf(&mut reader, cfg)
            .map(|(o, _)| o.into())
            .map_err(|e| anyhow::anyhow!("parsing RDF/XML ontology {}: {e}", path.display()))?,
        OntFormat::Ofn => read_ofn(&mut reader, cfg)
            .map(|(o, _)| o)
            .map_err(|e| anyhow::anyhow!("parsing OFN ontology {}: {e}", path.display()))?,
        OntFormat::Omn => read_omn(&mut reader, cfg)
            .map(|(o, _)| o)
            .map_err(|e| anyhow::anyhow!("parsing Manchester ontology {}: {e}", path.display()))?,
    };
    Ok(ontology)
}

/// Like [`parse_ofn`] but also returns the [`PrefixMapping`] collected by the
/// reader (so the caller can produce abbreviated IRIs in Manchester output).
/// The OFN and OWX readers expose a full `PrefixMapping`; the RDF/XML reader
/// does not, so that path returns a default (empty) mapping.
fn parse_ofn_with_pm(path: &Path) -> Result<(SetOntology<RcStr>, PrefixMapping)> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("opening ontology file: {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let format = detect_format(&src, ext.as_deref());
    let mut reader = std::io::Cursor::new(src);
    let cfg = ParserConfiguration::default();
    match format {
        OntFormat::Owx => read_owx(&mut reader, cfg)
            .map_err(|e| anyhow::anyhow!("parsing OWX ontology {}: {e}", path.display())),
        OntFormat::RdfXml => read_rdf(&mut reader, cfg)
            .map(|(o, _incomplete)| (o.into(), PrefixMapping::default()))
            .map_err(|e| anyhow::anyhow!("parsing RDF/XML ontology {}: {e}", path.display())),
        OntFormat::Ofn => read_ofn(&mut reader, cfg)
            .map_err(|e| anyhow::anyhow!("parsing OFN ontology {}: {e}", path.display())),
        OntFormat::Omn => read_omn(&mut reader, cfg)
            .map_err(|e| anyhow::anyhow!("parsing Manchester ontology {}: {e}", path.display())),
    }
}

/// Parse a Manchester-syntax class expression (`ce`) against the prefix map
/// collected from the ontology file (so e.g. `:A` resolves via the file's
/// default namespace), for the `*-expr` subcommands (issue #48).
fn parse_ce(pm: &PrefixMapping, s: &str) -> Result<horned_owl::model::ClassExpression<RcStr>> {
    let build: horned_owl::model::Build<RcStr> = horned_owl::model::Build::new();
    horned_owl::io::omn::reader::parse_class_expression(s, pm, &build)
        .map_err(|e| anyhow::anyhow!("parsing class expression '{s}': {e}"))
}

const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

/// Map every entity IRI to its first `rdfs:label` literal (if any). Used by
/// `justify --labels` to gloss opaque IRIs (e.g. SIO numeric codes).
fn build_label_map(onto: &SetOntology<RcStr>) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for ac in onto {
        if let Component::AnnotationAssertion(aa) = &ac.component {
            if aa.ann.ap.0.as_ref() != RDFS_LABEL {
                continue;
            }
            if let (AnnotationSubject::IRI(iri), AnnotationValue::Literal(lit)) =
                (&aa.subject, &aa.ann.av)
            {
                m.entry(iri.as_ref().to_string())
                    .or_insert_with(|| lit.literal().clone());
            }
        }
    }
    m
}

/// The local name of an IRI — the part after the last `#` or `/`.
fn local_name(iri: &str) -> &str {
    iri.rsplit(['#', '/']).next().unwrap_or(iri)
}

fn print_classification(h: &Classification) {
    let stdout = std::io::stdout();
    let mut out = BufWriter::with_capacity(1 << 16, stdout.lock());
    let _ = write_classification(&mut out, h);
    let _ = out.flush();
}

/// Print a prominent stderr warning if any class pair hit the per-pair
/// timeout — those pairs were recorded as "not subsumed", so the
/// hierarchy may be missing real subsumptions. Sound (no false edges),
/// but the user must know the result is an under-approximation.
/// The global budget left for classification once parsing has been paid for.
///
/// `--global-timeout-ms N` promises a bound on the WHOLE run, so the clock has to
/// include the parse that already happened. Charging it is the difference between
/// the flag meaning "N ms of reasoning" and "N ms of wall": on `ore_ont_7192`
/// parsing alone is 10.8 s, so a 55 s budget produced a ~66 s wall and blew a 60 s
/// cap that the same run finished under with NO deadline at all. Measured A/B on
/// one commit, verdict-preserving (identical row counts): `ore_ont_7192`
/// 68.6 s → 54.6 s, `ore_ont_2574` 45.8 s → 42.5 s.
///
/// `0` means unbounded and stays unbounded. `saturating_sub` floors at zero: if
/// parsing already outspent the budget the deadline is effectively "now", every
/// probe is cut, and the run returns the sound partial hierarchy rather than
/// silently running on.
///
/// **Honest bound.** Parsing and `convert_ontology` are not interruptible, so this
/// does not make the flag a hard wall cap — it stops the budget being *extended*
/// by work that already happened. Conversion is the other pre-deadline segment and
/// is covered separately by `RUSTDL_PREP_DEADLINE` (default off). See
/// `docs/2026-08-16-global-deadline-does-not-bound-wall.md`.
fn global_budget_after_parse(
    global_timeout_ms: u64,
    parse_elapsed: std::time::Duration,
) -> Option<std::time::Duration> {
    (global_timeout_ms != 0)
        .then(|| std::time::Duration::from_millis(global_timeout_ms).saturating_sub(parse_elapsed))
}

fn warn_if_incomplete(timed_out_pairs: usize, pair_timeout_ms: u64, global_timeout_ms: u64) {
    if timed_out_pairs == 0 {
        return;
    }
    let bound = match (pair_timeout_ms, global_timeout_ms) {
        (p, 0) => format!("{p} ms per-pair timeout"),
        (0, g) => format!("{g} ms global timeout"),
        (p, g) => format!("{p} ms per-pair / {g} ms global timeout"),
    };
    eprintln!(
        "\n⚠  INCOMPLETE: {timed_out_pairs} class pair(s) hit the {bound} and were recorded \
         as 'not subsumed'."
    );
    eprintln!(
        "   The classification is SOUND (no false subsumptions) but may be missing real ones. \
         Re-run with `--pair-timeout-ms 0 --global-timeout-ms 0` for the complete (unbounded) \
         result."
    );
}

/// Print a stderr warning if any axioms were dropped during conversion
/// (unsupported constructs — see `owl_dl_reasoner::dropped_axioms`). Those
/// axioms are simply absent from reasoning, so the result is a sound
/// under-approximation: nothing false is reported, but something may be
/// missing.
///
/// Called from every command that ANSWERS an entailment question. It used to
/// be called from three (`consistent`, `classify`, `realize`) out of 27, which
/// is how issue #72 could happen: `instances-expr` on an ontology whose only
/// two axioms were dropped printed NOTHING — no answer and no warning — while
/// `classify` on the same file reported the drop. `CLAUDE.md`'s own rule is
/// that a sound under-approximation the caller cannot detect is the defect.
///
/// Pure diagnostics (`tbox-stats`, `clause-stats`, `locality-stats`,
/// `residual-*`, `hyper-*`) are deliberately NOT included: they report on the
/// conversion itself rather than answering a question about the ontology, and
/// `dropped_block` costs one extra `convert_ontology` per call.
///
/// Goes to stderr, so it is emitted on the `--json` paths too: stdout stays a
/// single JSON object, and for the commands whose JSON carries no `dropped`
/// block this is the only signal available.
fn warn_if_dropped(dropped: &std::collections::BTreeMap<String, u64>) {
    let total: u64 = dropped.values().sum();
    if total == 0 {
        return;
    }
    let kinds: Vec<String> = dropped
        .iter()
        .map(|(kind, count)| format!("{kind} ×{count}"))
        .collect();
    eprintln!(
        "warning: {total} axiom(s) not understood and dropped ({}); results are a sound \
         under-approximation",
        kinds.join(", ")
    );
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
    writeln!(out, "# fragment: {}", stats.fragment)?;
    writeln!(
        out,
        "# abox_check: {}",
        if !owl_dl_reasoner::abox_check_enabled() {
            "skipped"
        } else if stats.inconsistent {
            "inconsistent"
        } else {
            "unknown"
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
    writeln!(
        out,
        "# label heuristic: pruned={} pass_through={} misses={}",
        stats.label_cache_pruned, stats.label_cache_pass_through, stats.label_cache_misses,
    )?;
    // Phase line items, in execution order, each MEASURED DIRECTLY. These eight
    // plus `unattributed` sum to the classify wall — no residual absorbs the
    // difference. Before 2026-08-01 `tier_walk` was that residual and every
    // unbudgeted prep second was charged to it (`ore_ont_1028`: 7198 ms reported
    // for an 80 ms tier walk), which falsified a taxonomy of the DNF corpus.
    writeln!(
        out,
        "# wall breakdown ms: saturate={} precheck={} prepare={} label_cache_build={} \
         unsat_probe={} tier_walk={} sweeps={} matrix={} unattributed={}",
        stats.saturate_wall_ms,
        stats.precheck_wall_ms,
        stats.prepare_wall_ms,
        stats.label_cache_build_wall_ms,
        stats.unsat_probe_wall_ms,
        stats.tier_walk_wall_ms,
        stats.sweep_wall_ms,
        stats.matrix_wall_ms,
        stats.unattributed_wall_ms,
    )?;
    // NESTED sub-timers of the label-cache / tier-walk phases above — reported on
    // their own line precisely so they are not mistaken for members of that sum.
    writeln!(
        out,
        "# wall breakdown ms (nested): snapshot_cache_build={} snapshot_replay={}",
        stats.snapshot_cache_build_wall_ms, stats.snapshot_replay_wall_ms,
    )?;
    writeln!(
        out,
        "# per-class BackPropRisk: safe={} unsafe={} (Phase 3a recon)",
        stats.per_class_safe_count, stats.per_class_unsafe_count,
    )?;
    let p = &stats.pairs_per_sub;
    if !p.is_empty() {
        let mut counts: Vec<u32> = p.values().copied().collect();
        counts.sort_unstable();
        let n = counts.len();
        let total: u64 = counts.iter().map(|&c| u64::from(c)).sum();
        let median = counts[n / 2];
        let p90 = counts[(n * 90) / 100];
        let p99 = counts[((n * 99) / 100).min(n - 1)];
        let max = counts[n - 1];
        writeln!(
            out,
            "# pairs-per-sub: n_subs={n} total={total} median={median} p90={p90} p99={p99} max={max}"
        )?;
        let h = &stats.wedge_cost_histogram_ms;
        writeln!(
            out,
            "# wedge-cost-histogram ms (0|1|2-4|5-9|10-19|20-49|50-99|100-999|≥1000):"
        )?;
        writeln!(
            out,
            "#   {} | {} | {} | {} | {} | {} | {} | {} | {}",
            h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]
        )?;
    }
    if stats.timed_out_pairs > 0 {
        writeln!(
            out,
            "# timed-out pairs: {} (defaulted to not-subsumed)",
            stats.timed_out_pairs
        )?;
    }
    if stats.hyper_proven_pairs > 0 {
        writeln!(
            out,
            "# hyper-proven pairs: {} (sound, skipped tableau)",
            stats.hyper_proven_pairs
        )?;
    }
    if stats.fallthrough_ran > 0 {
        writeln!(
            out,
            "# fallthrough (wedge-stall→tableau): ran={} rescued={} (of which diverged-stall={}) notsub={} noverdict={} from_diverged={}",
            stats.fallthrough_ran,
            stats.fallthrough_subsumed,
            stats.fallthrough_subsumed_diverged,
            stats.fallthrough_notsubsumed,
            stats.fallthrough_noverdict,
            stats.fallthrough_from_diverged
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
    // Direct edges. `taxonomy_direct_subsumers` skips UNSATISFIABLE subjects,
    // which are already reported on their own `unsat` lines — see its doc
    // comment for why (and for the 758-million-row ontology that made it
    // obvious).
    for c in classes {
        for sup in h.taxonomy_direct_subsumers(c) {
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
        Command::Consistent { file, json } => {
            let onto = parse_ofn(&file)?;
            let verdict = is_consistent(&onto).context("is_consistent")?;
            // NOTE: `dropped_block` re-runs `convert_ontology` (see its doc
            // comment) — one extra conversion per invocation, negligible
            // vs. reasoning; accepted trade-off.
            let dropped = json_out::dropped_block(&onto);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_consistent_json(
                        verdict, dropped
                    ))?
                );
                return Ok(());
            }
            println!(
                "{}",
                if verdict {
                    "consistent"
                } else {
                    "inconsistent"
                }
            );
            warn_if_dropped(&dropped);
        }
        Command::Disjoint {
            file,
            pair_timeout_ms,
            json,
        } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let deadline =
                (pair_timeout_ms > 0).then(|| std::time::Duration::from_millis(pair_timeout_ms));
            let classes =
                owl_dl_reasoner::disjoint_classes(&onto, deadline).context("disjoint_classes")?;
            let obj = owl_dl_reasoner::disjoint_object_properties(&onto)
                .context("disjoint_object_properties")?;
            let data = owl_dl_reasoner::disjoint_data_properties(&onto)
                .context("disjoint_data_properties")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_disjoint_json(
                        &classes, obj, data
                    ))?
                );
                return Ok(());
            }
            println!("# disjoint classes");
            for (a, b) in classes.pairs() {
                println!("{a}\t{b}");
            }
            if classes.incomplete() {
                eprintln!(
                    "warning: disjointness incomplete (budget/fragment) — sound under-approximation"
                );
            }
            println!("# disjoint object properties");
            for (a, b) in &obj {
                println!("{a}\t{b}");
            }
            println!("# disjoint data properties");
            for (a, b) in &data {
                println!("{a}\t{b}");
            }
        }
        Command::Individuals {
            file,
            pair_timeout_ms,
            json,
        } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let deadline =
                (pair_timeout_ms > 0).then(|| std::time::Duration::from_millis(pair_timeout_ms));
            let same =
                owl_dl_reasoner::same_individuals(&onto, deadline).context("same_individuals")?;
            let different = owl_dl_reasoner::different_individuals(&onto, deadline)
                .context("different_individuals")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_individuals_json(
                        &same, &different
                    ))?
                );
                return Ok(());
            }
            println!("# same individuals");
            for group in same.groups() {
                println!("{}", group.join("\t"));
            }
            println!("# different individuals");
            for (a, b) in different.pairs() {
                println!("{a}\t{b}");
            }
            if same.incomplete() || different.incomplete() {
                eprintln!(
                    "warning: individuals incomplete (budget/fragment) — sound under-approximation"
                );
            }
        }
        Command::PropertyValues {
            file,
            pair_timeout_ms,
            json,
        } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let deadline =
                (pair_timeout_ms > 0).then(|| std::time::Duration::from_millis(pair_timeout_ms));
            let obj = inferred_object_property_values(&onto, deadline)
                .context("inferred_object_property_values")?;
            let data =
                inferred_data_property_values(&onto).context("inferred_data_property_values")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_property_values_json(
                        &obj, &data
                    ))?
                );
                return Ok(());
            }
            println!("# object property values");
            for (s, p, o) in obj.triples() {
                println!("{s}\t{p}\t{o}");
            }
            println!("# data property values");
            // The lang tag is part of the literal's IDENTITY (issue #72), so it
            // is printed as a fifth column rather than dropped: `"bonjour"@fr`
            // and `"bonjour"@de` are distinct literals and must not render
            // identically. Empty for every non-`rdf:langString` value.
            for (s, p, lex, dt, lang) in data.quints() {
                println!("{s}\t{p}\t{lex}\t{dt}\t{lang}");
            }
            if obj.incomplete() || data.incomplete() {
                eprintln!(
                    "warning: property values incomplete (budget/fragment) — sound under-approximation"
                );
            }
        }
        Command::PropertyHierarchy { file, json } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let obj = owl_dl_reasoner::classify_object_property_hierarchy(&onto)
                .context("classify_object_property_hierarchy")?;
            let data = owl_dl_reasoner::classify_data_property_hierarchy(&onto)
                .context("classify_data_property_hierarchy")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_prophier_json(&obj, &data))?
                );
                return Ok(());
            }
            println!("# object property hierarchy");
            for (a, b) in obj.direct_subsumptions() {
                println!("{a}\t{b}");
            }
            println!("# data property hierarchy");
            for (a, b) in data.direct_subsumptions() {
                println!("{a}\t{b}");
            }
        }
        Command::Sat { file, class_iri } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
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
            warn_if_dropped(&json_out::dropped_block(&onto));
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
            global_timeout_ms,
            top_down: _,
            n2_classify,
            saturation_only,
            json,
        } => {
            // Opt-in phase timing (`RUSTDL_TIMING=1`): separate parse (horned-owl
            // read) from classify (convert/preprocess + reasoning), so the wall can
            // be compared apples-to-apples with reasoners whose reported time
            // excludes parsing (e.g. Konclude's `reason_ms`). Default output
            // unchanged.
            let timing = std::env::var_os("RUSTDL_TIMING").is_some();
            let t_parse = std::time::Instant::now();
            let onto = parse_ofn(&file)?;
            let parse_ms = t_parse.elapsed().as_secs_f64() * 1000.0;
            // 0 = unbounded; any positive value bounds each pair / the whole run.
            let timeout =
                (pair_timeout_ms != 0).then(|| std::time::Duration::from_millis(pair_timeout_ms));
            let global_budget = global_budget_after_parse(global_timeout_ms, t_parse.elapsed());
            let t_classify = std::time::Instant::now();
            let h = if saturation_only {
                classify_saturation_only(&onto).context("classify_saturation_only")?
            } else if n2_classify {
                // Legacy n² path honors only the per-pair budget.
                match timeout {
                    Some(t) => {
                        classify_n2_with_timeout(&onto, t).context("classify_n2_with_timeout")?
                    }
                    None => classify_n2(&onto).context("classify_n2")?,
                }
            } else {
                // Default top-down path: both bounds (either/both may be None).
                classify_with_budget(&onto, timeout, global_budget)
                    .context("classify_with_budget")?
            };
            let classify_ms = t_classify.elapsed().as_secs_f64() * 1000.0;
            if timing {
                eprintln!("TIMING parse_ms={parse_ms:.1} classify_ms={classify_ms:.1}");
            }
            // Read the conversion's dropped-axiom tally off the result rather
            // than calling `dropped_block`, which re-runs `convert_ontology`.
            // That second conversion is invisible when reasoning dominates and
            // ruinous when it does not: `ore_ont_868` spends 42 s of a 92 s
            // classify in conversion, so paying it twice was ~46% of the wall.
            let dropped = h.stats().dropped.by_kind().clone();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_classify_json(&h, dropped))?
                );
                warn_if_incomplete(
                    h.stats().timed_out_pairs,
                    pair_timeout_ms,
                    global_timeout_ms,
                );
                return Ok(());
            }
            print_classification(&h);
            warn_if_incomplete(
                h.stats().timed_out_pairs,
                pair_timeout_ms,
                global_timeout_ms,
            );
            warn_if_dropped(&dropped);
        }
        Command::Instance {
            file,
            class_iri,
            individual_iri,
            saturation_only,
        } => {
            let onto = parse_ofn(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
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
            warn_if_dropped(&json_out::dropped_block(&onto));
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
        Command::SatExpr { file, ce, json } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let ce = parse_ce(&pm, &ce)?;
            let v = owl_dl_reasoner::class_expression_satisfiable(&onto, &ce)
                .context("class_expression_satisfiable")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_sat_expr_json(v))?
                );
                return Ok(());
            }
            println!(
                "{}",
                if v.holds() {
                    "satisfiable"
                } else {
                    "unsatisfiable"
                }
            );
            if v.incomplete() {
                eprintln!("warning: verdict is a sound under-approximation (incomplete)");
            }
        }
        Command::SubclassExpr {
            file,
            sub_ce,
            sup_ce,
            json,
        } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let sub_ce = parse_ce(&pm, &sub_ce)?;
            let sup_ce = parse_ce(&pm, &sup_ce)?;
            let v = owl_dl_reasoner::class_expression_entailed_subclass(&onto, &sub_ce, &sup_ce)
                .context("class_expression_entailed_subclass")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_subclass_expr_json(v))?
                );
                return Ok(());
            }
            println!("{}", if v.holds() { "yes" } else { "no" });
            if v.incomplete() {
                eprintln!("warning: verdict is a sound under-approximation (incomplete)");
            }
        }
        Command::InstancesExpr { file, ce, json } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            warn_if_dropped(&json_out::dropped_block(&onto));
            let ce = parse_ce(&pm, &ce)?;
            let r = owl_dl_reasoner::class_expression_instances(&onto, &ce)
                .context("class_expression_instances")?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_instances_expr_json(&r))?
                );
                return Ok(());
            }
            for iri in r.individuals() {
                println!("{iri}");
            }
            if r.incomplete() {
                eprintln!("warning: verdict is a sound under-approximation (incomplete)");
            }
        }
        Command::Realize {
            file,
            saturation_only,
            properties,
            json,
        } => {
            let onto = parse_ofn(&file)?;
            let r = if saturation_only {
                realize_saturation_only(&onto).context("realize_saturation_only")?
            } else {
                realize(&onto).context("realize")?
            };
            // NOTE: `dropped_block` re-runs `convert_ontology` (see its doc
            // comment) — one extra conversion per invocation, negligible
            // vs. reasoning; accepted trade-off.
            let dropped = json_out::dropped_block(&onto);
            if json {
                if properties {
                    // stderr, so stdout stays a single JSON object.
                    eprintln!(
                        "note: --properties has no effect under --json in schema v1 (see issue #45)"
                    );
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_realize_json(&r, dropped))?
                );
                return Ok(());
            }
            print_realization(&r);
            warn_if_dropped(&dropped);
            if properties {
                match owl_dl_reasoner::materialize_object_property_assertions(&onto) {
                    Ok(triples) => {
                        println!("# inferred object property assertions");
                        for (s, p, o2) in triples {
                            println!("{s}\t{p}\t{o2}");
                        }
                    }
                    Err(e) => {
                        eprintln!("# object property assertions unavailable: {e}");
                    }
                }
                match owl_dl_reasoner::materialize_data_property_assertions(&onto) {
                    Ok(triples) => {
                        println!("# inferred data property assertions");
                        for (s, p, lex, dt, lang) in triples {
                            println!("{s}\t{p}\t{lex}\t{dt}\t{lang}");
                        }
                    }
                    Err(e) => {
                        eprintln!("# data property assertions unavailable: {e}");
                    }
                }
            }
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
        Command::Justify {
            file,
            query,
            all,
            max,
            labels,
            laconic,
            json,
        } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let q =
                owl_dl_reasoner::justify::parse_query(&query).map_err(|e| anyhow::anyhow!(e))?;
            if json {
                let (justs, enumeration_complete): (
                    Vec<owl_dl_reasoner::justify::Justification<RcStr>>,
                    bool,
                ) = if all {
                    // Probe with `max + 1`: `find_all_*` stops as soon as
                    // `found.len() >= max`, so a returned count of exactly
                    // `max` is ambiguous (genuinely capped, or coincidentally
                    // exhausted). Asking for one more disambiguates: if the
                    // probe still returns `<= max`, the true set is that
                    // small and enumeration is complete; if it returns
                    // `max + 1`, the true set has more and we were capped.
                    // `saturating_add` guards a `max` already at `usize::MAX`
                    // — in that degenerate (effectively-unbounded) case the
                    // probe can't add headroom, so whatever comes back is,
                    // by construction, the full set.
                    let probe_max = max.saturating_add(1);
                    let mut js = if laconic {
                        owl_dl_reasoner::find_all_laconic_justifications(&onto, &q, probe_max)
                            .context("find_all_laconic_justifications")?
                    } else {
                        owl_dl_reasoner::justify::find_all_justifications(&onto, &q, probe_max)
                            .context("find_all_justifications")?
                    };
                    let enumeration_complete = if max == usize::MAX {
                        true
                    } else if js.len() > max {
                        js.truncate(max);
                        false
                    } else {
                        true
                    };
                    (js, enumeration_complete)
                } else {
                    let one = if laconic {
                        owl_dl_reasoner::find_laconic_justification(&onto, &q)
                            .context("find_laconic_justification")?
                    } else {
                        owl_dl_reasoner::justify::find_one_justification(&onto, &q)
                            .context("find_one_justification")?
                    };
                    (one.into_iter().collect(), true)
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_out::build_justify_json(
                        &justs,
                        &pm,
                        laconic,
                        enumeration_complete,
                    ))?
                );
                return Ok(());
            }
            let label_map = labels.then(|| build_label_map(&onto));
            let render = |j: &owl_dl_reasoner::justify::Justification<RcStr>| {
                let note = if j.minimal_guaranteed {
                    format!("minimal ({})", j.fragment)
                } else {
                    format!("entailing; minimality NOT guaranteed ({})", j.fragment)
                };
                let note = if laconic {
                    format!(
                        "fragments sound; minimal among supported weakenings ({})",
                        j.fragment
                    )
                } else {
                    note
                };
                let kind = if laconic {
                    "laconic justification (structural)"
                } else {
                    "justification"
                };
                println!("# {kind} ({} axioms) — {note}", j.axioms.len());
                for ax in &j.axioms {
                    println!("  {}", ax.as_manchester_with_prefixes(&pm));
                    if let Some(lm) = &label_map {
                        let glosses: Vec<String> = owl_dl_reasoner::justify::component_entities(ax)
                            .into_iter()
                            .filter_map(|iri| {
                                lm.get(&iri)
                                    .map(|l| format!("{} = \"{l}\"", local_name(&iri)))
                            })
                            .collect();
                        if !glosses.is_empty() {
                            println!("      label: {}", glosses.join("; "));
                        }
                    }
                }
            };
            if all {
                let js = if laconic {
                    owl_dl_reasoner::find_all_laconic_justifications(&onto, &q, max)
                        .context("find_all_laconic_justifications")?
                } else {
                    owl_dl_reasoner::justify::find_all_justifications(&onto, &q, max)
                        .context("find_all_justifications")?
                };
                if js.is_empty() {
                    println!("not entailed (no justification)");
                } else {
                    println!("# {} justification(s)", js.len());
                    for j in &js {
                        render(j);
                    }
                }
            } else {
                let one = if laconic {
                    owl_dl_reasoner::find_laconic_justification(&onto, &q)
                        .context("find_laconic_justification")?
                } else {
                    owl_dl_reasoner::justify::find_one_justification(&onto, &q)
                        .context("find_one_justification")?
                };
                match one {
                    Some(j) => render(&j),
                    None => println!("not entailed (no justification)"),
                }
            }
        }
        Command::Repair {
            file,
            query,
            max,
            labels,
        } => {
            use owl_dl_reasoner::justify::component_entities;
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let q =
                owl_dl_reasoner::justify::parse_query(&query).map_err(|e| anyhow::anyhow!(e))?;
            let label_map = labels.then(|| build_label_map(&onto));
            let r = owl_dl_reasoner::find_repairs(&onto, &q, max).context("find_repairs")?;

            if !r.entailed {
                println!("not entailed; nothing to repair");
                return Ok(());
            }
            if r.repairs.is_empty() {
                println!("entailed, but no verifiable axiom removal found");
                return Ok(());
            }

            let completeness = if r.complete {
                "complete"
            } else {
                "w.r.t. found justifications (completeness not guaranteed)"
            };
            println!("# {} minimal repair(s) — {completeness}", r.repairs.len());
            for (i, rep) in r.repairs.iter().enumerate() {
                println!("repair {} (remove {} axiom(s)):", i + 1, rep.remove.len());
                for ax in &rep.remove {
                    println!("  {}", ax.as_manchester_with_prefixes(&pm));
                    if let Some(lm) = &label_map {
                        let glosses: Vec<String> = component_entities(ax)
                            .into_iter()
                            .filter_map(|iri| {
                                lm.get(&iri)
                                    .map(|l| format!("{} = \"{l}\"", local_name(&iri)))
                            })
                            .collect();
                        if !glosses.is_empty() {
                            println!("      label: {}", glosses.join("; "));
                        }
                    }
                }
            }
            if r.dropped_unverified > 0 {
                println!(
                    "# note: {} candidate(s) dropped (failed verification — justification set may be incomplete)",
                    r.dropped_unverified
                );
            }
        }
        Command::Prove {
            file,
            sub,
            sup,
            verify_proof,
            json,
        } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let result = prove_entailment_rcstr(&onto, &sub, &sup).context("prove_entailment")?;
            if json {
                let internal = owl_dl_core::convert::convert_ontology(&onto)
                    .context("re-convert for rendering")?;
                let prove_json = json_out::build_prove_json(&result, &internal, &pm)
                    .context("rendering prove --json proof tree")?;
                println!("{}", serde_json::to_string_pretty(&prove_json)?);
                return Ok(());
            }
            match result {
                ProveEntailmentResult::SaturatorProof(data) => {
                    let root = &data.root;
                    // Render using the internal vocabulary and synthetic defs.
                    // Re-convert to get the internal ontology (for rendering + content check).
                    let internal = owl_dl_core::convert::convert_ontology(&onto)
                        .context("re-convert for rendering")?;
                    if verify_proof {
                        // Use content-checking variant: validates axiom-ref content, not just range.
                        match owl_dl_reasoner::check_proof_with_content(root, &internal) {
                            Ok(()) => eprintln!("# proof verified OK (content-validated)"),
                            Err(e) => eprintln!("# proof check FAILED: {e}"),
                        }
                    }
                    let proof_text = render_proof_with_defs(
                        root,
                        Some(&internal.vocabulary),
                        Some(&data.trace.synthetic_defs),
                        0,
                    );
                    fn count_proof_nodes(node: &owl_dl_reasoner::ProofNode) -> usize {
                        1 + node.premises.iter().map(count_proof_nodes).sum::<usize>()
                    }
                    println!("# step proof ({} steps):", count_proof_nodes(root));
                    print!("{proof_text}");
                    // Collect all axiom refs across the whole proof tree.
                    fn collect_axiom_refs(node: &owl_dl_reasoner::ProofNode, out: &mut Vec<usize>) {
                        for ax in &node.axiom_refs {
                            if !out.contains(&ax.0) {
                                out.push(ax.0);
                            }
                        }
                        for premise in &node.premises {
                            collect_axiom_refs(premise, out);
                        }
                    }
                    let mut all_refs: Vec<usize> = Vec::new();
                    collect_axiom_refs(root, &mut all_refs);
                    all_refs.sort_unstable();
                    if !all_refs.is_empty() {
                        println!(
                            "# axiom provenance ({} axioms in ontology):",
                            internal.axioms.len()
                        );
                        for idx in all_refs {
                            if let Some(ax) = internal.axioms.get(idx) {
                                println!("  axiom[{idx}]: {ax:?}");
                            }
                        }
                    }
                }
                ProveEntailmentResult::JustificationFallback(j) => {
                    println!(
                        "# step proof unavailable (out of EL saturation fragment); \
                         axiom justification:"
                    );
                    if j.axioms.is_empty() {
                        println!("  (no justification available)");
                    } else {
                        for ax in &j.axioms {
                            println!("  {}", ax.as_manchester_with_prefixes(&pm));
                        }
                    }
                }
                ProveEntailmentResult::NotEntailed => {
                    println!("NOT entailed: {sub} SubClassOf {sup} does not hold in this ontology");
                }
            }
        }
        Command::Diagnose {
            file,
            all,
            max,
            labels,
        } => {
            use owl_dl_reasoner::justify::{
                Entailment, component_entities, find_all_justifications, find_one_justification,
            };
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let label_map = labels.then(|| build_label_map(&onto));

            // Shared renderer for a justification (mirrors the `justify` handler).
            let render = |j: &owl_dl_reasoner::justify::Justification<RcStr>, indent: &str| {
                let note = if j.minimal_guaranteed {
                    format!("minimal ({})", j.fragment)
                } else {
                    format!("entailing; minimality NOT guaranteed ({})", j.fragment)
                };
                println!("{indent}justification ({} axioms) — {note}", j.axioms.len());
                for ax in &j.axioms {
                    println!("{indent}  {}", ax.as_manchester_with_prefixes(&pm));
                    if let Some(lm) = &label_map {
                        let glosses: Vec<String> = component_entities(ax)
                            .into_iter()
                            .filter_map(|iri| {
                                lm.get(&iri)
                                    .map(|l| format!("{} = \"{l}\"", local_name(&iri)))
                            })
                            .collect();
                        if !glosses.is_empty() {
                            println!("{indent}      label: {}", glosses.join("; "));
                        }
                    }
                }
            };

            // Render either one or all justifications for an entailment.
            let render_q = |q: &Entailment, indent: &str| -> anyhow::Result<()> {
                if all {
                    let js = find_all_justifications(&onto, q, max)
                        .context("find_all_justifications")?;
                    if js.is_empty() {
                        println!("{indent}(no justification found)");
                    }
                    for j in &js {
                        render(j, indent);
                    }
                } else {
                    match find_one_justification(&onto, q).context("find_one_justification")? {
                        Some(j) => render(&j, indent),
                        None => println!("{indent}(no justification found)"),
                    }
                }
                Ok(())
            };

            let d = owl_dl_reasoner::diagnose(&onto).context("diagnose")?;
            println!("# diagnose: {}", file.display());

            if !d.consistent {
                println!("# consistency: INCONSISTENT");
                println!("## responsible axioms:");
                render_q(&Entailment::Inconsistent, "  ")?;
                return Ok(());
            }

            println!("# consistency: consistent");
            if d.all_unsat.is_empty() {
                println!("# coherent: no unsatisfiable classes");
                return Ok(());
            }
            println!(
                "# unsatisfiable: {}  ({} root, {} derived)",
                d.all_unsat.len(),
                d.roots.len(),
                d.derived.len()
            );

            println!("\n## ROOT unsatisfiable classes (fix these first)");
            for r in &d.roots {
                println!("ROOT  {r}");
                render_q(&Entailment::Unsatisfiable { class: r.clone() }, "  ")?;
                if let Some(deps) = d.root_derives.get(r)
                    && !deps.is_empty()
                {
                    println!("  derives: {}", deps.join(", "));
                }
            }

            if !d.derived.is_empty() {
                println!(
                    "\n## DERIVED unsatisfiable classes (likely resolve once roots are fixed)"
                );
                for dc in &d.derived {
                    println!("DERIVED {}   <= {}", dc.iri, dc.roots.join(", "));
                }
            }
        }
        Command::Report {
            file,
            output,
            labels,
            max_roots,
        } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let label_map = labels.then(|| build_label_map(&onto));
            let report = report::build_report(&onto, file.display().to_string(), max_roots)?;
            let html = report::render_html(&report, &pm, label_map.as_ref());
            match output {
                Some(path) => {
                    std::fs::write(&path, html)
                        .with_context(|| format!("writing report to {}", path.display()))?;
                    eprintln!("report written to {}", path.display());
                }
                None => println!("{html}"),
            }
        }
        Command::TboxStats { file } => {
            let onto = parse_ofn(&file)?;
            // Timed so a volume scan can see a conversion that grows in WALL without
            // growing in rule count — the v0.3.27/v0.3.29 conversion-DNF signature.
            // Covers convert + NNF + absorb + told-table build, i.e. everything
            // `tbox_stats` does; parsing is deliberately outside.
            let t0 = std::time::Instant::now();
            let stats = owl_dl_reasoner::tbox_stats(&onto).context("tbox_stats")?;
            let convert_ms = t0.elapsed().as_millis();
            println!("# convert_ms:           {convert_ms}");
            println!("# concept_rules:        {}", stats.concept_rules);
            println!("# told_super_edges:     {}", stats.told_super_edges);
            println!("# told_disjoint_pairs:  {}", stats.told_disjoint_pairs);
            // MEASUREMENT (RUSTDL_DKEY_SPLIT_STATS=1): how many DKey-disjointness
            // pairs the proposed collapse/broadcast split would drop. Report-only.
            if std::env::var("RUSTDL_DKEY_SPLIT_STATS").is_ok_and(|v| v != "0") {
                use std::sync::atomic::Ordering;
                let total = owl_dl_core::convert::DKEY_SPLIT_TOTAL.load(Ordering::Relaxed);
                let drop = owl_dl_core::convert::DKEY_SPLIT_WOULD_DROP.load(Ordering::Relaxed);
                println!("# dkey_pairs_total:     {total}");
                println!("# dkey_pairs_would_drop:{drop}");
            }
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
        Command::ResidualAbsorbability { file, tsv } => {
            let onto = parse_ofn(&file)?;
            let s = owl_dl_reasoner::residual_absorbability_stats(&onto)
                .context("residual_absorbability_stats")?;
            if tsv {
                // name residual_gcis domain binary nominal card_n_gt_1
                // qualified genuinely concept_rules concept_rule_or
                // concept_rule_or_with_extra_not_atomic
                println!(
                    "tsv:\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    file.file_stem().unwrap_or_default().to_string_lossy(),
                    s.residual_gcis,
                    s.domain_absorbable,
                    s.binary_absorbable,
                    s.nominal_absorbable,
                    s.card_antecedent_n_gt_1,
                    s.qualified_exists_antecedent,
                    s.genuinely_disjunctive,
                    s.concept_rules,
                    s.concept_rule_or,
                    s.concept_rule_or_with_extra_not_atomic,
                    s.concept_rule_or_guard_manufacturable,
                    s.concept_rule_or_guard_manufacturable_card_ge2_only,
                    s.concept_rule_or_guard_manufacturable_complex_only,
                    s.concept_rule_or_guard_manufacturable_synthetic_only,
                    s.distinct_shared_triggers,
                    s.shared_triggers_ge5,
                    s.max_disjunctive_rules_per_trigger,
                );
            } else {
                println!("# residual_gcis:                {}", s.residual_gcis);
                println!("#   domain_absorbable:          {}", s.domain_absorbable);
                println!("#   binary_absorbable:          {}", s.binary_absorbable);
                println!("#   nominal_absorbable:         {}", s.nominal_absorbable);
                println!(
                    "#   card_antecedent_n_gt_1:     {}  (UNSOUND to absorb as domain)",
                    s.card_antecedent_n_gt_1
                );
                println!(
                    "#   qualified_exists_antecedent:{}  (needs a filler check)",
                    s.qualified_exists_antecedent
                );
                println!(
                    "#   genuinely_disjunctive:      {}",
                    s.genuinely_disjunctive
                );
                println!("# removed_by_domain:            {}", s.removed_by_domain());
                println!(
                    "# removed_by_domain_and_binary: {}",
                    s.removed_by_domain_and_binary()
                );
                println!(
                    "# zero_residuals_under_domain:  {}",
                    s.zero_residuals_under_domain()
                );
                println!("# concept_rules:                {}", s.concept_rules);
                println!("#   conclusion_is_or:           {}", s.concept_rule_or);
                println!(
                    "#   ..with extra ¬Atomic:       {}  (binary-absorption candidates)",
                    s.concept_rule_or_with_extra_not_atomic
                );
                println!(
                    "#   ..guard_mfg tierA:          {}  (∃r.F / ≥1 r.F, F named atomic)",
                    s.concept_rule_or_guard_manufacturable
                );
                println!(
                    "#   ..guard_mfg tierB only:     {}  (≥k r.F, k≥2, F named atomic)",
                    s.concept_rule_or_guard_manufacturable_card_ge2_only
                );
                println!(
                    "#   ..guard_mfg tierC only:     {}  (complex filler — recursive minting)",
                    s.concept_rule_or_guard_manufacturable_complex_only
                );
                println!(
                    "#   ..synthetic filler only:    {}",
                    s.concept_rule_or_guard_manufacturable_synthetic_only
                );
                println!(
                    "#   ..guard_mfg any tier:       {}",
                    s.guard_manufacturable_any_tier()
                );
                println!(
                    "# all_or_manufacturable tierA:  {}",
                    s.all_or_rules_guard_manufacturable()
                );
                println!(
                    "# all_or_manufacturable any:    {}",
                    s.all_or_rules_guard_manufacturable_any_tier()
                );
                println!(
                    "# distinct_shared_triggers:     {}  (≥2 disjunctive rules)",
                    s.distinct_shared_triggers
                );
                println!("#   ..with ≥5:                  {}", s.shared_triggers_ge5);
                println!(
                    "# max_rules_per_trigger:        {}",
                    s.max_disjunctive_rules_per_trigger
                );
            }
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
            let census =
                owl_dl_reasoner::clause_deferred_census(&onto).context("deferred_census")?;
            for (kind, count) in census {
                println!("#   deferred[{kind}]: {count}");
            }
        }
        Command::HyperSat {
            file,
            depth,
            per_class_timeout_ms,
        } => {
            use owl_dl_reasoner::HyperResult;
            let onto = parse_ofn(&file)?;
            // Coarse syntactic feature scan — adequate for the SP2-soundness
            // gate question "are inverse/nominal present?"; do not
            // over-engineer beyond a `Debug`-string substring check.
            let mut has_inverse = false;
            let mut has_nominal = false;
            let mut has_card = false;
            for ac in &onto {
                let s = format!("{:?}", ac.component);
                if s.contains("InverseObjectProperties") || s.contains("ObjectInverseOf") {
                    has_inverse = true;
                }
                if s.contains("ObjectOneOf") || s.contains("ObjectHasValue") {
                    has_nominal = true;
                }
                if s.contains("ObjectMinCardinality")
                    || s.contains("ObjectMaxCardinality")
                    || s.contains("ObjectExactCardinality")
                {
                    has_card = true;
                }
            }
            println!("# features: inverse={has_inverse} nominal={has_nominal} card={has_card}");
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
            let mut total_is_blocked_calls = 0u64;
            let mut total_blocks_fired = 0u64;
            let mut total_block_eligible = 0u64;
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
                total_is_blocked_calls += r.stats.is_blocked_calls;
                total_blocks_fired += r.stats.blocks_fired;
                total_block_eligible += r.stats.block_eligible;
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
            println!("# total_is_blocked_calls: {total_is_blocked_calls}");
            println!("# total_blocks_fired:    {total_blocks_fired}");
            println!("# total_block_eligible:  {total_block_eligible}");
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
                    "#   {:?} wall={:.2}ms branches={} (disj={} merge={}) restores={} depth={} blk={}/{}  {}",
                    r.result,
                    r.wall_ms,
                    r.stats.branches_taken,
                    r.stats.disj_branches,
                    r.stats.merge_branches,
                    r.stats.restores,
                    r.stats.max_branch_depth,
                    r.stats.blocks_fired,
                    r.stats.block_eligible,
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
            // Sum profiling counters across retained pairs — diff
            // between blocking modes localises the perf bottleneck.
            {
                let mut tot_blocked = 0_u64;
                let mut tot_compares = 0_u64;
                let mut tot_matches = 0_u64;
                let mut tot_fired = 0_u64;
                let mut tot_eligible = 0_u64;
                for r in &probe.results {
                    tot_blocked += r.stats.is_blocked_calls;
                    tot_compares += r.stats.block_compares;
                    tot_matches += r.stats.match_attempts;
                    tot_fired += r.stats.blocks_fired;
                    tot_eligible += r.stats.block_eligible;
                }
                println!("# is_blocked_calls (sum retained):  {tot_blocked}");
                println!("# block_eligible  (sum retained):  {tot_eligible}");
                println!(
                    "# blocks_fired    (sum retained):  {tot_fired}  <-- blocking that actually caps the model"
                );
                println!("# block_compares  (sum retained):  {tot_compares}");
                println!("# match_attempts  (sum retained):  {tot_matches}");
            }
            // Wall-distribution histogram over branched pairs — answers
            // "how many pairs are slow?" for the HF5 wiring decision.
            {
                let bins = [10.0_f64, 100.0, 500.0, 1000.0, 2000.0, 5000.0];
                let labels = [
                    "<10ms",
                    "<100ms",
                    "<500ms",
                    "<1s",
                    "<2s",
                    "<5s",
                    ">=5s/stall",
                ];
                let mut counts = [0usize; 7];
                for r in probe.results.iter().filter(|r| r.stats.branches_taken > 0) {
                    let idx = bins.iter().position(|&b| r.wall_ms < b).unwrap_or(6);
                    counts[idx] += 1;
                }
                println!("# --- branched-pair wall histogram ---");
                for (lab, c) in labels.iter().zip(counts.iter()) {
                    println!("#   {lab:>11}: {c}");
                }
            }
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
                    "#   {:?} wall={:.2}ms branches={} (disj={} merge={}) restores={} depth={}  {} <= {}",
                    r.result,
                    r.wall_ms,
                    r.stats.branches_taken,
                    r.stats.disj_branches,
                    r.stats.merge_branches,
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

#[cfg(test)]
mod format_detect_tests {
    use super::{OntFormat, detect_format};

    #[test]
    fn ofn_content_with_owl_extension_is_ofn() {
        // The reported bug: ORE 2015 ships OWL-functional syntax with a
        // `.owl` extension. Content must win → OFN, not RDF/XML (which
        // panics on this input).
        let src = "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(<urn:o>)";
        assert_eq!(detect_format(src, Some("owl")), OntFormat::Ofn);
    }

    #[test]
    fn ofn_content_after_comments_and_bom() {
        let src = "\u{feff}# a comment\n\n  Ontology(<urn:o>)\n";
        assert_eq!(detect_format(src, Some("owl")), OntFormat::Ofn);
    }

    #[test]
    fn ofn_extension_still_ofn() {
        let src = "Prefix(:=<urn:#>)\nOntology()";
        assert_eq!(detect_format(src, Some("ofn")), OntFormat::Ofn);
    }

    #[test]
    fn rdf_xml_content_is_rdfxml() {
        let src = "<?xml version=\"1.0\"?>\n<rdf:RDF xmlns:rdf=\"...\">\n</rdf:RDF>";
        assert_eq!(detect_format(src, Some("owl")), OntFormat::RdfXml);
        // even with a misleading .ofn extension, the content wins
        assert_eq!(detect_format(src, Some("ofn")), OntFormat::RdfXml);
    }

    #[test]
    fn owl_xml_root_is_owx_even_with_owl_extension() {
        let src = "<?xml version=\"1.0\"?>\n<Ontology xmlns=\"http://www.w3.org/2002/07/owl#\"/>";
        assert_eq!(detect_format(src, Some("owl")), OntFormat::Owx);
    }

    #[test]
    fn ambiguous_xml_falls_back_to_extension() {
        let src = "<?xml version=\"1.0\"?>\n<something/>";
        assert_eq!(detect_format(src, Some("owx")), OntFormat::Owx);
        assert_eq!(detect_format(src, Some("owl")), OntFormat::RdfXml);
    }

    #[test]
    fn inconclusive_content_trusts_extension() {
        let src = "garbage that is neither";
        assert_eq!(detect_format(src, Some("owx")), OntFormat::Owx);
        assert_eq!(detect_format(src, Some("rdf")), OntFormat::RdfXml);
        assert_eq!(detect_format(src, Some("omn")), OntFormat::Omn);
        assert_eq!(detect_format(src, None), OntFormat::Ofn);
    }

    #[test]
    fn manchester_prefix_header_is_omn() {
        // Manchester uses the colon form `Prefix:` — distinct from OFN's
        // paren form `Prefix(` — so content wins even with a misleading ext.
        let src = "Prefix: : <urn:pizza#>\nOntology: <urn:pizza>\nClass: Pizza";
        assert_eq!(detect_format(src, Some("omn")), OntFormat::Omn);
        assert_eq!(detect_format(src, Some("ofn")), OntFormat::Omn);
    }

    #[test]
    fn manchester_bare_class_frame_is_omn() {
        // A Manchester document may open directly on a frame keyword.
        let src = "# header\n\nClass: Pizza\n    SubClassOf: Food";
        assert_eq!(detect_format(src, Some("omn")), OntFormat::Omn);
    }

    #[test]
    fn ofn_paren_prefix_not_confused_with_manchester() {
        // Regression: the OFN paren form must still win over the Manchester
        // colon sniff (the OFN check runs first and the forms don't collide).
        let src = "Prefix(:=<urn:#>)\nOntology()";
        assert_eq!(detect_format(src, Some("omn")), OntFormat::Ofn);
    }
}

#[cfg(test)]
mod manchester_parse_tests {
    use super::parse_ofn;
    use horned_owl::model::Component;
    use std::io::Write;

    /// The Manchester reader (wired via `OntFormat::Omn`) actually parses a
    /// `.omn` file end-to-end through `parse_ofn` — a content-sniffed
    /// Manchester source yields the expected `SubClassOf` axiom.
    #[test]
    fn parses_manchester_subclass_axiom() {
        let path = std::env::temp_dir().join("rustdl_omn_parse_subclass_test.omn");
        let mut f = std::fs::File::create(&path).expect("create temp .omn");
        write!(
            f,
            "Prefix: : <urn:p#>\nOntology: <urn:p>\nClass: Food\nClass: Pizza\n    SubClassOf: Food\n"
        )
        .expect("write temp .omn");
        drop(f);
        let onto = parse_ofn(&path).expect("parse Manchester ontology");
        let has_subclass = onto
            .iter()
            .any(|ac| matches!(ac.component, Component::SubClassOf(_)));
        std::fs::remove_file(&path).ok();
        assert!(
            has_subclass,
            "expected a SubClassOf axiom parsed from the Manchester source"
        );
    }
}

#[cfg(test)]
mod literal_parse_tests {
    use owl_dl_reasoner::justify::parse_literal_arg;

    #[test]
    fn parse_typed_integer() {
        let (lex, dt) = parse_literal_arg("\"5\"^^xsd:integer");
        assert_eq!(lex, "5");
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#integer");
    }

    #[test]
    fn parse_bare_string_defaults_to_xsd_string() {
        let (lex, dt) = parse_literal_arg("\"hi\"");
        assert_eq!(lex, "hi");
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#string");
    }

    #[test]
    fn parse_full_iri_datatype() {
        let (lex, dt) = parse_literal_arg("\"2.0\"^^<http://www.w3.org/2001/XMLSchema#double>");
        assert_eq!(lex, "2.0");
        assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#double");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod label_tests {
    use super::{build_label_map, local_name};
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;

    #[test]
    fn local_name_splits_on_hash_and_slash() {
        assert_eq!(local_name("http://x.org/foo#Bar"), "Bar");
        assert_eq!(local_name("http://x.org/path/Baz"), "Baz");
        assert_eq!(local_name("nocolon"), "nocolon");
    }

    #[test]
    fn label_map_picks_up_rdfs_labels_only() {
        let src = "\
Prefix(:=<http://t/>)\n\
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
Ontology(<http://t/o>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
AnnotationAssertion(rdfs:label :A \"Alpha\")\n\
AnnotationAssertion(rdfs:comment :B \"a comment, not a label\")\n\
SubClassOf(:A :B))\n";
        let (o, _): (SetOntology<RcStr>, _) = read_ofn(
            &mut std::io::Cursor::new(src),
            ParserConfiguration::default(),
        )
        .unwrap();
        let m = build_label_map(&o);
        assert_eq!(m.get("http://t/A").map(String::as_str), Some("Alpha"));
        // rdfs:comment is not a label, and B has no label
        assert_eq!(m.get("http://t/B"), None);
    }
}

#[cfg(test)]
mod global_budget_tests {
    use super::global_budget_after_parse;
    use std::time::Duration;

    /// `0` means unbounded, and stays unbounded no matter how long parsing took.
    #[test]
    fn zero_stays_unbounded() {
        assert_eq!(global_budget_after_parse(0, Duration::from_secs(30)), None);
    }

    /// The point of the change: parse time is CHARGED against the budget, so a
    /// 55 s budget after an 11 s parse leaves 44 s — not 55. Deleting the
    /// `saturating_sub` fails here.
    #[test]
    fn parse_time_is_charged_against_the_budget() {
        let budget = Duration::from_secs(55);
        let parse = Duration::from_millis(10_800);
        let left = global_budget_after_parse(55_000, parse).expect("a non-zero budget is bounded");
        assert_eq!(left, Duration::from_millis(44_200));
        assert!(
            left < budget,
            "budget must shrink by the parse, else the flag means 'N ms of reasoning'"
        );
    }

    /// A parse that outspends the whole budget floors at zero rather than
    /// underflowing (a `Duration` subtraction below zero panics) or wrapping to a
    /// huge budget — which would be an unbounded run wearing a deadline.
    #[test]
    fn parse_longer_than_budget_floors_at_zero() {
        assert_eq!(
            global_budget_after_parse(1_000, Duration::from_secs(9)),
            Some(Duration::ZERO)
        );
    }

    /// An instant parse leaves the budget intact — the change must not tax runs
    /// whose input is cheap to read, which is every in-tree fixture.
    #[test]
    fn negligible_parse_leaves_the_budget_intact() {
        let parse = Duration::from_micros(50);
        assert_eq!(
            global_budget_after_parse(30_000, parse),
            Duration::from_secs(30).checked_sub(parse)
        );
    }
}

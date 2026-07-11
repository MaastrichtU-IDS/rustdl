// Harness types/functions are consumed across the matrix submodules and by the
// `matrix` subcommand wired in the final task; allow the interim unused/visibility
// lints while the module tree is being built out task by task.
#![allow(dead_code, unreachable_pub)]

pub mod corpus;
pub use corpus::corpus_load_ofn;
pub mod correctness;
pub mod model;
pub mod provenance;
pub mod render;
pub mod run;

use crate::matrix::model::{
    Budgets, CellResult, RunMetadata, Status, append_cell, read_cells, write_metadata,
};
use crate::matrix::run::{TimedRun, timed};
use anyhow::Result;
use std::path::PathBuf;

pub struct MatrixArgs {
    pub tier: String,
    pub out: PathBuf,
    pub pair_timeout_ms: u64,
    pub global_timeout_s: u64,
    pub resume: bool,
    pub tools: PathBuf,
    pub work_dir: PathBuf,
    pub rustdl_bin: PathBuf,
    pub repo_root: PathBuf,
}

pub fn already_done(existing: &[CellResult], ont: &str, reasoner: &str) -> bool {
    existing
        .iter()
        .any(|c| c.ont == ont && c.reasoner == reasoner)
}

/// Map a `TimedRun` + an inconsistency signal to a `Status`.
fn status_of(run: &TimedRun, inconsistent: bool) -> Status {
    if inconsistent {
        return Status::Inconsistent;
    }
    if run.timed_out {
        return Status::Dnf;
    }
    // ROBOT exits 1 on unsat classes though reasoning completed; treat only
    // non-timeout, non-1 failures as errors when no output was produced.
    match run.exit_code {
        Some(0 | 1) => Status::Ok,
        _ => Status::Error,
    }
}

pub fn run_matrix(args: &MatrixArgs) -> Result<()> {
    let results = args.out.join("results.jsonl");
    let meta_path = args.out.join("run-metadata.json");
    let matrix_md = args.out.join("MATRIX.md");

    // Stage 0: fresh-binary guard + provenance.
    provenance::assert_fresh_binary(&args.rustdl_bin, &args.repo_root)?;
    let reasoners = provenance::capture_reasoners(&args.tools, &args.rustdl_bin, &args.repo_root)?;
    let oracle_id = reasoners
        .get("konclude")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .map_or_else(
            || "konclude".to_string(),
            |v| format!("konclude-{}", v.trim_start_matches('v')),
        );
    let meta = RunMetadata {
        date: iso_now(),
        tier: args.tier.clone(),
        oracle: oracle_id.clone(),
        host: provenance::capture_host(),
        budgets: Budgets {
            pair_timeout_ms: args.pair_timeout_ms,
            global_timeout_s: args.global_timeout_s,
        },
        reasoners,
    };
    write_metadata(&meta_path, &meta)?;

    // Stage 1: enumerate + normalize.
    let onts = corpus::enumerate(&args.tier, &args.work_dir, &args.tools)?;
    eprintln!("matrix: {} ontologies in tier {}", onts.len(), args.tier);

    let existing = if args.resume {
        read_cells(&results)?
    } else {
        Vec::new()
    };
    let robot = args.tools.join("bin/robot");
    let konclude = args.tools.join("bin/konclude");

    const REASONERS: [&str; 5] = ["rustdl", "konclude", "hermit", "elk", "whelk-rs"];

    for ont in &onts {
        // Resume short-circuit: if every reasoner for this ont is already
        // recorded, skip the ont entirely — including the expensive per-ont
        // Konclude oracle run, which would otherwise burn up to
        // `global_timeout_s` per already-complete ont on the big resumable tiers.
        if args.resume
            && REASONERS
                .iter()
                .all(|r| already_done(&existing, &ont.meta.name, r))
        {
            continue;
        }

        // Stage 2: oracle (Konclude) — run once per ont.
        let kon_out = ont.owx.with_extension("kon.owx");
        let kon_cmd = [
            konclude.to_str().expect("path is valid utf-8"),
            "classification",
            "-i",
            ont.owx.to_str().expect("path is valid utf-8"),
            "-o",
            kon_out.to_str().expect("path is valid utf-8"),
        ];
        let kon_run = timed(&kon_cmd, args.global_timeout_s)?;
        let oracle_verdict = if kon_run.timed_out || !kon_out.exists() {
            None
        } else {
            owl_dl_reasoner::oracle_diff::read_owx_verdict(&kon_out).ok()
        };

        // One cell per reasoner.
        for reasoner in REASONERS {
            if args.resume && already_done(&existing, &ont.meta.name, reasoner) {
                continue;
            }
            let cell = build_cell(
                args,
                ont,
                reasoner,
                oracle_verdict.as_ref(),
                &oracle_id,
                &kon_run,
                &robot,
            )?;
            append_cell(&results, &cell)?;
            eprintln!(
                "  {} / {} -> {:?} {} ms",
                ont.meta.name,
                reasoner,
                cell.status,
                cell.wall_ms.unwrap_or(0)
            );
        }
    }

    // Stage 4: render.
    let all = read_cells(&results)?;
    std::fs::write(&matrix_md, render::render_markdown(&meta, &all))?;
    eprintln!("matrix: wrote {}", matrix_md.display());
    Ok(())
}

fn build_cell(
    args: &MatrixArgs,
    ont: &corpus::StagedOnt,
    reasoner: &str,
    oracle: Option<&owl_dl_reasoner::oracle_diff::OwxVerdict>,
    oracle_id: &str,
    kon_run: &TimedRun,
    robot: &std::path::Path,
) -> Result<CellResult> {
    let el_only = matches!(reasoner, "elk" | "whelk-rs");
    let status: Status;
    let (mut wall, mut rss): (Option<u64>, Option<u64>) = (None, None);
    let mut correctness = None;

    if ont.meta.fragment == "convert-error" {
        status = Status::Error;
    } else if el_only && !corpus::is_el_fragment(&ont.meta.fragment) {
        status = Status::Na;
    } else {
        match reasoner {
            "konclude" => {
                wall = Some(kon_run.wall_ms);
                rss = Some(kon_run.peak_rss_mb);
                status = status_of(kon_run, false);
                // Konclude is the oracle -> FP/MISSED trivially 0 when it finished.
                if let Some(orc) = oracle {
                    correctness = Some(correctness::Correctness {
                        closure_size: orc.edges.len(),
                        fp: 0,
                        missed: 0,
                    });
                }
            }
            "rustdl" => {
                let cmd = [
                    args.rustdl_bin.to_str().expect("path is valid utf-8"),
                    "classify",
                    ont.ofn.to_str().expect("path is valid utf-8"),
                    "--pair-timeout-ms",
                    &args.pair_timeout_ms.to_string(),
                ];
                let r = timed(&cmd, args.global_timeout_s)?;
                wall = Some(r.wall_ms);
                rss = Some(r.peak_rss_mb);
                status = status_of(&r, false);
                if let (Status::Ok, Some(orc)) = (status, oracle) {
                    correctness =
                        correctness::rustdl_vs_oracle(ont, orc, args.pair_timeout_ms).ok();
                }
            }
            "hermit" | "elk" => {
                let engine = if reasoner == "hermit" {
                    "hermit"
                } else {
                    "elk"
                };
                let out = ont.owx.with_extension(format!("{engine}.owx"));
                let cmd = [
                    robot.to_str().expect("path is valid utf-8"),
                    "reason",
                    "--reasoner",
                    engine,
                    "--axiom-generators",
                    "subclass",
                    "-i",
                    ont.owl.to_str().expect("path is valid utf-8"),
                    "-o",
                    out.to_str().expect("path is valid utf-8"),
                ];
                let r = timed(&cmd, args.global_timeout_s)?;
                wall = Some(r.wall_ms);
                rss = Some(r.peak_rss_mb);
                // ROBOT errors on inconsistency. It prints
                // "ERROR ... The ontology is inconsistent. TIP: ..." to
                // STDOUT (exit code 1, stderr empty), so check both streams.
                let inconsistent = r.stdout.to_lowercase().contains("inconsistent")
                    || r.stderr.to_lowercase().contains("inconsistent");
                status = if inconsistent {
                    Status::Inconsistent
                } else if r.timed_out {
                    Status::Dnf
                } else if out.exists() {
                    Status::Ok
                } else {
                    Status::Error
                };
                if let (Status::Ok, Some(orc)) = (status, oracle) {
                    correctness = correctness::owx_vs_oracle(&out, orc).ok();
                }
            }
            "whelk-rs" => {
                // whelk runs in-process behind the `whelk-compare` feature; when
                // the feature is off, record `na` with a note rather than a fake number.
                status = Status::Na;
            }
            _ => unreachable!(),
        }
    }

    let (closure_size, fp, missed) = match correctness {
        Some(c) => (Some(c.closure_size), Some(c.fp), Some(c.missed)),
        None => (None, None, None),
    };
    Ok(CellResult {
        ont: ont.meta.name.clone(),
        source: ont.meta.source.clone(),
        sha256: ont.meta.sha256.clone(),
        size_bytes: ont.meta.size_bytes,
        classes: ont.meta.classes,
        fragment: ont.meta.fragment.clone(),
        reasoner: reasoner.into(),
        status,
        wall_ms: wall,
        peak_rss_mb: rss,
        closure_size,
        fp,
        missed,
        oracle: oracle_id.into(),
    })
}

/// ISO-8601 UTC timestamp without pulling in `chrono`: read `date -u`.
fn iso_now() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::model::{CellResult, Status};

    fn c(ont: &str, r: &str) -> CellResult {
        CellResult {
            ont: ont.into(),
            source: "s".into(),
            sha256: "h".into(),
            size_bytes: 1,
            classes: 1,
            fragment: "EL".into(),
            reasoner: r.into(),
            status: Status::Ok,
            wall_ms: Some(1),
            peak_rss_mb: Some(1),
            closure_size: Some(0),
            fp: Some(0),
            missed: Some(0),
            oracle: "k".into(),
        }
    }

    #[test]
    fn resume_skips_completed_cells_only() {
        let existing = vec![c("wine", "rustdl"), c("wine", "konclude")];
        assert!(already_done(&existing, "wine", "rustdl"));
        assert!(!already_done(&existing, "wine", "hermit"));
        assert!(!already_done(&existing, "pizza", "rustdl"));
    }
}

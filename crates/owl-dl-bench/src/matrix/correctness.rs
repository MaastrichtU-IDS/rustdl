use crate::matrix::corpus::StagedOnt;
use anyhow::{Context, Result};
use owl_dl_reasoner::classify_with_budget;
use owl_dl_reasoner::oracle_diff::{
    OwxVerdict, PairSet, aligned_closures, aligned_owx_closures, read_owx_verdict,
};
use std::path::Path;
use std::time::Duration;

/// FP/MISSED counts for one reasoner's classification against the Konclude
/// oracle's closure. `closure_size` is the reasoner's own closure size (the
/// denominator used for reporting percentages elsewhere in the matrix).
#[derive(Debug, Clone, Copy)]
pub struct Correctness {
    pub closure_size: usize,
    pub fp: usize,
    pub missed: usize,
}

/// Diff a reasoner's closure against the oracle's: FP = pairs the reasoner
/// asserts that the oracle does not (unsound); MISSED = pairs the oracle
/// asserts that the reasoner does not (incomplete).
pub fn diff_pairsets(reasoner: &PairSet, oracle: &PairSet) -> Correctness {
    let fp = reasoner.difference(oracle).count();
    let missed = oracle.difference(reasoner).count();
    Correctness {
        closure_size: reasoner.len(),
        fp,
        missed,
    }
}

/// Classify `ont` with rustdl's top-down classifier under a per-pair
/// deadline, align its closure against the oracle's, and diff.
///
/// `global_timeout_s` threads the matrix's `--global-timeout-s` knob into
/// this IN-PROCESS call as an explicit aggregate deadline. Before this fix,
/// that knob only bounded the SUBPROCESS reasoners (via `run::timed`,
/// including the subprocess `rustdl classify` run used for wall/RSS) — this
/// correctness call classified again in-process with only a per-pair bound,
/// so a pathological ontology (e.g. `ore_ont_10080`) could hang the matrix
/// even though the subprocess cell for the same ontology had already
/// finished (or been killed) within `global_timeout_s`.
pub fn rustdl_vs_oracle(
    ont: &StagedOnt,
    oracle: &OwxVerdict,
    pair_ms: u64,
    global_timeout_s: u64,
) -> Result<Correctness> {
    let onto = crate::matrix::corpus_load_ofn(&ont.ofn)?;
    let c = classify_with_budget(
        &onto,
        Some(Duration::from_millis(pair_ms)),
        Some(Duration::from_secs(global_timeout_s)),
    )
    .context("rustdl classify")?;
    let (rustdl, oracle_pairs) = aligned_closures(&c, oracle);
    Ok(diff_pairsets(&rustdl, &oracle_pairs))
}

/// Read a reasoner's own `.owx` output, align its closure against the
/// oracle's, and diff. Used for the non-rustdl reasoners (HermiT/ELK/whelk)
/// whose verdicts are captured as `.owx` files rather than run in-process.
pub fn owx_vs_oracle(reasoner_owx: &Path, oracle: &OwxVerdict) -> Result<Correctness> {
    let v = read_owx_verdict(reasoner_owx)?;
    let (reasoner, oracle_pairs) = aligned_owx_closures(&v, oracle);
    Ok(diff_pairsets(&reasoner, &oracle_pairs))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use owl_dl_reasoner::oracle_diff::PairSet;

    fn ps(pairs: &[(&str, &str)]) -> PairSet {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn fp_and_missed_counted_against_oracle() {
        let oracle = ps(&[("A", "B"), ("B", "C"), ("A", "C")]);
        let reasoner = ps(&[("A", "B"), ("A", "C"), ("A", "D")]); // missing B<C; extra A<D
        let c = diff_pairsets(&reasoner, &oracle);
        assert_eq!(c.closure_size, 3);
        assert_eq!(c.fp, 1); // A<D is unsound
        assert_eq!(c.missed, 1); // B<C missed
    }

    #[test]
    fn identical_closures_are_clean() {
        let o = ps(&[("A", "B")]);
        let c = diff_pairsets(&o.clone(), &o);
        assert_eq!(c.fp, 0);
        assert_eq!(c.missed, 0);
    }
}

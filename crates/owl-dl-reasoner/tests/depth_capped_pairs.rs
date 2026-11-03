//! #288: a main-tableau probe cut by the search's decision-depth limit
//! (`MAX_SEARCH_DEPTH`) is counted in `ClassificationStats::depth_capped_pairs`,
//! so it is not reported as a timeout. `X ⊑ (A₀ ⊔ B₀) ⊓ … ⊓ (Aₙ₋₁ ⊔ Bₙ₋₁)` with
//! independent unions needs `n` decision levels for `X`'s unsat probe: above the
//! limit the probe is cut in milliseconds however large the per-pair budget is;
//! below it, it is decided.
//!
//! Sabotage: make `last_probe_depth_capped` always return `false`, and
//! `a_probe_past_the_depth_limit_is_counted_as_depth_capped` fails.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{MAX_SEARCH_DEPTH, classify_with_timeout};

fn unions(n: usize) -> SetOntology<RcStr> {
    let mut src =
        String::from("Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n");
    src.push_str("SubClassOf(:X ObjectIntersectionOf(");
    for k in 0..n {
        write!(src, " ObjectUnionOf(:A{k} :B{k})").unwrap();
    }
    src.push_str("))\n)\n");
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// A budget far above what the depth-limit cut costs, so a deadline cannot be
/// what stops the probe, even on a slow debug build.
const BUDGET: Duration = Duration::from_secs(20);

#[test]
fn a_probe_past_the_depth_limit_is_counted_as_depth_capped() {
    let c = classify_with_timeout(&unions(MAX_SEARCH_DEPTH + 20), BUDGET).unwrap();
    let s = c.stats();
    assert!(s.timed_out_pairs >= 1, "fixture must cut a probe: {s:?}");
    assert_eq!(
        s.depth_capped_pairs, s.timed_out_pairs,
        "every cut here is a depth-limit cut, none a timeout"
    );
}

#[test]
fn a_probe_within_the_depth_limit_is_decided() {
    let c = classify_with_timeout(&unions(MAX_SEARCH_DEPTH - 56), BUDGET).unwrap();
    let s = c.stats();
    assert_eq!(s.timed_out_pairs, 0, "{s:?}");
    assert_eq!(s.depth_capped_pairs, 0);
}

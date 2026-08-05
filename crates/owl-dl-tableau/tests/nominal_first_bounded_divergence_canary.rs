//! Task 5 (issue #35 v4): a FAST, always-on companion to the `#[ignore]`d
//! load-bearing gate in `nominal_first_bounded.rs`.
//!
//! That gate (`issue35_v4_completion_graph_is_bounded`) currently FAILS —
//! with `RUSTDL_MAX_NODES=0` (cap OFF) the reproducer's completion graph
//! does not converge, even with `RUSTDL_NOMINAL_FIRST=1` (the fix). This
//! test proves the same divergence WITHOUT hanging: under a small,
//! finite node cap the search still cannot reach `Sat`/`Unsat` on its own
//! — it hits `NodeCap` — demonstrating the graph blows past a bound far
//! smaller than any genuine model of this ontology needs (a hand-built
//! model exists with 3 named individuals + 2 fresh `C`-witnesses, i.e. ~6
//! nodes; see the task-5 report).
//!
//! Task B (safety net) made `NodeCap` a HARD early-return in
//! `search::branch` (a global node-cap trip abandons the remaining
//! sibling disjuncts instead of soft-retrying them) — sound, since
//! `NodeCap` maps to a MISS, never a false positive. The **observable
//! signature of the divergence changed** as a result: it used to be
//! "`graph_len` tracks `cap - 1`, final verdict `Sat`-via-backtrack";
//! it is now "verdict `NodeCap`, `graph_len` rolled all the way back to
//! (near-)initial size" — the hard early-return unwinds the diverging
//! branch's nodes on the way out instead of leaving them packed to the
//! cap. Both signatures equally prove the same underlying fact: this
//! reproducer's search does not converge within a small cap. If a future
//! fix genuinely bounds this reproducer, this test will start failing
//! (the verdict will become `Sat`/`Unsat` even at a tiny cap) — that is
//! the intended signal to promote `nominal_first_bounded.rs`'s ignored
//! gate back to live.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{ConceptExpr, RoleHierarchyBuilder, absorb, convert_ontology, nnf_axioms};
use owl_dl_tableau::{SearchVerdict, TableauContext};
use std::io::Cursor;

const REPRODUCER: &str = "\
Prefix(:=<http://example.org/card#>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))
  SubClassOf(:A ObjectOneOf(:x :y :z))
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
  ObjectPropertyDomain(:r :A)
)
";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _prefixes) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    onto
}

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. This test file
        // contains exactly one test, so there is no cross-test race on
        // process-wide env state; restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Same probe as `nominal_first_bounded.rs`'s `build_reproducer_probe`,
/// but returning the final graph size alongside the verdict, and taking a
/// max-depth cap short of `DEEP_SEARCH_DEPTH` (unnecessary here since the
/// live-node cap — not depth — is what stops this search quickly).
fn run_probe() -> (SearchVerdict, usize) {
    let onto = parse(REPRODUCER);
    let mut internal = convert_ontology(&onto).expect("convert");
    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    let hierarchy = RoleHierarchyBuilder::with_roles(
        u32::try_from(internal.vocabulary.num_roles()).expect("role count fits u32"),
    )
    .build();
    let b_class = internal
        .vocabulary
        .class_id("http://example.org/card#B")
        .expect(":B declared");
    let b_concept = internal.concepts.atomic(b_class);
    debug_assert!(matches!(
        internal.concepts.get(b_concept),
        ConceptExpr::Atomic(c) if *c == b_class
    ));

    let mut ctx = TableauContext::with_tbox_and_hierarchy(&internal.concepts, &tbox, &hierarchy);
    ctx.set_anywhere_blocking(true);
    let test_root = ctx.new_node();
    ctx.add_label(test_root, b_concept);
    let verdict = owl_dl_tableau::search(&mut ctx, 1_000_000);
    (verdict, ctx.graph().len())
}

/// FAST divergence proof (small cap, fix ON): the search does NOT converge
/// on its own — it trips the global node cap (`NodeCap`) rather than
/// reaching `Sat`/`Unsat`. Post-Task-B, `search::branch` treats `NodeCap`
/// as a HARD early-return: the diverging branch's nodes are rolled back on
/// the way out and no sibling disjunct is retried, so the *final* graph
/// size collapses back to (near-)its pre-branch size rather than tracking
/// the cap — the verdict tag (`NodeCap`) is now the load-bearing signal of
/// divergence, not `graph_len`.
///
/// **Cap pinned to 10, not the Step-1 gate's 64-node threshold.** A SEPARATE
/// pre-existing bug (independent of this task, reachable through the real
/// `owl_dl_reasoner::is_class_satisfiable`/CLI `sat` path too, not just this
/// probe) trips a `debug_assert_eq!` in
/// `TableauContext::remove_edge_recorded` (`crates/owl-dl-tableau/src/lib.rs`
/// around line 1529 — an edge/in-edge index mismatch during rollback) once
/// this reproducer's graph exceeds roughly 10-19 nodes, ONLY in
/// debug-assertion builds (release is unaffected; CI's `cargo test
/// --workspace --all-targets` runs the debug profile). Confirmed via
/// `RUSTDL_MAX_NODES=20..70` against a debug CLI build: every value panics;
/// `=10` does not. Using cap 10 here keeps this canary crash-free in CI
/// while still demonstrating the divergence (a `NodeCap` verdict); see the
/// task-5 report for the full repro and a recommendation to file the
/// edge-index bug separately.
///
/// If a future fix genuinely bounds this reproducer, the verdict will
/// become `Sat`/`Unsat` even at this tiny cap — that is the signal to
/// promote `nominal_first_bounded.rs`'s `#[ignore]`d gate back to live.
#[test]
fn issue35_v4_reproducer_diverges_graph_len_tracks_cap() {
    let _fix = SetEnvGuard::set("RUSTDL_NOMINAL_FIRST", "1");
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "10");
    // PINNED OFF 2026-08-05. `RUSTDL_DOMAIN_ABSORPTION` became the default that
    // day and it CLOSES this reproducer — the sibling gate
    // `issue35_v4_completion_graph_is_bounded` was promoted from `#[ignore]` on
    // the strength of it (cap OFF: 300 s+ hang at `=0`, 0.00 s at the default).
    // So this test no longer characterises the shipped configuration; it now
    // characterises the NodeCap safety net on a deliberately UN-absorbed one,
    // which is still worth keeping, because that net protects every ontology
    // domain absorption does not happen to cure. Without this pin the test
    // reports `Sat` and fails.
    let _no_domain_absorption = SetEnvGuard::set("RUSTDL_DOMAIN_ABSORPTION", "0");
    let (verdict, len) = run_probe();
    assert!(
        matches!(verdict, SearchVerdict::NodeCap),
        "expected a raw NodeCap trip (hard early-return), got {verdict:?}"
    );
    // Post-hard-early-return the diverging branch's nodes are rolled back
    // on the way out, so the graph collapses back to near its initial size
    // instead of tracking the cap. A little slack for the root + a handful
    // of deterministically-derived labels/successors that aren't rolled
    // back because they predate the diverging branch.
    assert!(
        len < 10,
        "expected graph_len to have unwound below the cap, got {len}"
    );
}

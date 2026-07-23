//! Task 5 (issue #35 v4), Step 1: the LOAD-BEARING cap-disabled,
//! tableau-layer bounded-node gate.
//!
//! Builds the reproducer's completion probe directly (mirroring the real
//! `owl_dl_reasoner::decide` seeding: `convert_ontology` -> NNF -> `absorb`
//! -> `TableauContext::with_tbox_and_hierarchy`, anywhere-blocking on, a
//! fresh anonymous root labelled with the test concept, then
//! `owl_dl_tableau::search`) and — per the task-5 brief — asserts the
//! completion graph stays under 64 nodes with `RUSTDL_MAX_NODES=0` (cap
//! OFF) and `RUSTDL_NOMINAL_FIRST=1` (fix ON).
//!
//! # HONESTY REQUIREMENT: this assertion currently FAILS (hangs)
//!
//! Empirically (see `.superpowers/sdd/task-5-report.md` for the full
//! diagnosis and measurements), the 3-axiom reproducer core —
//! `SubClassOf(A, ObjectOneOf(x,y,z))` + `EquivalentClasses(B,
//! ObjectIntersectionOf(A, ObjectMinCardinality(2, r, C)))` +
//! `ObjectPropertyDomain(r, A)` — does **not** terminate with the cap
//! disabled, even with `RUSTDL_NOMINAL_FIRST=1`. Diagnosis (confirmed via
//! `tbox-stats`/`residual-triggers` on the reproducer and by selectively
//! dropping each axiom):
//!
//! `ObjectPropertyDomain(r, A)` absorbs to a **universal, untriggered**
//! Or-shaped residual GCI `⊤ ⊑ ¬∃r.⊤ ⊔ A` (confirmed: `tbox-stats` reports
//! `residual_gcis: 1` / `defer_or: 1`, and it disappears when the domain
//! axiom is dropped). `has_pending_nominal_disjunction` — the Task 2
//! guard consulted by `apply_exists`/`apply_min` (Task 3) — only inspects
//! `tbox.concept_rules{,_by_trigger}` keyed by an atomic trigger label; it
//! has no visibility into this universal residual, which applies to
//! *every* node (including freshly-generated `≥2 r.C` witnesses) once
//! materialised by `apply_deferred_or_residuals`. When the search chooses
//! the `A` disjunct on a fresh witness, the covering axiom's nominal `Or`
//! then forces that witness to merge into one of `{x,y,z}` — an
//! individual that may already be the *owner* of the very `≥2 r.C`
//! constraint being satisfied. Folding a cardinality witness into its own
//! owner drops the pairwise-distinct-witness count below the required
//! threshold, so `apply_min` regenerates a replacement witness — which is
//! itself eligible for the same domain-residual branch — an unbounded
//! generate/merge/regenerate cascade the nominal-first fix (Tasks 1-4,
//! scoped only to the covering-axiom disjunction) does not gate.
//!
//! Confirmed empirically (`rustdl sat` on the reproducer, cap swept):
//! `RUSTDL_MAX_NODES=500` -> 0.5s, `=2000` -> 19.4s, `=8000` -> exceeds
//! 30s — steep superlinear wall growth in the cap, the signature of an
//! actual combinatorial cascade, not a slow-but-convergent search.
//! Dropping either `ObjectPropertyDomain(r,A)` or the `ObjectOneOf`
//! covering axiom alone (keeping the other two) terminates in well under
//! a second (see `nominal_first_minimality_variants.rs`), confirming the
//! interaction (not either construct alone) is the cause.
//!
//! Per the task-5 brief's HONESTY REQUIREMENT, this test is **not** forced
//! green: it is `#[ignore]`d so `cargo test` never hangs on it. Run it
//! manually, under a shell timeout, to reproduce:
//! `RUSTUP_TOOLCHAIN=stable timeout 30 cargo test -p owl-dl-tableau --test
//! nominal_first_bounded -- --ignored` (exit 124 = confirmed non-
//! termination). See `nominal_first_bounded_divergence_canary.rs` for a
//! FAST, always-on regression test that documents the same divergence
//! without hanging (small cap -> `NodeCap`, not `Sat`/`Unsat`).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{ConceptExpr, RoleHierarchyBuilder, absorb, convert_ontology, nnf_axioms};
use owl_dl_tableau::{SearchVerdict, TableauContext};
use std::io::Cursor;

/// The issue-#35 v4 reproducer core (see task-5 brief): a `SubClassOf`
/// covering-nominal disjunction, a defined class combining that covering
/// class with a `≥2` cardinality, and a property-domain axiom binding the
/// role back to the covering class.
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
        // contains exactly one (ignored) test, so there is no cross-test
        // race on process-wide env state; restored on Drop.
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

/// Build the reproducer's tableau-layer probe: is `:B` satisfiable? Mirrors
/// `owl_dl_reasoner::decide`'s seeding (convert -> NNF -> absorb -> fresh
/// anonymous root labelled with the test concept), minus the `ABox`
/// scaffolding the reasoner also runs (this ontology has no `ABox` axioms
/// — `x`/`y`/`z` are referenced only via the `ObjectOneOf` nominal
/// concept, never asserted — so `collect_abox`-equivalent seeding would be
/// a no-op here). The role hierarchy is a trivial reflexive-only closure
/// (`RoleHierarchyBuilder::with_roles(n).build()`), which is exactly what
/// the reasoner's `build_role_hierarchy` would also produce for this
/// ontology: no `SubObjectPropertyOf`/`InverseObjectProperties`/
/// `SymmetricObjectProperty` axioms are present.
fn build_reproducer_probe() -> SearchVerdict {
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
    // `ConceptPool::atomic` needs `&mut`; `B`'s atomic concept is already
    // interned by `convert_ontology`/`absorb`, so this is a cheap lookup —
    // done before the pool is borrowed immutably by the tableau context.
    let b_concept = internal.concepts.atomic(b_class);
    debug_assert!(matches!(
        internal.concepts.get(b_concept),
        ConceptExpr::Atomic(c) if *c == b_class
    ));

    let mut ctx = TableauContext::with_tbox_and_hierarchy(&internal.concepts, &tbox, &hierarchy);
    // Mirror `owl_dl_reasoner::decide`'s deadline-free-path default:
    // anywhere-blocking ON (Motik/Shearer/Horrocks) — required so this
    // probe faces the same blocking regime the real reasoner uses on the
    // `sat`/un-timed `realize` query paths.
    ctx.set_anywhere_blocking(true);
    let test_root = ctx.new_node();
    ctx.add_label(test_root, b_concept);
    // Mirror `owl_dl_reasoner::decide`'s deadline-free path: run on a
    // dedicated 1 GiB-stack thread at `DEEP_SEARCH_DEPTH` (1_000_000). The
    // default test-harness thread stack is far too small for this probe's
    // recursion depth — verified: without this, the probe stack-overflows
    // (SIGABRT) rather than diverging cleanly, which would misrepresent the
    // real reasoner path's behaviour (a hang, not a crash).
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(1024 * 1024 * 1024)
            .spawn_scoped(scope, || owl_dl_tableau::search(&mut ctx, 1_000_000))
            .expect("spawn deep tableau search thread")
            .join()
            .expect("deep tableau search thread panicked")
    })
}

/// See the module doc: this assertion is the brief's Step-1 load-bearing
/// gate and CURRENTLY FAILS (the probe does not terminate) — Tasks 1-4 do
/// not close this reproducer. `#[ignore]`d so `cargo test` never hangs;
/// run manually under a shell `timeout` to reproduce the finding.
#[test]
#[ignore = "issue #35 v4: reproducer does NOT terminate even with the fix on \
            (cap OFF) — see module doc + task-5-report.md. Run manually under \
            `timeout` to confirm non-termination; do not remove this ignore \
            without first fixing the domain-residual/cardinality-merge gap."]
fn issue35_v4_completion_graph_is_bounded() {
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "0");
    let _fix = SetEnvGuard::set("RUSTDL_NOMINAL_FIRST", "1");
    let verdict = build_reproducer_probe();
    assert!(
        matches!(verdict, SearchVerdict::Sat | SearchVerdict::Unsat(_)),
        "must decide, not stall: {verdict:?}"
    );
}

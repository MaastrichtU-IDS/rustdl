//! Task 5 (issue #35 v4): a FAST, always-on companion to the gate in
//! `nominal_first_bounded.rs`, run with `RUSTDL_DOMAIN_ABSORPTION=0`.
//!
//! Until #284 this reproducer diverged with domain absorption off: under a small
//! node cap the search tripped `NodeCap` instead of reaching a verdict, and this
//! file asserted exactly that. #284 found the cause, rules firing on a node the
//! o-rule had already merged away, and the reproducer now converges to `Sat`
//! under a 10-node cap. The test below pins that.

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

/// The reproducer CONVERGES since #284, even with domain absorption pinned off
/// and a 10-node cap: the verdict is `Sat` (a hand-built model has ~6 nodes).
///
/// This test used to assert a `NodeCap` trip, as the always-on proof that the
/// un-absorbed reproducer diverged. The divergence was the #284 mechanism: the
/// o-rule merged the fresh `≥2 r.C` witness into a nominal node, and the
/// saturator kept firing rules on the merged-away witness, whose `≥2 r.C` looked
/// unsatisfied once its edges had moved, so it generated again, forever.
/// Merged-away nodes are now skipped. The old header said to flip this test
/// once a fix bounds the reproducer, and this version does that. The `NodeCap`
/// safety net itself stays covered by `owl-dl-reasoner`'s `node_cap.rs`,
/// `disjoint_nodecap.rs` and `individuals_nodecap.rs`.
#[test]
fn issue35_v4_reproducer_converges_without_domain_absorption() {
    let _fix = SetEnvGuard::set("RUSTDL_NOMINAL_FIRST", "1");
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "10");
    // Pinned off so this keeps exercising the un-absorbed residual
    // `⊤ ⊑ ¬∃r.⊤ ⊔ A`; the default (domain absorption ON) never opens the cycle.
    let _no_domain_absorption = SetEnvGuard::set("RUSTDL_DOMAIN_ABSORPTION", "0");
    let (verdict, len) = run_probe();
    assert!(
        matches!(verdict, SearchVerdict::Sat),
        "expected the reproducer to converge to Sat under a 10-node cap, got {verdict:?}"
    );
    assert!(len < 10, "expected a model under the cap, got {len} nodes");
}

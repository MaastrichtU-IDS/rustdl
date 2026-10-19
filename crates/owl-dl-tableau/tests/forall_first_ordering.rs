//! #288: the search tries `∀` disjuncts before atomic ones.
//!
//! `fixtures/forall_first_core.ofn` is six definitions from PMD core with the
//! names replaced. Each `D ≡ ∃R.C` leaves the disjunction `∀R.¬C ⊔ D` on every
//! node. With `D` tried first, `D ⊑ ∃R.C` builds a successor carrying the same
//! disjunctions, and `sat` of an unconstrained class grew a 361-node graph in
//! ~4 CPU-s without one clash. With the `∀` disjunct first, nothing is generated
//! and the model is the root alone.
//!
//! One test, both arms: they share `RUSTDL_FORALL_FIRST`, a process-wide
//! variable, and the context reads it at construction.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{RoleHierarchyBuilder, absorb, convert_ontology, nnf_axioms};
use owl_dl_tableau::{SearchVerdict, TableauContext};
use std::io::Cursor;

const CORE: &str = include_str!("fixtures/forall_first_core.ofn");

struct EnvGuard(&'static str, Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: this file holds one test, so nothing else reads the
        // environment concurrently; restored on drop.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

/// `sat X` for the unconstrained class `X`; returns the verdict and graph size.
/// Runs on a 256 MB stack, which the uncapped old order needs (it recurses
/// ~2,600 branches deep).
fn probe() -> (SearchVerdict, usize) {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(probe_inner)
        .expect("spawn probe thread")
        .join()
        .expect("probe thread")
}

fn probe_inner() -> (SearchVerdict, usize) {
    let mut reader = Cursor::new(CORE);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    let mut internal = convert_ontology(&onto).expect("convert");
    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    let hierarchy = RoleHierarchyBuilder::with_roles(
        u32::try_from(internal.vocabulary.num_roles()).expect("role count fits u32"),
    )
    .build();
    let x = internal
        .vocabulary
        .class_id("http://t/X")
        .expect(":X declared");
    let x = internal.concepts.atomic(x);
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&internal.concepts, &tbox, &hierarchy);
    ctx.set_anywhere_blocking(true);
    let root = ctx.new_node();
    ctx.add_label(root, x);
    let verdict = owl_dl_tableau::search(&mut ctx, 1_000_000);
    (verdict, ctx.graph().len())
}

#[test]
fn forall_disjuncts_are_tried_first() {
    // A 100-node cap keeps the old-order control fast (uncapped it builds 361
    // nodes, ~35 s in a debug build). The cap is cached per process, so it is
    // set before either probe; the forall-first arm never comes near it.
    let _cap = EnvGuard::set("RUSTDL_MAX_NODES", "100");
    assert_eq!(
        owl_dl_tableau::max_nodes(),
        Some(100),
        "the node cap was read before this test set it; the control arm would be uncapped"
    );

    let (verdict, nodes) = {
        let _on = EnvGuard::set("RUSTDL_FORALL_FIRST", "1");
        probe()
    };
    assert!(matches!(verdict, SearchVerdict::Sat), "got {verdict:?}");
    assert_eq!(nodes, 1, "expected the root alone");

    // Control: with the old order the same search generates past the cap, so
    // the assertion above depends on the ordering and not on the fixture alone.
    let (verdict, _) = {
        let _off = EnvGuard::set("RUSTDL_FORALL_FIRST", "0");
        probe()
    };
    assert!(
        matches!(verdict, SearchVerdict::NodeCap),
        "expected the old order to generate past 100 nodes, got {verdict:?}"
    );
}

//! #288: incremental saturation changes the work `saturate` does, not the
//! search.
//!
//! With `RUSTDL_INCREMENTAL_SATURATE` on, a `saturate` call after a fixpoint
//! plus label additions skips nodes with no rule work but fires the remaining
//! rules in the order a full pass would. A first version reached the same
//! fixpoints by another order; that steered the search elsewhere, and PMD core's
//! `PMD_0020213` went from 44 CPU-s to no answer in 300 s. So each case below
//! runs the search both ways and asserts the same verdict (dependency sets
//! included) and the same final graph, node by node.
//!
//! One test, both arms: the flag is a process-wide variable that the context
//! reads at construction.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{RoleHierarchyBuilder, absorb, convert_ontology, nnf_axioms};
use owl_dl_tableau::TableauContext;
use std::io::Cursor;

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

/// The `∀`-first core (#288) searched in the old disjunct order, which builds
/// nodes, blocks them and backtracks until the node cap stops it.
const FORALL_CORE: &str = include_str!("fixtures/forall_first_core.ofn");

/// A cycle with a disjunction and `∀` on each node, so blocking and branching
/// interleave.
const CYCLE: &str = "Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(
SubClassOf(:X ObjectSomeValuesFrom(:r :X))
SubClassOf(:X ObjectUnionOf(:B :C))
SubClassOf(:B ObjectAllValuesFrom(:r :D))
SubClassOf(:C ObjectSomeValuesFrom(:s ObjectUnionOf(:E :F)))
SubClassOf(ObjectIntersectionOf(:D :E) owl:Nothing)
SubClassOf(:D ObjectSomeValuesFrom(:s :E))
)";

/// `≤2` over three witnesses, the clash two levels down (#76).
const LATE_MERGE: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:C1 :C) SubClassOf(:C2 :C) SubClassOf(:C3 :C)
SubClassOf(:X ObjectMaxCardinality(2 :s :C))
SubClassOf(:X ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C1 ObjectAllValuesFrom(:p :Y))))
SubClassOf(:X ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C2 ObjectSomeValuesFrom(:p ObjectComplementOf(:Y)))))
SubClassOf(:X :D1) SubClassOf(:D1 :D2) SubClassOf(:D2 ObjectSomeValuesFrom(:s :C3))
)";

/// `≤1 r.C` under a disjunction, so the merge and the choose rule run inside
/// a branch (#299).
const LE1_UNDER_DISJUNCT: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:X ObjectMaxCardinality(1 :r :C))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C ObjectAllValuesFrom(:p :Y))))
SubClassOf(:X ObjectUnionOf(:B1 :B2))
SubClassOf(:B1 ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C ObjectSomeValuesFrom(:p ObjectComplementOf(:Y)))))
SubClassOf(:B2 ObjectSomeValuesFrom(:r :E))
)";

/// An unqualified `≤1` over four witnesses: three forced merges.
const FAN: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:X ObjectMaxCardinality(1 :r))
SubClassOf(:X ObjectSomeValuesFrom(:r :B1))
SubClassOf(:X ObjectSomeValuesFrom(:r :B2))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectUnionOf(:B3 :B4)))
SubClassOf(:B4 ObjectSomeValuesFrom(:r :B1))
)";

/// Disjunctions and an `∃` chain, found by fuzzing. Dirtying every touched
/// neighbour at once, as the first version did, reruns rules on nodes a full
/// pass would only revisit on the next call, and the search here diverges.
const EAGER_DIRTY: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:X ObjectUnionOf(:C5 :C8))
SubClassOf(:X ObjectUnionOf(:C0 ObjectComplementOf(:C6)))
SubClassOf(:X ObjectUnionOf(:C3 :C6))
SubClassOf(:C3 ObjectSomeValuesFrom(:t :C0))
SubClassOf(:C0 ObjectUnionOf(:C3 :C8))
SubClassOf(:C0 ObjectSomeValuesFrom(:r :C9))
SubClassOf(:C9 ObjectSomeValuesFrom(:s ObjectUnionOf(:C3 :C5)))
)";

/// The verdict (with its dependency set) and every node's labels after `sat X`.
type Outcome = (String, Vec<Vec<owl_dl_core::ConceptId>>);

fn probe(src: &'static str) -> Outcome {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || probe_inner(src))
        .expect("spawn probe thread")
        .join()
        .expect("probe thread")
}

fn probe_inner(src: &str) -> Outcome {
    let mut reader = Cursor::new(src);
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
    let labels = (0..ctx.graph().len())
        .map(|i| {
            let n = owl_dl_tableau::NodeId::new(u32::try_from(i).expect("fits"));
            ctx.graph().node(n).labels().to_vec()
        })
        .collect();
    (format!("{verdict:?}"), labels)
}

#[test]
fn incremental_saturation_makes_the_same_search() {
    // The cap stops the old-order `∀` core; it is cached per process, so it is
    // set before any probe.
    let _cap = EnvGuard::set("RUSTDL_MAX_NODES", "100");
    assert_eq!(owl_dl_tableau::max_nodes(), Some(100));

    // `RUSTDL_FORALL_FIRST` per case: the old order makes the `∀` core search.
    for (name, src, forall_first) in [
        ("forall core, old order", FORALL_CORE, "0"),
        ("cycle", CYCLE, "0"),
        ("late merge", LATE_MERGE, "0"),
        ("le1 under a disjunct", LE1_UNDER_DISJUNCT, "0"),
        ("fan", FAN, "0"),
        ("eager dirtying", EAGER_DIRTY, "1"),
    ] {
        let _order = EnvGuard::set("RUSTDL_FORALL_FIRST", forall_first);
        let full = {
            let _off = EnvGuard::set("RUSTDL_INCREMENTAL_SATURATE", "0");
            probe(src)
        };
        let incremental = {
            let _on = EnvGuard::set("RUSTDL_INCREMENTAL_SATURATE", "1");
            probe(src)
        };
        assert_eq!(full.0, incremental.0, "{name}: verdicts differ");
        assert_eq!(
            full.1.len(),
            incremental.1.len(),
            "{name}: graph sizes differ"
        );
        for (i, (a, b)) in full.1.iter().zip(&incremental.1).enumerate() {
            assert_eq!(a, b, "{name}: labels of node {i} differ");
        }
    }
}

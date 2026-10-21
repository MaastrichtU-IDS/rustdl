//! `≥n` over more than `MAX_CLIQUE_SEARCH` candidate witnesses terminates
//! (#304, #306 review).
//!
//! `has_distinct_witnesses` gives up searching for `n` pairwise-distinct
//! witnesses past 32 candidates. It first answered `false` there, so
//! `apply_min` made `n` fresh witnesses on every saturation pass, each pass
//! adding candidates, until the node cap: `≥33 r.C` alone never finished. The
//! reasoner reads a node cap as `sat`, so only the graph size tells the two
//! apart.
//!
//! One test: the node cap is a process-wide variable read once.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{RoleHierarchyBuilder, absorb, convert_ontology, nnf_axioms};
use owl_dl_tableau::{SearchVerdict, TableauContext};
use std::io::Cursor;

/// `≥33 r.C`: past the search cap from the first pass. `HermiT`: satisfiable.
const MIN_33: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:X ObjectMinCardinality(33 :r :C))
)";

/// 40 existing `C`-witnesses, none known distinct, and `≥2 r.C`. `HermiT`:
/// satisfiable.
const MANY_EXISTING: &str = "Prefix(:=<http://t/>)
Ontology(
SubClassOf(:X ObjectMinCardinality(2 :r :C))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W0)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W1)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W2)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W3)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W4)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W5)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W6)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W7)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W8)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W9)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W10)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W11)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W12)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W13)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W14)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W15)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W16)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W17)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W18)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W19)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W20)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W21)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W22)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W23)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W24)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W25)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W26)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W27)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W28)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W29)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W30)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W31)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W32)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W33)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W34)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W35)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W36)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W37)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W38)))
SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :W39)))
)";

#[allow(unsafe_code)]
fn set_env(key: &str, value: &str) {
    // SAFETY: this file holds one test, so nothing else reads the environment
    // concurrently.
    unsafe { std::env::set_var(key, value) };
}

/// `sat X`: the verdict and the final graph size.
fn probe(src: &str) -> (SearchVerdict, usize) {
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
    (verdict, ctx.graph().len())
}

#[test]
fn a_min_past_the_search_cap_terminates() {
    // A low cap keeps the looping version fast: it ends in `NodeCap`.
    set_env("RUSTDL_MAX_NODES", "500");
    set_env("RUSTDL_MIN_FRESH_WITNESSES", "1");
    assert_eq!(owl_dl_tableau::max_nodes(), Some(500));

    let (verdict, nodes) = probe(MIN_33);
    assert!(
        matches!(verdict, SearchVerdict::Sat),
        "≥33: got {verdict:?}"
    );
    assert!(
        nodes <= 34,
        "≥33: expected the root and 33 witnesses, got {nodes} nodes"
    );

    let (verdict, nodes) = probe(MANY_EXISTING);
    assert!(
        matches!(verdict, SearchVerdict::Sat),
        "many existing: got {verdict:?}"
    );
    // The root, the 40, and at most one fresh pair: `apply_min` can fire once
    // before the 40 carry `C`, which comes from decomposing their filler. This
    // relies on `saturate` running `apply_exists` before `apply_min` on the
    // root; if that order changes, re-derive the bound rather than read a
    // larger graph as the loop coming back.
    assert!(
        nodes <= 43,
        "many existing: expected at most one fresh pair, got {nodes} nodes"
    );
}

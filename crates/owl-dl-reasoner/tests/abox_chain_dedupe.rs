//! #447: Rule 4 of the `ABox` saturation (role chains, transitivity) first
//! checks a candidate in a small per-anchor cache of known conclusions, and
//! looks it up in the global edge set only on a miss.
//!
//! Why this is pinned by counters and not by a timing: on `family.ofn` the
//! join tries ~101 M candidates for 228 149 new edges. Testing each one in the
//! global 267 112-entry `HashSet` was memory-latency-bound, cost 6–10 s on the
//! coder host (the classify pre-check's budget is 12 s, so about 1 run in 10
//! was cut) and doubled with an unrelated codegen change (9f74f15b). A timing
//! guard can only see that on the host and build where it happens.
//! `chain_edge_set_probes` counts every lookup the fixpoint makes in its global
//! edge set during Rule 4 (the set's fields are private, so no lookup escapes
//! the count). It is host-independent; it can vary slightly with `HashSet`
//! iteration order, so the guard bounds it rather than pinning it.
//!
//! Not covered here: what one lookup costs. A slower hasher or a codegen change
//! is invisible to these counters; `shipped_default_budget_covers_family_precheck`
//! (release only) is the timing guard.
//!
//! The equivalence of the modes (same fixpoint, same counters) is the
//! differential property test `local_and_global_chain_dedupe_agree` in
//! `src/abox_saturation.rs`, with per-polarity canaries beside it.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_reasoner::abox_saturation::{SaturationResult, saturate_abox_consistency};

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn internal(src: &str) -> InternalOntology {
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::Cursor::new(src),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    owl_dl_core::convert::convert_ontology(&onto).expect("convert")
}

/// Layers of individuals, every node of one layer `p`-linked to every node of
/// the next; `p ⊑ t`, `t` transitive, and the chain `t ∘ p⁻ ⊑ q`. Dense, so
/// the transitive join reaches most conclusions along many paths: many more
/// candidates than new edges, which is the shape that made `family` slow.
fn layered(layers: usize, width: usize) -> String {
    let mut s = String::from(
        "Prefix(:=<http://example.org/layers#>)\n\
         Ontology(<http://example.org/layers>\n\
         Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:t))\n\
         Declaration(ObjectProperty(:q))\n\
         SubObjectPropertyOf(:p :t) TransitiveObjectProperty(:t)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:t ObjectInverseOf(:p)) :q)\n",
    );
    for l in 0..layers {
        for w in 0..width {
            let _ = writeln!(s, "Declaration(NamedIndividual(:n{l}_{w}))");
        }
    }
    for l in 0..layers - 1 {
        for a in 0..width {
            for b in 0..width {
                let _ = writeln!(s, "ObjectPropertyAssertion(:p :n{l}_{a} :n{}_{b})", l + 1);
            }
        }
    }
    s.push(')');
    s
}

fn fires(r: &SaturationResult) -> u64 {
    r.chain2_fires + r.chain3_fires
}

/// The guard. Sabotage: turn the cache off, or probe the global set per
/// candidate anywhere in Rule 4, and the lookups climb to the attempts.
#[test]
fn rule4_looks_up_few_candidates_in_the_global_edge_set() {
    let _l = lock();
    let r = saturate_abox_consistency(&internal(&layered(6, 10)));
    assert!(!r.clash);
    assert!(fires(&r) > 0, "the fixture exercises Rule 4");
    assert!(
        r.chain_edge_set_probes >= fires(&r),
        "every new conclusion is inserted into the global set"
    );
    assert!(
        r.chain_join_attempts >= 10 * r.chain_edge_set_probes,
        "Rule 4 looked up {} of its {} candidates in the global edge set ({} new): \
         the per-anchor cache (#447) is bypassed",
        r.chain_edge_set_probes,
        r.chain_join_attempts,
        fires(&r)
    );
}

/// `RUSTDL_ABOX_CHAIN_GLOBAL_DEDUPE=1` reaches the public entry point: every
/// candidate is then a global lookup. Same fixpoint either way.
#[test]
fn global_dedupe_switch_reaches_the_public_entry() {
    let _l = lock();
    let onto = internal(&layered(6, 10));
    let local = saturate_abox_consistency(&onto);
    // SAFETY: set_var is unsafe under edition 2024; every env access in this
    // binary holds `lock()`.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_ABOX_CHAIN_GLOBAL_DEDUPE", "1");
    }
    let global = saturate_abox_consistency(&onto);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_ABOX_CHAIN_GLOBAL_DEDUPE");
    }
    assert_eq!(global.chain_edge_set_probes, global.chain_join_attempts);
    assert!(10 * local.chain_edge_set_probes <= global.chain_edge_set_probes);
    assert_eq!(local.chain_join_attempts, global.chain_join_attempts);
    assert_eq!(fires(&local), fires(&global));
    let sorted = |r: &SaturationResult| {
        let mut e = r.edges.clone();
        e.sort_unstable();
        e
    };
    assert_eq!(sorted(&local), sorted(&global));
}

/// The same bound on `family.ofn`, the ontology #447 was measured on (measured:
/// 315 257 lookups for 101 466 357 candidates and 228 149 new edges).
/// `#[ignore]`d because the corpus file is gitignored, and slow in debug.
#[test]
#[ignore = "needs ontologies/real/family.ofn (gitignored); slow in the debug profile"]
fn family_rule4_looks_up_few_candidates() {
    let _l = lock();
    let src = std::fs::read_to_string("../../ontologies/real/family.ofn")
        .expect("ontologies/real/family.ofn (run ./scripts/fetch-real-ontologies.sh)");
    let r = saturate_abox_consistency(&internal(&src));
    assert!(r.clash, "family.ofn is inconsistent");
    assert!(r.chain_edge_set_probes >= fires(&r));
    assert!(r.chain_join_attempts >= 100 * r.chain_edge_set_probes);
}

//! Canaries for the `emitted`-before-`droppable` ordering defect in
//! `seed_disjoint_bucket::try_emit` (`RUSTDL_DKEY_EMIT_ORDER`, default OFF).
//!
//! THE BUG. A `DKey` pair can belong to SEVERAL role components, and the
//! collapse/broadcast split is a PER-COMPONENT judgement — the same pair can be
//! unusable in one component (both keys value-only, no collapse role) and genuinely
//! consumable in another (one key BROADCAST there by a `∀`). `try_emit` claimed the
//! pair in its `emitted` dedup set BEFORE consulting the split, so whichever component
//! the `BTreeMap` reached first spent the pair permanently; if that component declined
//! it, the component that could have used it was never asked and the entailed
//! `DisjointClasses(DKey, DKey)` was silently never emitted.
//!
//! The symptom is NON-MONOTONIC, and that is what [`p_alone_is_unsat_at_every_setting`]
//! plus [`unrelated_second_property_must_not_lose_the_clash`] pin as a pair: `∀p.[0,5] ⊓
//! ∃p.{9}` is `⊥` on its own, and stays `⊥` only with the lever on once an UNRELATED
//! data property `q` merely mentions the same two keys in value position.
//!
//! DIRECTION OF RISK — INVERTED versus the rest of the `DKey` area. The lever makes
//! conversion emit MORE disjointness, so its failure mode is a FALSE POSITIVE, not a
//! miss. [`overlapping_ranges_stay_satisfiable`] and
//! [`cross_datatype_stays_a_deliberate_miss`] are the FP guards;
//! [`the_recovered_pair_is_emitted_exactly_once`] is the negative control for the
//! dedup itself (moving the `insert` must not let one pair be emitted twice).
//!
//! NOTE ON EVIDENCE. The curated corpus is INERT for this area —
//! `datatype_value_membership.rs` says so itself ("the corpus has NO such clash, so
//! these canaries are the ENTIRE safety net"). The FP=0 net shows non-regression only;
//! these canaries and the `ore_ont_5368` discriminator are the actual evidence.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_emit_order`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ir::ConceptExpr;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Mirrors dkey_post_nnf.rs.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const PFX: &str = "Prefix(:=<http://ex.org/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";
const DECLS: &str = "Declaration(Class(:Direct))
Declaration(Class(:Other))
Declaration(DataProperty(:p))
Declaration(DataProperty(:q))
";

/// `xsd:integer[0, 5]`.
const R05: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer \
                   xsd:maxInclusive \"5\"^^xsd:integer)";
/// `xsd:integer[0, 20]` — a SUPERSET of both `[0,5]` and the point `9`, so it is
/// disjoint from neither and contributes no clash of its own. Its only job is to make
/// `q` merge-inducing-but-not-collapse (a `∀role.DKey` is `m_star` yet, with a
/// provably pure-`DKey` filler, not a COLLAPSE source) so `q`'s component survives the
/// merging gate and reaches `try_emit`.
const R020: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer \
                    xsd:maxInclusive \"20\"^^xsd:integer)";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://ex.org/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn unsat_of(body: &str) -> Vec<String> {
    let c = owl_dl_reasoner::classify(&parse(body)).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

fn unsat_on(body: &str) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "1");
    unsat_of(body)
}

fn unsat_off(body: &str) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "0");
    unsat_of(body)
}

/// `Direct ⊑ ∀p.[0,5] ⊓ ∃p.{9}` — unsatisfiable on its own, at every flag setting.
fn p_clash() -> String {
    format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"9\"^^xsd:integer))"
    )
}

/// A SECOND role component that merely mentions the same two keys in VALUE position.
/// It entails nothing about `Direct` and clashes with nothing itself.
fn q_decoy() -> String {
    format!(
        "SubClassOf(:Other DataAllValuesFrom(:q {R020}))
SubClassOf(:Other DataSomeValuesFrom(:q {R05}))
SubClassOf(:Other DataHasValue(:q \"9\"^^xsd:integer))"
    )
}

fn direct() -> Vec<String> {
    vec!["http://ex.org/Direct".to_string()]
}

// ----------------------------------------------------- the monotonicity differential

/// CONTROL — the clash alone. One role component, so the ordering defect cannot bite:
/// `Direct ≡ ⊥` at BOTH flag settings. This is the baseline the next test loses.
#[test]
fn p_alone_is_unsat_at_every_setting() {
    let body = p_clash();
    assert_eq!(unsat_off(&body), direct(), "the calculus handles this");
    assert_eq!(
        unsat_on(&body),
        direct(),
        "and the lever does not disturb it"
    );
}

/// THE REPRODUCER. Adding `q` — a different data property, no hierarchy edge to `p`,
/// entailing nothing about `Direct` — must not make `Direct` satisfiable again.
///
/// The single disjoint pair `[0,5] ⟂ 9` sits in TWO components: `p`'s, where `[0,5]`
/// is BROADCAST by the `∀` so the split keeps it, and `q`'s, where both keys are
/// value-only so the split declines it. Flag OFF, `q`'s component is visited first and
/// spends the pair, `p`'s group short-circuits, and the axiom is never emitted.
#[test]
fn unrelated_second_property_must_not_lose_the_clash() {
    let body = format!("{}\n{}", p_clash(), q_decoy());
    assert_eq!(
        unsat_on(&body),
        direct(),
        "∀p.[0,5] ⊓ ∃p.{{9}} is empty however many unrelated properties exist"
    );
    assert!(
        unsat_off(&body).is_empty(),
        "flag OFF still exhibits the defect — if this ever passes, the ordering \
         defect was fixed somewhere else and this canary no longer guards anything"
    );
}

/// The `RUSTDL_DKEY_MERGING_GATE=0` ANOMALY, resolved at the ROOT. Turning a purely
/// RESTRICTIVE gate off cannot lose entailments, yet it did: with `RUSTDL_DKEY_POST_NNF=0`
/// (which exposes the NNF-only `∀q` the 0.4.8 lever otherwise sees), the default finds
/// both classes unsat and `MERGING_GATE=0` finds NEITHER — the gate was accidentally
/// masking this ordering defect by skipping `q`'s component entirely.
#[test]
fn merging_gate_off_agrees_with_default_on_the_nnf_fixture() {
    let both = format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"9\"^^xsd:integer))
SubClassOf(:Other ObjectComplementOf(DataSomeValuesFrom(:q DataComplementOf({R05}))))
SubClassOf(:Other DataHasValue(:q \"9\"^^xsd:integer))"
    );
    let expect = vec![
        "http://ex.org/Direct".to_string(),
        "http://ex.org/Other".to_string(),
    ];

    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _post = SetEnvGuard::set("RUSTDL_DKEY_POST_NNF", "0");

    {
        let _lever = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "0");
        let _gate = SetEnvGuard::set("RUSTDL_DKEY_MERGING_GATE", "0");
        assert!(
            unsat_of(&both).is_empty(),
            "the anomaly itself: gate OFF loses BOTH classes before the fix"
        );
    }
    {
        let _lever = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "1");
        let _gate = SetEnvGuard::set("RUSTDL_DKEY_MERGING_GATE", "0");
        assert_eq!(unsat_of(&both), expect, "gate OFF, lever ON");
    }
    {
        let _lever = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "1");
        assert_eq!(
            unsat_of(&both),
            expect,
            "gate default-ON, lever ON — agrees"
        );
    }
}

// ------------------------------------------------------------ negative controls (FP)

/// NEGATIVE CONTROL FOR THE DEDUP. Moving `emitted.insert` past the droppable test is
/// only correct if the pair can still be emitted at most ONCE.
///
/// The fixture is built so the dedup is LOAD-BEARING: `[0,5] ⟂ 9` reaches THREE role
/// components — `q`'s, which declines it (both keys value-only), and `p`'s AND `p2`'s,
/// which both KEEP it (each broadcasts `[0,5]` via its own `∀`). The `p`/`q` fixture
/// alone would not do: with only one keeping component the guard is unobservable, and
/// deleting it would leave the test green.
///
/// Counted at the CONVERSION level, not through classify: a duplicate `DisjointClasses`
/// is logically idempotent and would be invisible in the verdict.
#[test]
fn the_recovered_pair_is_emitted_exactly_once() {
    let body = format!(
        "{}\n{}\nDeclaration(DataProperty(:p2))
SubClassOf(:Other2 DataAllValuesFrom(:p2 {R05}))
SubClassOf(:Other2 DataHasValue(:p2 \"9\"^^xsd:integer))
Declaration(Class(:Other2))",
        p_clash(),
        q_decoy()
    );
    let onto = parse(&body);

    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _lever = SetEnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "1");
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");

    let mut pairs: Vec<(String, String)> = Vec::new();
    for ax in &internal.axioms {
        let owl_dl_core::ontology::Axiom::DisjointClasses(cs) = ax else {
            continue;
        };
        let iris: Vec<&str> = cs
            .iter()
            .filter_map(|&c| match internal.concepts.get(c) {
                ConceptExpr::Atomic(cid) => Some(internal.vocabulary.class_iri(*cid)),
                _ => None,
            })
            .filter(|iri| owl_dl_core::convert::is_dkey_iri(iri))
            .collect();
        if iris.len() == cs.len() && cs.len() == 2 {
            let (mut a, mut b) = (iris[0].to_string(), iris[1].to_string());
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            pairs.push((a, b));
        }
    }
    assert_eq!(
        pairs.len(),
        1,
        "exactly one DKey-disjointness axiom, emitted once: {pairs:?}"
    );
    let mut deduped = pairs.clone();
    deduped.dedup();
    assert_eq!(deduped.len(), pairs.len(), "no duplicate pair: {pairs:?}");
}

/// NEGATIVE — FP GUARD. The lever must never emit disjointness for ranges that OVERLAP.
/// `∀p.[0,5]` with `p = 3` is satisfiable; the same two-component shape is present, so
/// if the lever ever bypassed the per-pair `disjoint()` test this would go unsat — a
/// FALSE POSITIVE, the one direction FP=0 forbids.
#[test]
fn overlapping_ranges_stay_satisfiable() {
    let body = format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"3\"^^xsd:integer))
{}",
        q_decoy()
    );
    assert!(unsat_on(&body).is_empty(), "3 ∈ [0,5] — no clash");
    assert_eq!(unsat_on(&body), unsat_off(&body));
}

/// NEGATIVE — the cross-datatype FP hazard, pinned as a DELIBERATE MISS.
/// `∀p.xsd:integer[0,5]` with `p = "9"^^xsd:string` IS unsatisfiable, but rustdl
/// declines it by design: `seed_disjoint_bucket` is invoked once PER datatype bucket
/// with only that bucket's keys, so an int↔string pair is structurally unconstructible.
/// The lever changes WHEN a pair is claimed, never WHICH keys are in a bucket — this
/// must stay a MISS rather than become an (here accidentally correct) unsat.
#[test]
fn cross_datatype_stays_a_deliberate_miss() {
    let body = format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"9\"^^xsd:string))
{}",
        q_decoy()
    );
    assert!(
        unsat_on(&body).is_empty(),
        "DKey buckets are strictly disjoint — this clash is a documented MISS"
    );
    assert_eq!(unsat_on(&body), unsat_off(&body));
}

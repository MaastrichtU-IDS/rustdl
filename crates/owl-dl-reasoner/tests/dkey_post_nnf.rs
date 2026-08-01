//! Canaries for NNF-only `∀role.DKey` restrictions in the bounded `DKey`-disjointness
//! gates (`RUSTDL_DKEY_POST_NNF`, default OFF).
//!
//! THE BUG (D10 class). `dkey_components` runs inside `convert_ontology`, i.e. BEFORE
//! NNF, and classifies roles by matching syntactic `All` / `Max` pool entries. A
//! universal restriction that exists only AFTER NNF is invisible. The reachable
//! OWL 2 DL shape is `ObjectComplementOf(DataSomeValuesFrom(q, DataComplementOf(r)))`,
//! which NNF turns into `∀q.DKey(r)`: pre-NNF the pool holds `Not(Some(q, Not(DKey)))`,
//! so `q` is marked neither merge-inducing nor collapse/broadcast, its component is
//! gated out, and the disjointness pair is dropped — after which the post-NNF `∀`
//! has nothing to clash against. rustdl reports the class satisfiable under EVERY
//! engine flag while the banner certifies `Horn (hyper Horn fixpoint is complete)`.
//! Konclude v0.7.0 and `HermiT` 1.4.3 both report it `≡ owl:Nothing` (run 2026-08-01).
//!
//! It is a completeness REGRESSION versus pre-v0.3.29: `RUSTDL_BOUNDED_DKEY_DISJOINT=0`
//! still catches it, and either `DKey` gate alone is enough to lose it.
//!
//! NEGATIVES FIRST — but read the sabotage result before trusting them. Three of the
//! seven tests are cases that must stay SATISFIABLE (an in-range value, an unrelated
//! property) or stay a MISS (a cross-datatype clash rustdl declines by design, since
//! `DKey` buckets must never cross-subsume). **Six sabotages were run; only three
//! negatives' worth of protection is real, and the reason is structural:**
//!
//! - `in_range_value_stays_satisfiable` IS a genuine FP guard — deleting the
//!   per-pair `disjoint()` value-space test makes it fail.
//! - `out_of_range_on_unrelated_property_stays_satisfiable` is a NON-REGRESSION /
//!   volume control, **not** an FP guard, and saying otherwise would be false. Two
//!   deliberate over-approximations (anchoring every dual under every role; then
//!   also marking every role merge-inducing) both left it GREEN. That is correct
//!   rather than a weak test: every axiom this pass can cause to be emitted is
//!   guarded by `disjoint()` and is therefore semantically TRUE, so no amount of
//!   over-anchoring can produce a false positive — it can only produce dead weight.
//!   The role-component machinery is a cost bound, not a soundness gate.
//! - `cross_datatype_stays_a_deliberate_miss` is likewise inert with respect to this
//!   change: `seed_disjoint_bucket` is invoked once PER datatype bucket with only
//!   that bucket's keys, so a cross-bucket pair is structurally unconstructible no
//!   matter what the components say. Cross-bucket seeding is guarded elsewhere, by
//!   `parser_matrix_mutual_exclusivity` in `data_axioms.rs`.
//!
//! NOTE ON EVIDENCE. The curated corpus is INERT for this area —
//! `datatype_value_membership.rs` says so itself ("the corpus has NO such clash, so
//! these canaries are the ENTIRE safety net"). The FP=0 net shows non-regression;
//! these canaries plus the Konclude ∪ `HermiT` adjudication are what carry correctness.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_post_nnf`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Mirrors classify_defined_sweep.rs.
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
const DECLS: &str = "Declaration(Class(:Negated))
Declaration(Class(:Direct))
Declaration(Class(:A))
Declaration(DataProperty(:p))
Declaration(DataProperty(:q))
Declaration(DataProperty(:z))
";
/// `xsd:integer[0, 5]`.
const R05: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer \
                   xsd:maxInclusive \"5\"^^xsd:integer)";

fn unsat_of(body: &str) -> Vec<String> {
    let src = format!("{PFX}Ontology(<http://ex.org/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
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
    let _flag = SetEnvGuard::set("RUSTDL_DKEY_POST_NNF", "1");
    unsat_of(body)
}

fn unsat_off(body: &str) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_DKEY_POST_NNF", "0");
    unsat_of(body)
}

/// `Negated ⊑ ¬∃q.¬[0,5]`, i.e. `∀q.[0,5]` after NNF.
fn nnf_forall_q() -> String {
    format!(
        "SubClassOf(:Negated ObjectComplementOf(DataSomeValuesFrom(:q DataComplementOf({R05}))))"
    )
}

fn negated() -> Vec<String> {
    vec!["http://ex.org/Negated".to_string()]
}

// ---------------------------------------------------------------- negatives

/// NEGATIVE — in-range value. `∀q.[0,5]` with `q = 3` is SATISFIABLE. Konclude and
/// `HermiT` agree (`Negated ⊑ owl:Thing`). If the wider role classification ever made
/// this unsat it would mean a `DisjointClasses(DKey, DKey)` was emitted for a pair
/// that overlaps — a FALSE POSITIVE, the one direction FP=0 forbids.
#[test]
fn in_range_value_stays_satisfiable() {
    let body = format!(
        "{}\nSubClassOf(:Negated DataHasValue(:q \"3\"^^xsd:integer))",
        nnf_forall_q()
    );
    assert!(unsat_on(&body).is_empty(), "3 ∈ [0,5] — no clash");
    assert_eq!(unsat_on(&body), unsat_off(&body));
}

/// NEGATIVE — unrelated property. The `∀` is on `q`, the out-of-range value on `z`,
/// and there is no hierarchy edge between them, so the two `DKeys` can never be
/// co-labelled. Satisfiable per Konclude and `HermiT`. Guards against the new
/// merge-inducing bits over-unioning role components.
#[test]
fn out_of_range_on_unrelated_property_stays_satisfiable() {
    let body = format!(
        "{}\nSubClassOf(:Negated DataHasValue(:z \"9\"^^xsd:integer))",
        nnf_forall_q()
    );
    assert!(unsat_on(&body).is_empty(), "the ∀ is on q, the value on z");
    assert_eq!(unsat_on(&body), unsat_off(&body));
}

/// NEGATIVE — the cross-datatype FP hazard, pinned as a DELIBERATE MISS.
/// `∀q.xsd:integer[0,5]` with `q = "9"^^xsd:string` IS unsatisfiable (Konclude and
/// `HermiT` both say so), but rustdl declines it by design: `seed_dkey_subsumptions`
/// buckets by datatype and NEVER seeds an edge across buckets, because an int↔float
/// or int↔string cross-subsumption would be a false positive. This test pins that
/// the wider role classification does not leak a cross-bucket pair — it must stay a
/// MISS, not become a (here accidentally correct) unsat.
#[test]
fn cross_datatype_stays_a_deliberate_miss() {
    let body = format!(
        "{}\nSubClassOf(:Negated DataHasValue(:q \"9\"^^xsd:string))",
        nnf_forall_q()
    );
    assert!(
        unsat_on(&body).is_empty(),
        "DKey buckets are strictly disjoint — this clash is a documented MISS"
    );
    assert_eq!(unsat_on(&body), unsat_off(&body));
}

// ---------------------------------------------------------------- positives

/// THE REPRODUCER (review R1 fixture `only_neg`). Konclude and `HermiT`: `Negated ≡ ⊥`.
#[test]
fn nnf_only_forall_derives_unsat() {
    let body = format!(
        "{}\nSubClassOf(:Negated DataHasValue(:q \"9\"^^xsd:integer))",
        nnf_forall_q()
    );
    assert_eq!(unsat_on(&body), negated(), "∀q.[0,5] ⊓ ∃q.{{9}} is empty");
    assert!(unsat_off(&body).is_empty(), "flag OFF keeps the old miss");
}

/// The `∃` sits under `¬(A ⊔ …)`, so it is reached only by recursing through the
/// negative-polarity `Or`. Guards the walk's De Morgan arms — a `Not(Some(..))`-only
/// pattern match would miss this. Konclude and `HermiT`: `Negated ≡ ⊥`.
#[test]
fn nnf_forall_under_negated_union_derives_unsat() {
    let body = format!(
        "SubClassOf(:Negated ObjectComplementOf(ObjectUnionOf(:A \
         DataSomeValuesFrom(:q DataComplementOf({R05})))))
SubClassOf(:Negated DataHasValue(:q \"9\"^^xsd:integer))"
    );
    assert_eq!(unsat_on(&body), negated());
    assert!(unsat_off(&body).is_empty());
}

/// SPELLING DIFFERENTIAL — the direct gate for the bug. The NNF-only spelling and
/// the directly-written `∀p.[0,5] ⊓ ∃p.{9}` (which is caught at EVERY flag setting,
/// proving the calculus can do it) must classify the same way once the lever is on.
#[test]
fn nnf_spelling_matches_direct_spelling() {
    let direct = format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"9\"^^xsd:integer))"
    );
    let nnf = format!(
        "{}\nSubClassOf(:Negated DataHasValue(:q \"9\"^^xsd:integer))",
        nnf_forall_q()
    );
    assert_eq!(
        unsat_off(&direct),
        vec!["http://ex.org/Direct".to_string()],
        "the direct spelling is caught even flag-OFF — the calculus is fine"
    );
    assert_eq!(
        unsat_on(&nnf).len(),
        unsat_on(&direct).len(),
        "the two spellings of ∀p.[0,5] ⊓ ∃p.{{9}} must agree"
    );
}

/// The `DKEY_MERGING_GATE=0` ANOMALY, resolved. Both spellings in ONE file: flag-OFF
/// the default finds both unsat, but `RUSTDL_DKEY_MERGING_GATE=0` finds NEITHER —
/// adding sound entailed disjointness LOSES entailments.
///
/// Root cause (measured with `RUSTDL_DKEY_SPLIT_STATS=1`, `dkey_pairs_total=1`,
/// `would_drop=1`): the single `DKey([0,5]) ⟂ DKey(9)` pair belongs to two role
/// components — `p`'s, where it is NOT droppable because `[0,5]` is broadcast there,
/// and `q`'s, where pre-NNF it looks value-only. `seed_disjoint_bucket::try_emit`
/// runs `emitted.insert(pair)` BEFORE the droppable test, so whichever component the
/// `BTreeMap` visits first consumes the pair permanently and `p`'s group short-
/// circuits. The merging gate accidentally hid this by skipping `q`'s component.
///
/// With this lever on, `q` carries the NNF `∀`, so `[0,5]` is broadcast in `q`'s
/// component too and the pair is droppable in NEITHER — `would_drop` goes 1 → 0 and
/// both settings agree. The `emitted`-before-`droppable` ordering defect ITSELF is a
/// separate latent bug and is NOT fixed here; this test pins that the lever must not
/// leave the anomaly in place.
#[test]
fn merging_gate_anomaly_resolved() {
    let both = format!(
        "SubClassOf(:Direct DataAllValuesFrom(:p {R05}))
SubClassOf(:Direct DataHasValue(:p \"9\"^^xsd:integer))
{}
SubClassOf(:Negated DataHasValue(:q \"9\"^^xsd:integer))",
        nnf_forall_q()
    );
    let expect = vec![
        "http://ex.org/Direct".to_string(),
        "http://ex.org/Negated".to_string(),
    ];
    assert_eq!(unsat_on(&both), expect, "default settings, lever ON");

    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _post = SetEnvGuard::set("RUSTDL_DKEY_POST_NNF", "1");
    let _gate = SetEnvGuard::set("RUSTDL_DKEY_MERGING_GATE", "0");
    assert_eq!(
        unsat_of(&both),
        expect,
        "with the lever on, MERGING_GATE=0 must no longer lose BOTH classes"
    );
}

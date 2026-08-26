//! Integration tests for `inferred_object_property_values` /
//! `inferred_data_property_values` (issue #45, Task 4.1).
//!
//! `inferred_object_property_values` = the sound `materialize_object_property_assertions`
//! seed, plus a budgeted/bounded entailment extension over the seed's own
//! individual-pair neighborhood (never the full `|I|²×|R|` cross-product). The
//! bounded-extension oracle (candidate pairs beyond the seed neighborhood) is
//! Task 4.4's concern; here we confirm the seed surfaces correctly and the
//! public API shape is right.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{Component, Individual, ObjectPropertyExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{inferred_data_property_values, inferred_object_property_values};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::io::Cursor;
use std::path::Path;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

/// Symmetric(:r); r(a,b) ⇒ r(b,a) entailed. Both directions are already in the
/// `materialize_object_property_assertions` seed (the `ABox` saturator closes
/// symmetric roles), so this exercises the seed-surfacing path, not the
/// bounded entailment extension.
#[test]
fn object_values_include_asserted_and_symmetric() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(ObjectProperty(:r)) SymmetricObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b))",
    );
    let v = inferred_object_property_values(&o, None).unwrap();
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#r", "http://ex/#a"));
}

/// A plain `DataPropertyAssertion` must surface as its 4-tuple (subject,
/// property, lexical, datatype) — `inferred_data_property_values` is a
/// structural passthrough over `materialize_data_property_assertions` with the
/// `lang` element dropped.
#[test]
fn data_values_include_asserted() {
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(DataProperty(:dp))
            DataPropertyAssertion(:dp :a "42"^^xsd:integer))"#,
    );
    let v = inferred_data_property_values(&o).unwrap();
    assert!(v.quints().iter().any(|(s, p, lex, dt, lang)| {
        s == "http://ex/#a"
            && p == "http://ex/#dp"
            && lex == "42"
            && dt == "http://www.w3.org/2001/XMLSchema#integer"
            && lang.is_empty()
    }));
    assert!(!v.incomplete());
}

/// `r(a,b)`, `r(b,c)`, `Transitive(r)` ⇒ `r(a,c)` entailed — already closed by
/// the `materialize_object_property_assertions` seed itself (transitive
/// closure is part of that closure), so all three triples are present without
/// the extension needing to add anything new. `r` is non-symmetric, so the
/// candidate-pair neighborhood also probes the reverse orientations
/// (`r(b,a)`, `r(c,b)`, `r(c,a)`) — none are entailed (`Some(true)`), but
/// running those probes at all still marks the result `incomplete()` per the
/// documented honesty policy (any extension probe run ⇒ `incomplete`, even
/// when it adds nothing).
#[test]
fn object_values_transitive_seed_and_honest_incomplete_flag() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            TransitiveObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :c))",
    );
    let v = inferred_object_property_values(&o, None).unwrap();
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#r", "http://ex/#c"));
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#c"));
    // Never entailed — the reverse orientations must NOT appear (soundness).
    assert!(!has("http://ex/#b", "http://ex/#r", "http://ex/#a"));
    assert!(!has("http://ex/#c", "http://ex/#r", "http://ex/#b"));
    assert!(!has("http://ex/#c", "http://ex/#r", "http://ex/#a"));
    assert!(v.incomplete());
}

/// Regression test for PR #50 review Fix 2 (Important): a MISS reported as
/// complete. The axioms `SubClassOf(:C, ObjectUnionOf(ObjectHasValue(:R :b), ObjectHasValue(:R :c)))`,
/// `ClassAssertion(:C :a)`, and `NegativeObjectPropertyAssertion(:R :a :c)` together
/// entail `R(a,b)` (the only remaining disjunct once `¬R(a,c)` rules the
/// other out) — but that edge lives strictly between individuals the
/// Horn-only `materialize_object_property_assertions` seed never connects
/// (the seed does not reason over class-level disjunction at all), so the
/// seed is empty, `candidate_extension_pairs` has nothing to probe, the
/// extension loop never runs, and pre-fix `incomplete` stayed at its `false`
/// initial value — a genuine MISS silently reported as complete.
///
/// Pre-fix: `v.incomplete()` is `false` (BUG). Post-fix: the disjunctive
/// (off-`Horn`/`PureEl`-fragment) `TBox` initializes `incomplete = true`.
///
/// FP=0 is the hard, unconditional half: regardless of `incomplete`, this
/// function's bounded extension has no candidate pair to consult here (empty
/// seed ⟹ empty neighborhood), so it structurally CANNOT emit either
/// disjunct as a spurious triple — asserted here as the non-negotiable
/// soundness guard.
#[test]
fn disjunctive_has_value_reports_incomplete_and_no_fp() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:C)) Declaration(ObjectProperty(:R))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c))
            SubClassOf(:C ObjectUnionOf(ObjectHasValue(:R :b) ObjectHasValue(:R :c)))
            ClassAssertion(:C :a)
            NegativeObjectPropertyAssertion(:R :a :c))",
    );
    let v = inferred_object_property_values(&o, None).expect("KB is consistent");
    assert!(
        v.incomplete(),
        "off-fragment (disjunctive) TBox must be honestly reported incomplete"
    );
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    // FP=0 (hard, unconditional): neither disjunct may be spuriously emitted.
    assert!(
        !has("http://ex/#a", "http://ex/#R", "http://ex/#b"),
        "must not spuriously emit the entailed-but-underivable triple"
    );
    assert!(
        !has("http://ex/#a", "http://ex/#R", "http://ex/#c"),
        "must not emit the ruled-out disjunct"
    );
}

/// Regression test for PR #50 review Fix 3 (Important, test-hole): every
/// OTHER fixture in this file is TBox-free, so the bounded extension probe
/// (`consistent_with_extra`'s `extra_neg_prop` path) only ever sets
/// `incomplete` and never legitimately ADDS a non-seed triple — a
/// subject/object-swap or role-orientation bug in that probe could pass the
/// whole suite. This fixture forces a genuine non-seed `R(a,b)` entailment
/// via a real `TBox` axiom and asserts the extension positively fires.
///
/// Shape: `:seed` establishes `(a,b)` as a candidate pair (the bounded
/// extension only probes pairs that already co-occur in a seed edge — see
/// `candidate_extension_pairs`), while the actually-tested edge is
/// `R(a,b)`, entailed only via a disjunctive `ObjectHasValue` case-split
/// (`C ≡ ∃R.{b} ⊔ ∃R.{d}`, `¬R(a,d)` rules out the `d` disjunct, forcing
/// `R(a,b)`) — VERIFIED non-seed: `abox_saturation`'s `collect_hasvalues`
/// only descends into `Some`/`And` bodies, not `Or`
/// (`crates/owl-dl-reasoner/src/abox_saturation.rs`), so a UNION of
/// `ObjectHasValue`s is never captured by the Horn-only seed — this needs
/// the tableau's disjunctive case-split (`consistent_with_extra` proving
/// `KB ∪ {¬R(a,b)}` inconsistent), exactly the `extra_neg_prop` path this
/// test exercises to a POSITIVE result.
#[test]
fn extension_positively_fires_non_seed_triple() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:C))
            Declaration(ObjectProperty(:seed)) Declaration(ObjectProperty(:R))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:d))
            ObjectPropertyAssertion(:seed :a :b)
            SubClassOf(:C ObjectUnionOf(ObjectHasValue(:R :b) ObjectHasValue(:R :d)))
            ClassAssertion(:C :a)
            NegativeObjectPropertyAssertion(:R :a :d))",
    );
    let v = inferred_object_property_values(&o, None).expect("KB is consistent");
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    // Sanity: the seed edge that established the (a,b) candidate pair.
    assert!(has("http://ex/#a", "http://ex/#seed", "http://ex/#b"));
    // The point of this test: the extension genuinely FIRED and added a
    // non-seed triple, not just flagged `incomplete`.
    assert!(
        has("http://ex/#a", "http://ex/#R", "http://ex/#b"),
        "the extension must positively add the entailed non-seed triple R(a,b); got {:?}",
        v.triples()
    );
    // FP direction: the ruled-out disjunct, and the reverse orientation
    // (never entailed — `:R`/`:seed` are not symmetric/inverse), must NOT
    // appear.
    assert!(!has("http://ex/#a", "http://ex/#R", "http://ex/#d"));
    assert!(!has("http://ex/#b", "http://ex/#R", "http://ex/#a"));
    assert!(!has("http://ex/#b", "http://ex/#seed", "http://ex/#a"));
}

/// Regression test for PR #50 review Fix 2 ("proper" pass): the
/// conjunctive-antecedent counterexample that the review found the FIRST Fix
/// 2 (`analyze_fragment(PureEl|Horn)` gate) still under-reports on. The
/// axioms `SubClassOf(ObjectIntersectionOf(:A :B) :C)`,
/// `SubClassOf(:C ObjectHasValue(:R :c))`, `ClassAssertion(:A :a)`,
/// `ClassAssertion(:B :a)` are all Horn-clausal (no disjunction anywhere), so
/// `analyze_fragment` reports `Horn` and the OLD gate started `incomplete`
/// at `false` — yet `R(a,c)` IS entailed (`a:A ⊓ a:B ⟹ a:C ⟹ R(a,c)`) and the
/// `ABox` saturator's `SubClassOf` indexing DROPS the entire first axiom
/// (`abox_saturation.rs`'s `atomic_class(*sub)` gate: `sub` is the
/// non-atomic `And(A,B)`), so the seed never derives `a:C` and never fires
/// `R(a,c)`. Pre-fix: `incomplete()` is `false` (BUG). Post-fix
/// (`object_property_edge_complete`): the non-atomic `SubClassOf` antecedent
/// is outside the whitelist ⇒ `incomplete()` is `true`.
///
/// FP=0 is the hard, unconditional half: this ontology's seed never connects
/// `a` and `c` (the dropped axiom is the only thing that would), so the
/// bounded extension has no candidate pair to probe either — `R(a,c)`
/// structurally cannot be spuriously emitted here.
#[test]
fn conjunctive_antecedent_reports_incomplete_and_no_fp() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
            Declaration(ObjectProperty(:R))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:c))
            SubClassOf(ObjectIntersectionOf(:A :B) :C)
            SubClassOf(:C ObjectHasValue(:R :c))
            ClassAssertion(:A :a)
            ClassAssertion(:B :a))",
    );
    let v = inferred_object_property_values(&o, None).expect("KB is consistent");
    assert!(
        v.incomplete(),
        "conjunctive-antecedent TBox must be honestly reported incomplete \
         (the ABox saturator drops the whole non-atomic-sub SubClassOf axiom)"
    );
    assert!(
        !v.triples()
            .iter()
            .any(|(s, p, o)| s == "http://ex/#a" && p == "http://ex/#R" && o == "http://ex/#c"),
        "must not spuriously emit the entailed-but-underivable triple R(a,c)"
    );
}

/// Completeness-POSITIVE regression test for PR #50 review Fix 2 ("proper"
/// pass): pins that `object_property_edge_complete`'s gate is not vacuously
/// always-incomplete. A single object property `:R`, declared BOTH
/// `SymmetricObjectProperty` and `TransitiveObjectProperty` (both fully
/// captured by the `ABox` saturator regardless of role polarity — see
/// `object_property_edge_complete`'s doc), plus an atomic-antecedent
/// `SubClassOf(:C, ObjectHasValue(:R, :b))` (captured: `sub` is atomic,
/// `sup` is a plain `ObjectHasValue`) and `ClassAssertion(:C, :a)`. This
/// entails `R(a,b)` (has-value trigger), `R(b,a)` (symmetric), and the
/// self-loops `R(a,a)`/`R(b,b)` (transitive closure over the symmetric
/// pair) — a fully self-closed neighborhood over the SOLE declared object
/// property, so the bounded extension (which probes every declared object
/// property against every candidate pair) finds every candidate already
/// seeded and never runs a probe. Both conditions the honest gate requires
/// hold: every axiom is in the whitelist, AND the extension has nothing left
/// to probe.
#[test]
fn atomic_antecedent_hasvalue_with_role_characteristics_reports_complete() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:C)) Declaration(ObjectProperty(:R))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            SymmetricObjectProperty(:R) TransitiveObjectProperty(:R)
            SubClassOf(:C ObjectHasValue(:R :b))
            ClassAssertion(:C :a))",
    );
    let v = inferred_object_property_values(&o, None).expect("KB is consistent");
    assert!(
        !v.incomplete(),
        "a fully-whitelisted TBox/RBox with a self-closed seed neighborhood \
         must report complete, else the gate is vacuously always-incomplete; got {:?}",
        v.triples()
    );
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    assert!(has("http://ex/#a", "http://ex/#R", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#R", "http://ex/#a"));
}

/// Regression test for the Critical under-report bug fixed alongside PR #50's
/// review pass: `InverseFunctionalObjectProperty`/`FunctionalObjectProperty`
/// were whitelisted in `is_edge_complete_axiom` (`incomplete` could report
/// `false`) even though they are NOT edge-safe — they force a named-individual
/// identity that the `ABox` saturator's Rule 7 (`abox_saturation.rs`) only
/// propagates TYPES across, never edges (edge-folding, Rule 9b, fires only for
/// an EXPLICIT `SameIndividual`). So an entailed edge between two individuals
/// merged only via functionality/inverse-functionality can be missed while
/// `incomplete()` claims completeness.
///
/// Counterexample: `InverseFunctionalObjectProperty(:R)` +
/// `SymmetricObjectProperty(:R)` + `R(a,b)`, `R(a,c)`, `R(b,e)`. Symmetry gives
/// `R(b,a)`/`R(c,a)`; inverse-functionality then forces `b = c` (both are
/// `R`-predecessors of `a`); so `R(c,e)` is genuinely entailed (`c = b`,
/// `R(b,e)` asserted) — but `(c,e)` never co-occurs in a seed edge (the seed
/// only ever connects pairs that appear together in an asserted/derived edge,
/// and nothing directly relates `c` and `e`), so the bounded extension probe
/// never gets a chance to check that pair either: a genuine MISS.
///
/// Before this fix: `is_edge_complete_axiom`'s `FunctionalRole(_) |
/// InverseFunctionalRole(_) => true` arm made `object_property_edge_complete`
/// return `true` for this ontology (every other axiom shape here — role
/// assertions, `SymmetricRole` — is also whitelisted), so `incomplete()` was
/// `false` while `R(c,e)` was silently dropped: the exact under-report this
/// test pins closed. After this fix, the arm returns `false` for
/// `InverseFunctionalRole`, so `incomplete()` is honestly `true`.
///
/// FP=0 (hard, unconditional): `R(c,e)` must never be spuriously fabricated
/// either — the fix only changes the honesty flag, not the (still sound)
/// seed/extension machinery, so the missed edge stays missing rather than
/// being invented.
#[test]
fn inverse_functional_forced_identity_reports_incomplete_and_no_fp() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:R))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:e))
            InverseFunctionalObjectProperty(:R)
            SymmetricObjectProperty(:R)
            ObjectPropertyAssertion(:R :a :b)
            ObjectPropertyAssertion(:R :a :c)
            ObjectPropertyAssertion(:R :b :e))",
    );
    let v = inferred_object_property_values(&o, None).expect("KB is consistent");
    assert!(
        v.incomplete(),
        "InverseFunctionalObjectProperty forces a named-individual identity \
         the ABox saturator does not fold edges across; must be honestly \
         reported incomplete"
    );
    // FP=0: the genuinely-entailed-but-missed R(c,e) must not be spuriously
    // fabricated by the (unchanged) seed/extension machinery.
    assert!(
        !v.triples()
            .iter()
            .any(|(s, p, o)| s == "http://ex/#c" && p == "http://ex/#R" && o == "http://ex/#e"),
        "R(c,e) is entailed only via the identity-inducing axiom; the sound \
         seed/extension must not fabricate it"
    );
}

/// External completeness oracle for `inferred_object_property_values` (issue
/// #45's FP=0 soundness guard, Task 4.4). Same design as
/// `materialize_oracle.rs::oracle_edges` / `materialize_matches_hermit_oracle`:
/// the oracle is generated offline by `docker/robot/property-oracle.sh`
/// (ROBOT + embedded `HermiT`) and committed as `pv-materialized.owx`, so this
/// test needs no docker at run time.
///
/// Regenerate after changing the fixture:
///   bash docker/robot/property-oracle.sh \
///     crates/owl-dl-reasoner/tests/fixtures/property_values/pv.ofn \
///     crates/owl-dl-reasoner/tests/fixtures/property_values/pv-materialized.owx
const TOP_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
const BOTTOM_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";

type Triples = BTreeSet<(String, String, String)>;

/// `HermiT`-inferred object-property assertions between NAMED individuals from
/// the committed oracle (top/bottom filtered, matching
/// `inferred_object_property_values`'s scope).
fn oracle_object_edges(path: &Path) -> Triples {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut set = Triples::new();
    for ax in &onto {
        if let Component::ObjectPropertyAssertion(opa) = &ax.component
            && let (
                ObjectPropertyExpression::ObjectProperty(p),
                Individual::Named(s),
                Individual::Named(t),
            ) = (&opa.ope, &opa.from, &opa.to)
        {
            let prop = p.0.to_string();
            if prop == TOP_OBJECT_PROPERTY || prop == BOTTOM_OBJECT_PROPERTY {
                continue;
            }
            set.insert((s.0.to_string(), prop, t.0.to_string()));
        }
    }
    set
}

/// FP (HARD, UNCONDITIONAL): `inferred_object_property_values` must never emit
/// a triple `HermiT` does not entail — this is the issue #45 soundness
/// guarantee and this assertion must never be weakened. MISSED (entailed
/// triples the bounded extension does not surface) is only asserted empty
/// when the result reports itself complete (`!incomplete()`); otherwise it is
/// a documented, honestly-flagged sound under-approximation.
#[test]
fn object_property_values_matches_hermit_oracle() {
    let dir = Path::new("tests/fixtures/property_values");
    let file = File::open(dir.join("pv.ofn")).expect("fixture");
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse fixture");

    let result = inferred_object_property_values(&onto, None).expect("inferred object values");
    let got: Triples = result.triples().iter().cloned().collect();
    let oracle = oracle_object_edges(&dir.join("pv-materialized.owx"));

    let missed: Vec<_> = oracle.difference(&got).collect();
    let fp: Vec<_> = got.difference(&oracle).collect();

    assert!(
        fp.is_empty(),
        "FP — inferred_object_property_values returns, HermiT does not: {fp:?}"
    );
    if result.incomplete() {
        if !missed.is_empty() {
            eprintln!(
                "MISSED (sound under-approx, incomplete()=true) — HermiT infers, \
                 inferred_object_property_values omits: {missed:?}"
            );
        }
    } else {
        assert!(
            missed.is_empty(),
            "MISSED — HermiT infers, inferred_object_property_values omits: {missed:?}"
        );
    }
}

use owl_dl_verify::model::{
    FiniteModel, build_role_hierarchy, chain_range_out_of_profile, effective_ranges,
};
use owl_dl_verify::{Bounds, Interpretation, UnresolvedReason, Verdict};

mod common;
use common::load;

const CHAIN: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/t>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :B)
SubClassOf(:B :C)
)
";

#[test]
fn seeding_labels_are_sorted_and_contain_the_class_itself() {
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);

    let a = internal
        .vocabulary
        .class_id("http://ex.org/A")
        .expect("A declared");
    let e = model
        .element_of_class(a)
        .expect("A is satisfiable, so it is seeded");
    let label = model.label(e);

    assert!(
        label.windows(2).all(|w| w[0] <= w[1]),
        "labels must be sorted: {label:?}"
    );
    assert!(
        label.contains(&a),
        "subsumers_of is reflexive, so A must be in its own label"
    );
    assert!(
        model.in_concept(e, a),
        "in_concept must agree with the label"
    );
}

/// Was misnamed `derived_equivalent_classes_share_one_element` while its body asserted the
/// OPPOSITE (`assert_ne!` on two classes that are NOT equivalent) — renamed to match what it
/// actually checks. See `derived_equivalent_classes_share_one_element` below for the positive
/// case this file was missing entirely: that two classes that ARE derived-equivalent really do
/// collapse onto one `Element`, which is the load-bearing property `model.rs`'s module doc
/// states (`intern` dedups by label content, and `subsumers_of` coinciding is exactly
/// derived-equivalence).
#[test]
fn non_equivalent_classes_get_distinct_elements() {
    // B and C are NOT equivalent here, so they must be distinct elements.
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    assert_ne!(
        model.element_of_class(b),
        model.element_of_class(c),
        "B and C have different subsumer sets, so they must not be interned together"
    );
}

/// The positive case the file above was missing: two classes that are derived-equivalent
/// (NOT asserted via `EquivalentClasses` — via mutual `SubClassOf`, so `subsumers_of` has to do
/// the work) really do collapse onto the SAME `Element`. This is the property `model.rs`'s
/// module doc calls out explicitly ("Two classes share an element exactly when their subsumer
/// sets coincide … happens exactly for derived-equivalent classes") and, before this test,
/// nothing in the suite asserted it.
#[test]
fn derived_equivalent_classes_share_one_element() {
    const MUTUAL_SUBCLASS: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/eq>
Declaration(Class(:A)) Declaration(Class(:B))
SubClassOf(:A :B)
SubClassOf(:B :A)
)
";
    let internal = load(MUTUAL_SUBCLASS);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    assert_eq!(
        subs.subsumers_of(a),
        subs.subsumers_of(b),
        "A and B mutually subsume, so their subsumer sets must coincide"
    );
    let ea = model.element_of_class(a);
    let eb = model.element_of_class(b);
    assert!(ea.is_some(), "A is satisfiable, so it must be seeded");
    assert_eq!(
        ea, eb,
        "derived-equivalent classes must be interned onto the SAME element"
    );
}

#[test]
fn subsumers_of_is_reflexive_which_interning_and_the_witness_argument_both_need() {
    let internal = load(CHAIN);
    let (subs, _, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    assert!(
        subs.subsumers_of(a).contains(&a),
        "spec §3 and the label-interning argument both consume reflexivity"
    );
}

const RANGES: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/r>
Declaration(Class(:F)) Declaration(Class(:G))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubObjectPropertyOf(:p :q)
ObjectPropertyRange(:q :F)
ObjectPropertyRange(:p :G)
)
";

#[test]
fn effective_ranges_unions_over_super_roles() {
    let internal = load(RANGES);
    let hier = build_role_hierarchy(&internal);
    let er = effective_ranges(&internal, &hier);
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let q = internal.vocabulary.role_id("http://ex.org/q").expect("q");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let g = internal.vocabulary.class_id("http://ex.org/G").expect("G");

    let pr = er.get(&p).cloned().unwrap_or_default();
    assert!(
        pr.contains(&f),
        "p ⊑ q, so Range(q,F) constrains p-successors"
    );
    assert!(
        pr.contains(&g),
        "p's own range must be included (super_roles is reflexive)"
    );
    let qr = er.get(&q).cloned().unwrap_or_default();
    assert!(qr.contains(&f));
    assert!(
        !qr.contains(&g),
        "a SUB-role's range must NOT leak upward to q"
    );
}

// --- Range-augmented existential edges (flat, non-nested) --------------------
//
// This is a FLAT existential (`C ⊑ ∃u.A`, role `u` ranged over `F`), not the
// nested `∃t.∃u.A` shape the task brief's own `PROBE_B` uses. See
// `nested_existential_range_gap_probe_b` below and the task-4 report for why
// the nested shape cannot pass: the saturator only range-folds an existential's
// OUTER-most RHS body (`atomic_existential_rhs`'s `effective_ranges` parameter is
// applied to the top-level role only); a flat existential IS that top-level body,
// so its Tseitin target already carries `F` as a told subsumer and `target_label`
// takes the `Ok` fast path. This is a genuine, passing exercise of `successors`/
// `has_edge`/`edges` plus `expand`'s edge-creation path, and it preserves
// `PROBE_B`'s stated purpose: "A used as a class" and "A used as a `u`-filler"
// must be different elements, and only the filler-successor carries `F`.
const FLAT_RANGE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/flat>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:u :A))
ObjectPropertyRange(:u :F)
)
";

#[test]
fn a_as_a_class_and_a_as_a_u_successor_are_distinct_elements() {
    let internal = load(FLAT_RANGE);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(h);
    let reasons = model.expand(&internal, &subs, &facts, &eff, &Bounds::default());
    assert!(
        reasons.is_empty(),
        "the flat existential's target is already range-folded by the saturator, \
         so target_label must take the Ok fast path: {reasons:?}"
    );

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_a = model.element_of_class(a).expect("A is seeded");

    // EXISTENTIAL, not universal: a broken expansion that produces zero
    // successors would pass a forall-phrased assertion vacuously.
    let succ: Vec<_> = model
        .elements()
        .flat_map(|e| model.successors(e, u))
        .collect();
    assert!(!succ.is_empty(), "the u-edge must exist");
    let witness = succ[0];
    assert!(
        model.in_concept(witness, f),
        "the u-successor must carry Range(u,F)"
    );
    assert!(
        !model.in_concept(x_a, f),
        "A-as-a-class must NOT carry F — A ⊑ F is not entailed"
    );
    assert_ne!(witness, x_a, "the two must be different elements");
    assert!(
        model.has_edge(
            model
                .elements()
                .find(|&e| model.successors(e, u).contains(&witness))
                .expect("some element has the u-edge"),
            u,
            witness
        ),
        "has_edge must agree with successors"
    );
}

// --- The range IS folded through the And-wrapped Tseitin path (issue #81) ---
//
// `∃u.A` nested inside an `ObjectIntersectionOf` on the RHS of `∃t.(...)`
// reaches `target_label` for role `u` via
// `introduce_equivalent_existential_marker`, a two-way Tseitin marker that
// pushes its own existential fact.
//
// This test used to assert the OPPOSITE — that `A`'s target is "NOT
// range-folded by this path either", so `aug = [F]` stayed non-empty and
// `expand` had to report `LabelNotClosed`. That was a BUG being pinned, not a
// contract: `atomic_classes_with_existential_markers` lowered the inner body
// with `atomic_or_tseitin_body`, which never received `effective_ranges`.
// Issue #81 threads them through, so the witness now carries `Range(u) = F`
// and the gap is closed rather than merely reported.
//
// The report-only contract now lives on `topwitness.ofn`, whose gap is a
// documented design decision instead of a defect — see
// `verified_check_can_still_carry_nonempty_build_reasons`.
const AND_WRAPPED_NESTED_RANGE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/andwrapped>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectIntersectionOf(:B ObjectSomeValuesFrom(:u :A))))
ObjectPropertyRange(:u :F)
)
";

#[test]
#[allow(clippy::many_single_char_names)]
fn and_wrapped_nested_existential_range_is_folded() {
    let internal = load(AND_WRAPPED_NESTED_RANGE);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(h);
    let reasons = model.expand(&internal, &subs, &facts, &eff, &Bounds::default());
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { role, .. } if *role == u)),
        "issue #81 folds Range(u) into the And-wrapped nested witness, so there \
         is nothing left for expand to report here: {reasons:?}"
    );
    let w = model
        .elements()
        .flat_map(|e| model.successors(e, u))
        .next()
        .expect("the inner ∃u.A must produce a u-successor");
    assert!(
        model.in_concept(w, a),
        "the witness must satisfy the body A"
    );
    assert!(
        model.in_concept(w, f),
        "and must carry Range(u) = F — this is the #81 fold, and the assertion \
         this test used to make in reverse"
    );
}

// --- Review finding 1 (Task 5, round 1): build_model must actually CONVERGE
// on this fixture, not just report LabelNotClosed once from bare expand().
//
// `expand()`'s Err arm did not originally know how to look up an injected
// `Q` (that lookup was added only inside `materialise_exists`), so
// `build_model` on this exact fixture re-reported the same `(A, u)` gap every
// round forever: `pending` never emptied, and `inject_conjunction` kept
// re-pushing an equivalent axiom for the same already-injected `Q`. It
// failed SAFE (`BoundTripped`, never a false `Verified`), but it defeated
// convergence on a shape this file already exercises via bare `expand()`.
// Fixed by factoring the injected-`Q` lookup into `lookup_injected`, shared
// by both `expand`'s and `materialise_exists`'s `Err` arms.
#[test]
fn build_model_converges_on_and_wrapped_nested_range() {
    let internal = load(AND_WRAPPED_NESTED_RANGE);
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::BoundTripped { .. })),
        "build_model must converge once expand()'s Err arm can see the round-1 \
         injected Q, not spin forever re-reporting the same closed gap: {reasons:?}"
    );
}

// --- Bare nested existential: a MEASURED architecture gap, not a test bug ----
//
// The task brief's `PROBE_B` and the pre-supplied `label-closure-range-sub.ofn`
// fixture both use the shape `C ⊑ ∃t.∃u.A` — a nested existential with NO
// `ObjectIntersectionOf` wrapper. Measured directly (see the task-4 report for
// the full facts dump): `saturate_with_exists_facts` returns exactly ONE fact,
// `(C, t, T)`, where `T` is a Tseitin marker allocated by
// `TseitinAllocator::introduce_existential_marker` — a ONE-WAY marker (it pushes
// only an `ExistentialTrigger`, never an `ExistentialFact`) used specifically for
// "a nested existential AS the outer body" in
// `atomic_or_tseitin_body_with_extras`. `T` therefore never gets its OWN
// existential fact for role `u`, so `by_sub` in `expand` has NO entry keyed by
// `T`, so `target_label` is NEVER CALLED for role `u` at all — not `Ok`, not
// `Err`. No edge is created and no `LabelNotClosed` is reported either: `expand`
// returns `[]` on this exact input. This is neither a bug in `target_label`
// (correct as specified: it can only act on facts it is given) nor in `expand`'s
// worklist (correct as specified: it can only walk facts `saturate_with_exists_facts`
// hands it) — it is a fact the current saturator's public API does not expose for
// this construct. Recovering it would mean either widening
// `saturate_with_exists_facts`'s contract (a saturation-engine change, out of this
// crate's declared scope) or having `expand` independently recover nested
// Tseitin structure from `InternalOntology`.
//
// Task 4b built exactly that second option (`FiniteModel::expand_from_axioms`,
// exercised directly by `axiom_driven_expansion_materialises_the_nested_chain`
// above, which passes on the equivalent bare-nested shape `C ⊑ ∃t.∃u.A`). But
// THIS test calls only `expand`, by design (its purpose is to keep documenting
// the fact-driven path's own reach), and `expand` itself is untouched by 4b —
// so re-running it here still fails, unchanged. See the per-test `#[ignore]`
// reasons below for what was actually measured on this binary, including a
// probe of what happens if `expand_from_axioms` IS added to this call site.
// See `docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md` for why an
// `#[ignore]`d claim must be revisited rather than silently trusted.
const NESTED_RANGE_NO_AND: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/pb>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
ObjectPropertyRange(:u :F)
)
";

#[test]
#[ignore = "RE-MEASURED for Task 5 (2026-08-28): still FAILS, unchanged — this test \
            calls model.expand() DIRECTLY (never expand_from_axioms, never \
            build_model), and Task 5's wiring lives entirely inside build_model's \
            fixpoint loop and expand_from_axioms's materialise_exists. expand() \
            itself is untouched by Task 5, exactly as by 4b: saturate_with_exists_facts \
            still emits no fact for the inner role, so expand()'s by_sub map has no \
            entry for it and no u-edge is created — confirmed by re-running this exact \
            test body against the Task-5 binary; output unchanged from the 4b \
            measurement. Separately, even the underlying label-closure gap this probe \
            was designed to exercise is now CLOSED at the build_model level for the \
            equivalent named-filler shape (see \
            axiom_driven_expansion_materialises_the_nested_chain and \
            injection_closes_the_label_so_the_healthy_fixture_needs_no_label_not_closed), \
            via injection through expand_from_axioms — but this test cannot observe \
            that, since it never calls the axiom-driven path. Left ignored, scoped to \
            documenting expand()'s own reach."]
fn nested_existential_range_gap_probe_b() {
    let internal = load(NESTED_RANGE_NO_AND);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(h);
    let _ = model.expand(&internal, &subs, &facts, &eff, &Bounds::default());

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_a = model.element_of_class(a).expect("A is seeded");

    let succ: Vec<_> = model
        .elements()
        .flat_map(|e| model.successors(e, u))
        .collect();
    assert!(!succ.is_empty(), "the u-edge must exist");
    let witness = succ[0];
    assert!(
        model.in_concept(witness, f),
        "the u-successor must carry Range(u,F)"
    );
    assert!(
        !model.in_concept(x_a, f),
        "A-as-a-class must NOT carry F — A ⊑ F is not entailed"
    );
    assert_ne!(witness, x_a, "the two must be different elements");
}

#[test]
#[ignore = "RE-MEASURED for Task 5 (2026-08-28): still FAILS as literally written \
            (reasons == [] from model.expand() alone — confirmed unchanged against \
            the Task-5 binary), because this test checks ONLY expand()'s return \
            value, and Task 5's injection wiring lives in build_model + \
            expand_from_axioms, not in expand() itself. The gap this test was \
            written to name is now SUPERSEDED, not just reachable: \
            injection_closes_the_label_so_the_healthy_fixture_needs_no_label_not_closed \
            runs build_model on this EXACT fixture and asserts the opposite outcome \
            this test does (no LabelNotClosed survives, because expand_from_axioms's \
            materialise_exists finds the round-1-injected Q ≡ A ⊓ F, re-saturated, \
            and uses its closed row {A,F,G,Q} as the label instead of reporting). So \
            this ignored test does not describe a residual defect — it pins the \
            report-only behaviour of expand() called in isolation, which build_model \
            deliberately does not settle for. Left ignored rather than deleted, since \
            it still documents that expand() alone is report-only by design."]
fn label_closure_case_reports_label_not_closed_rather_than_a_wrong_label() {
    let ofn = std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn")
        .expect("fixture present");
    let internal = load(&ofn);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts);
    let reasons = model.expand(&internal, &subs, &facts, &eff, &Bounds::default());
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "Range(u,F)+F⊑G needs closure this local rule cannot supply; report it, \
         do not emit a truncated label. Got {reasons:?}"
    );
}

// --- Axiom-driven expansion reaches nested existential witnesses -----------
//
// Task 4b. `saturate_with_exists_facts` gives the nested existential's Tseitin
// marker an EMPTY subsumer set and emits no fact for it (see the task-4b
// brief and the two `#[ignore]`d tests above), so a fact-driven model has no
// element for the `u`-successor at all. `expand_from_axioms` derives the
// existential structure from the axioms via `ConceptPool` instead, reaching
// the hop the fact list omits.
const NESTED_MONO: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/nm>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
SubClassOf(:A :F)
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)
)
";

#[test]
#[allow(clippy::many_single_char_names)]
fn axiom_driven_expansion_materialises_the_nested_chain() {
    let internal = load(NESTED_MONO);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let hier = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &hier);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(hier);
    let bounds = Bounds::default();
    let _ = model.expand(&internal, &subs, &facts, &eff, &bounds);
    let _ = model.expand_from_axioms(&internal, &subs, &eff, &bounds);

    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let t = internal.vocabulary.role_id("http://ex.org/t").expect("t");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_c = model.element_of_class(c).expect("C is satisfiable");

    // EXISTENTIAL at each hop: a zero-successor model passes a forall phrasing vacuously.
    let mid = model.successors(x_c, t);
    assert!(
        !mid.is_empty(),
        "C must gain a t-successor from its own axiom"
    );
    let leaf: Vec<_> = mid.iter().flat_map(|m| model.successors(*m, u)).collect();
    assert!(
        !leaf.is_empty(),
        "the NESTED u-successor is what the fact list omits"
    );
    let w = leaf[0];
    assert!(
        model.in_concept(w, a),
        "the leaf must satisfy the body class A"
    );
    assert!(
        model.in_concept(w, f),
        "and A ⊑ F must be closed INTO the leaf label — this is what makes the #80 shape detectable"
    );
}

// --- Base closure survives, AND the range augmentation is now closed --------
//
// Fix-round-1 finding (still pinned below): `materialise_exists`'s `Some` arm
// looked up each required atom individually via `target_label`, and on `Err`
// dropped the atom's OWN base closure too, not just the disputed range
// classes — the leaf witness's label came back completely EMPTY, so
// `in_concept(leaf, A)` was `false` even though the axiom trivially entails
// the witness is an `A`.
//
// The SECOND half of this test used to assert `!in_concept(w, X)` — "the
// unclosable range class X must NOT be force-added — that would be inventing
// an entailment". Issue #81 shows that assertion had a false premise: `X` is
// not invented, it is ENTAILED by `Range(u,X)`, and the only reason the leaf
// lacked it was that the saturator did not fold ranges into nested
// existential witnesses. Now that it does, this fixture's leaf legitimately
// carries both `A` and `X`, and the evaluator agrees. The assertion is
// inverted rather than deleted: it now pins the fold, so a regression that
// re-drops the range fails here.
const NESTED_UNCLOSED_RANGE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/nur>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:X))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
ObjectPropertyRange(:u :X)
)
";

#[test]
#[allow(clippy::many_single_char_names)]
fn axiom_driven_expansion_keeps_base_closure_and_folds_the_range() {
    let internal = load(NESTED_UNCLOSED_RANGE);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let hier = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &hier);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(hier);
    let bounds = Bounds::default();
    let mut reasons = model.expand(&internal, &subs, &facts, &eff, &bounds);
    reasons.extend(model.expand_from_axioms(&internal, &subs, &eff, &bounds));

    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let x = internal.vocabulary.class_id("http://ex.org/X").expect("X");
    let t = internal.vocabulary.role_id("http://ex.org/t").expect("t");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let x_c = model.element_of_class(c).expect("C is satisfiable");

    let mid = model.successors(x_c, t);
    assert!(
        !mid.is_empty(),
        "C must gain a t-successor from its own axiom"
    );
    let leaf: Vec<_> = mid.iter().flat_map(|m| model.successors(*m, u)).collect();
    assert!(!leaf.is_empty(), "the u-successor must still be built out");
    let w = leaf[0];

    assert!(
        model.in_concept(w, a),
        "the atom's own base closure (A) is entailed unconditionally and must \
         survive"
    );
    assert!(
        model.in_concept(w, x),
        "Range(u,X) entails every u-successor is an X, and issue #81 folds that \
         into the nested witness — this used to assert the OPPOSITE, on the \
         mistaken premise that adding X would be inventing an entailment"
    );
}

// --- Task 5: injection to a fixpoint, and build_model itself ---------------

#[test]
fn injection_closes_the_label_so_the_healthy_fixture_needs_no_label_not_closed() {
    let ofn =
        std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn").expect("fixture");
    let internal = load(&ofn);
    let (_model, reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "injection must close the label Task 4 could only report: {reasons:?}"
    );
}

// --- Regression pin: ONE round is genuinely insufficient here -------------
//
// `label-closure-range-sub.ofn` is the fixture the design spec's own example
// uses for "two runs are NOT enough": round 1 can only DISCOVER the
// (A, u) gap (report `LabelNotClosed`, since `target_label` has no injected
// `Q` to consult yet); round 2 is what re-saturates `working` with the
// injected `Q ≡ A ⊓ F` and resolves it via `Q`'s own closure. Measured
// directly: `max_rounds: 1` trips `BoundTripped` on this exact fixture, and
// `max_rounds: 2` (or the default 8) succeeds with an empty reason list.
#[test]
fn one_round_is_insufficient_for_label_closure_range_sub() {
    let ofn =
        std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_rounds: 1,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped {
                bound: "max_rounds",
                ..
            }
        )),
        "one round must be insufficient — the injected Q is only visible starting round 2: {reasons:?}"
    );
}

#[test]
fn two_rounds_suffice_for_label_closure_range_sub() {
    let ofn =
        std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_rounds: 2,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        !reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped { .. } | UnresolvedReason::LabelNotClosed { .. }
        )),
        "round 2 is where the injected Q first becomes visible, and it must fully \
         close the label: {reasons:?}"
    );
}

// --- cascade.ofn converges in one round, and now with NO residual ----------
//
// Two corrections live here, both MEASURED against this implementation.
//
// (1) The design spec (`docs/superpowers/specs/2026-08-27-negative-
// certificates-phase1-design.md`) states as fact that "cascade.ofn needs three
// [rounds]", and this test's original form asserted `BoundTripped` at
// `max_rounds: 1`. That is wrong on this fixture: `build_model` converges in
// round 1, at ANY `max_rounds >= 1`, because there is no injectable gap for
// `inject_conjunction` to close.
//
// (2) This test also used to require a RESIDUAL marker-targeted
// `LabelNotClosed` — the fact-driven path targeting a nested existential at a
// Tseitin marker whose `Range(u)` could not be folded in. Issue #81 fixed that
// in the saturator, so the residual is gone and cascade converges CLEANLY.
// That is the engine getting more correct, not this fixture getting weaker:
// rustdl now derives `A ⊑ FINAL` on cascade.ofn, which is exactly Konclude's
// only non-trivial row and which the pre-fix engine missed entirely.
//
// The report-only contract — that a genuine unclosable gap is surfaced rather
// than silently dropped — is pinned on `topwitness.ofn` instead, whose gap is
// a documented design decision rather than a defect. See
// `verified_check_can_still_carry_nonempty_build_reasons`.
#[test]
fn cascade_converges_in_one_round_with_no_residual() {
    let ofn = std::fs::read_to_string("tests/fixtures/cascade.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_rounds: 1,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        !reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped {
                bound: "max_rounds",
                ..
            }
        )),
        "measured: cascade.ofn has no injectable gap, so it converges in round 1 \
         even at max_rounds=1 — this CONTRADICTS the design doc's \"needs three \
         rounds\" prediction: {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "issue #81 folded ObjectPropertyRange into nested existential witnesses, \
         so cascade.ofn's marker-targeted residue is CLOSED, not merely \
         unreported: {reasons:?}"
    );
}

// --- The `RunDelta` automatic-injection path lost its fixture to a FIX -------
//
// This slot held `injected_q_unsatisfiable_reports_run_delta_not_label_not_
// closed`, on `C ⊑ ∃t.∃u.A` + `Range(u,F)` + `DisjointClasses(A,F)`. It worked
// only because the saturator did NOT fold `Range(u)` into the nested `∃u.A`
// witness: `C` stayed satisfiable, round 1 injected `Q ≡ A ⊓ F`, `Q`
// re-saturated unsatisfiable, and round 2 reported `RunDelta`.
//
// Issue #81 folded that range. The witness is now `A ⊓ F`, which IS `⊥`, so
// `C` is unsatisfiable outright — Konclude agrees (`EquivalentClasses(Nothing,
// C)`), and the pre-fix engine reported no unsat at all. `C` therefore gets no
// model element, `expand_from_axioms` never builds the witness, and
// `build_model` returns ZERO reasons. The test did not detect a regression; its
// premise was a missing entailment.
//
// The test below pins what is now TRUE on that fixture. The `RunDelta`
// automatic path is left with NO known-firing fixture, recorded here rather
// than papered over with a bug-shaped vehicle: closing the gap that fed it is
// what removed it, and any replacement built on another unfolded-range shape
// would die the same way the moment that shape is fixed too.
const RUN_DELTA_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/rd>
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
ObjectPropertyRange(:u :F)
DisjointClasses(:A :F)
)
";

#[test]
fn range_folded_nested_witness_makes_the_subject_unsatisfiable() {
    let internal = load(RUN_DELTA_FIXTURE);
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let (subs, _facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    assert!(
        subs.is_unsatisfiable(c),
        "Range(u,F) folded into the nested ∃u.A witness gives A ⊓ F = ⊥, so no \
         u-successor can exist and C is unsatisfiable — Konclude reports \
         EquivalentClasses(Nothing, C) on this exact fixture"
    );
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        reasons.is_empty(),
        "with C unsatisfiable there is no element to expand and hence no gap to \
         report — a non-empty reasons list here would mean the builder is \
         inventing work on an empty class: {reasons:?}"
    );
}

#[test]
fn chain_edges_are_materialised_onto_the_declared_super_role() {
    // This used to read `chainpoison.ofn`, which paired the same
    // `Chain(t,u) ⊑ r` shape with `ObjectPropertyDomain(r, owl:Nothing)` so `C`
    // (the only class requiring the chain-materialised edge) was poisoned into
    // unsatisfiability. Issue #80/#82's saturator fix (the `Some`/`Min`
    // one-way-marker bug in `atomic_or_tseitin_body_with_extras`) closed that
    // poisoning gap, so `C` is now correctly reported unsatisfiable, gets no
    // model element at all, and this test's own `!m.edges(r).is_empty()`
    // assertion started failing — for the RIGHT reason (the model got more
    // correct), not a regression. Repointed at `chain-ok.ofn`: the identical
    // chain shape with the domain poison removed, so `C` stays satisfiable and
    // the materialised edge is not a function of any engine completeness fix.
    // Do not repoint this at another engine-defect fixture — that is exactly
    // what broke it here.
    let ofn = std::fs::read_to_string("tests/fixtures/chain-ok.ofn").expect("fixture");
    let internal = load(&ofn);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let r = internal.vocabulary.role_id("http://ex.org/r").expect("r");
    assert!(
        !m.edges(r).is_empty(),
        "Chain(t,u) ⊑ r must materialise an r-edge"
    );
}

#[test]
fn transitivity_with_a_range_is_not_refused() {
    // A materialised transitive edge's target was already an edge-target of the
    // same or a sub-role, so it already carries eff_ranges(r). Refusing would be
    // pure coverage loss over the dominant wild combination.
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/tr>
Declaration(Class(:F)) Declaration(ObjectProperty(:r))
TransitiveObjectProperty(:r)
ObjectPropertyRange(:r :F)
)
";
    let internal = load(ofn);
    let h = build_role_hierarchy(&internal);
    assert!(
        chain_range_out_of_profile(&internal, &h).is_none(),
        "TransitiveRole is exempt by construction"
    );
}

#[test]
fn a_chain_whose_head_range_is_not_covered_by_the_second_leg_is_refused() {
    let ofn = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/cr>
Declaration(Class(:F))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)
ObjectPropertyRange(:r :F)
)
";
    let internal = load(ofn);
    let h = build_role_hierarchy(&internal);
    assert!(chain_range_out_of_profile(&internal, &h).is_some());
}

// --- Task 13: the two count-based `Bounds` fields, isolated ---------------
//
// `max_rounds` (the outer `build_model` loop) and the checking-time
// `deadline` were already pinned by `one_round_is_insufficient_for_label_
// closure_range_sub` (above) and `verify_honours_an_already_elapsed_deadline_
// and_reports_it_as_a_deadline_not_a_count` (`tests/evaluator.rs`). These two
// close the remaining pair: `max_elements` and `max_edges`, both checked
// inside `FiniteModel::expand`/`push_edge`. `cascade.ofn` is reused rather
// than a fresh fixture because it is already established (Task 5) to
// materialise multiple genuinely NEW elements and edges beyond its seed
// population — exactly what a bound of `1` needs in order to be tripped by
// something other than the seed step itself (`FiniteModel::seed` never
// checks a bound at all; only `expand`'s own `intern`/`push_edge` calls do).

#[test]
fn max_elements_one_trips_bound_tripped_naming_the_bound() {
    let ofn = std::fs::read_to_string("tests/fixtures/cascade.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_elements: 1,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped {
                bound: "max_elements",
                limit: Some(1),
            }
        )),
        "cascade.ofn materialises new elements beyond its seed population, so \
         max_elements: 1 must trip BoundTripped naming \"max_elements\" with \
         limit Some(1) — a builder that silently truncated at the bound and \
         returned an empty reason list would pass everything else in this \
         suite while never surfacing that it had done so: {reasons:?}"
    );
}

#[test]
fn max_edges_one_trips_bound_tripped_naming_the_bound() {
    let ofn = std::fs::read_to_string("tests/fixtures/cascade.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_edges: 1,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped {
                bound: "max_edges",
                limit: Some(1),
            }
        )),
        "cascade.ofn's existential cascade materialises more than one edge, so \
         max_edges: 1 must trip BoundTripped naming \"max_edges\" with \
         limit Some(1): {reasons:?}"
    );
}

// --- Task 13: a genuine conjunctive trigger that ONLY full injection closes -
//
// `conjtrigger.ofn`: `C ⊑ ∃t.Y`, `Range(t,F)`, `Y ⊓ F ⊑ H`. Round 1 cannot
// close the `t`-successor's label — `target_label` finds `Y`'s own subsumer
// set does not already contain the range class `F` (`aug = [F]`), and no `Q`
// has been injected yet, so it reports `LabelNotClosed{class: Y, role: t}`.
// The injected `Q ≡ Y ⊓ F` is what makes `Y ⊓ F ⊑ H` fire AT ALL: the EL
// saturator's conjunction-introduction rule only ever matches a class whose
// OWN told-subsumer set contains both `Y` and `F` simultaneously, and that
// relationship exists only for the synthetic `Q` `inject_conjunction`
// creates — never for `Y` itself (`aug` is built as exactly the classes `y`
// does NOT already subsume, so `y ⊓ aug` can never independently satisfy the
// rule; see `inject_conjunction`'s doc and the `RUN_DELTA_FIXTURE` comment
// above, which makes the identical argument for why `run_deltas` can't fire
// on `y`).
//
// This is the fixture Step 5 of Task 13's brief asks for: a case a "cheap
// closure-union" shortcut (append `aug`'s atoms to the target label directly,
// without creating `Q` and re-saturating) would NOT close. Such a shortcut
// would produce a label containing `Y` and `F` but never `H` (nothing runs
// the `Y ⊓ F ⊑ H` rule over a label that is not itself a real class's
// told-subsumer set) — and checking the ORIGINAL axiom `SubClassOf(Y⊓F, H)`
// against that element would find the antecedent holding and the consequent
// absent, i.e. `Violated`, not `Verified`. Contrast with
// `label-closure-range-sub.ofn` (already in this suite, from Task 5): there
// the injected class's OWN closure is all that is needed (`F ⊑ G` is a plain
// subsumer fold, matching regardless of what else is in `F`'s subsumer set),
// so a cheap closure-union would have sufficed there too — it is the "one
// where the cheap closure-union would suffice" half of Step 5, already
// covered by `one_round_is_insufficient_for_label_closure_range_sub` /
// `two_rounds_suffice_for_label_closure_range_sub` above. This fixture is the
// other half.
#[test]
fn conjunctive_trigger_needs_full_injection_and_verifies() {
    let ofn = std::fs::read_to_string("tests/fixtures/conjtrigger.ofn").expect("fixture");
    let internal = load(&ofn);
    let (m, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        build_reasons.is_empty(),
        "full injection should close conjtrigger.ofn's only gap with no residual \
         reports: {build_reasons:?}"
    );
    let (verdict, _verified_model) = owl_dl_verify::verify(m, &internal, None);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "injecting Q ≡ Y ⊓ F and re-saturating must make Y ⊓ F ⊑ H hold on the \
         t-successor's label — a cheap closure-union shortcut would leave H \
         out of that label and this would report Violated instead: {verdict:?}"
    );
}

// Sabotage via an existing knob rather than editing source: at `max_rounds:
// 1`, injection is discovered but never actually takes effect (the loop
// returns at the bound before a second round can consult it) — the same
// "one round is not enough" shape as `label-closure-range-sub.ofn`, and
// direct evidence that this fixture's `Verified` result above is genuinely
// produced by the SECOND round's injected lookup, not by some other path
// that would have closed it in round 1 regardless.
//
// MEASURED (Task 13, fix round 1): this test isolates the OUTER
// `build_model` loop MORE CLEANLY than `label-closure-range-sub.ofn`'s
// equivalent test does. At `max_rounds: 1`, `label-closure-range-sub.ofn`
// trips `BoundTripped{max_rounds}` TWICE — once from `build_model`'s own
// loop and once from `expand_from_axioms`'s internal round counter, which
// shares the same `bounds.max_rounds` value (see `model.rs`'s comment above
// `one_round_is_insufficient_for_label_closure_range_sub`). `conjtrigger.ofn`
// trips it only ONCE here: `expand_from_axioms`'s own loop reaches `!grew`
// (nothing new to materialise on this fixture, since the `t`-witness is
// already interned by the FACT-driven `expand()` before `expand_from_axioms`
// runs) and returns before its internal round counter ever reaches the
// shared bound, so only the outer loop's `BoundTripped` fires. Confirmed by
// directly counting `reasons`' `BoundTripped` entries for both fixtures
// side by side (one vs. two).
#[test]
fn conjunctive_trigger_is_insufficient_at_one_round() {
    let ofn = std::fs::read_to_string("tests/fixtures/conjtrigger.ofn").expect("fixture");
    let internal = load(&ofn);
    let bounds = Bounds {
        max_rounds: 1,
        ..Bounds::default()
    };
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &bounds).expect("builds");
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            UnresolvedReason::BoundTripped {
                bound: "max_rounds",
                ..
            }
        )),
        "one round must be insufficient — Y ⊓ F ⊑ H only becomes reachable once \
         Q is injected and re-saturated, starting round 2: {reasons:?}"
    );
}

// --- `Verified` check + non-empty build-time `reasons` ---------------------
//
// `owl-dl-cli`'s `fold_build_reasons` downgrades a `Verified` CHECK result to
// `Unresolved` whenever `build_model`'s own BUILD-time `reasons` are non-empty,
// so the CLI never reports exit 0 over an admitted, unclosed gap. This test is
// what establishes that combination is REACHABLE at all; `crates/owl-dl-cli/
// tests/verify_el.rs`'s `build_reasons_downgrade_a_verified_check_to_unresolved_
// and_exit_three` is what proves the CLI acts on it.
//
// THE VEHICLE WAS REPLACED, AND WHY MATTERS MORE THAN WHAT.
// This used to read `markerresidue.ofn`, whose residue came from a SATURATOR
// DEFECT: nested existential bodies were lowered without folding the role's
// `ObjectPropertyRange`, so a fact targeted a bare Tseitin marker whose label
// `target_label` could not close. Issue #81 fixed that defect, the residue
// vanished, and FIVE tests in this file broke at once — every one of them
// built on the same bug. An `#[ignore]`d or bug-pinned test is a claim about
// the engine that goes stale silently; these at least failed loudly.
//
// `topwitness.ofn` is durable for a reason no engine fix can erase:
// `A ⊑ ∃u.⊤` lowers via `atomic_existential_rhs`'s `Top` arm to a
// DELIBERATELY subsumer-less witness ("the witness has no subsumers
// (⊤-equivalent), so it only ever triggers domain(R)"). Folding `Range(u)`
// into it would destroy the domain inference that arm exists for, so the
// unclosable augmentation is a documented design decision, not a defect.
// Nothing in the fixture checks the witness's own label, so every WRITTEN
// axiom holds and the CHECK verdict is `Verified` regardless.
//
// Do not repoint this at a fixture whose residue is an engine defect — that
// is exactly what broke its predecessor.
#[test]
fn verified_check_can_still_carry_nonempty_build_reasons() {
    let ofn = std::fs::read_to_string("tests/fixtures/topwitness.ofn").expect("fixture");
    let internal = load(&ofn);
    let (m, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        !build_reasons.is_empty(),
        "topwitness.ofn must produce a residual LabelNotClosed from build_model \
         (the ⊤-witness is subsumer-less by design, so Range(u) cannot be folded \
         into it) — pairing that with a Verified check is the whole point of this \
         fixture: {build_reasons:?}"
    );
    assert!(
        build_reasons
            .iter()
            .all(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "expected only LabelNotClosed reasons on this fixture, got: {build_reasons:?}"
    );
    let (verdict, _verified_model) = owl_dl_verify::verify(m, &internal, None);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "every WRITTEN axiom in topwitness.ofn holds in the model as built — \
         nothing checks the ⊤-witness's own label — so the CHECK result must be \
         Verified even though build_model's own reasons are non-empty: {verdict:?}"
    );
}

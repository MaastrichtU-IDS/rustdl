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

#[test]
fn derived_equivalent_classes_share_one_element() {
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

// --- LabelNotClosed, positive case (report-only path actually reports) ------
//
// `∃u.A` nested inside an `ObjectIntersectionOf` on the RHS of `∃t.(...)` DOES
// reach `target_label` for role `u` (via `introduce_equivalent_existential_marker`,
// a two-way Tseitin marker that DOES push its own existential fact) — unlike the
// bare `∃t.∃u.A` shape below, whose one-way marker never does. `A`'s target is
// NOT range-folded by this path either, so `aug = [F]` is non-empty and `expand`
// must report `LabelNotClosed` rather than guess. This is the positive
// counterpart to `nested_existential_range_gap_*`: it demonstrates the
// report-only contract actually firing on a real ontology, not just on the
// pre-supplied fixture that (per the report) never reaches `target_label` at all.
const AND_WRAPPED_NESTED_RANGE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/andwrapped>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:F))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectIntersectionOf(:B ObjectSomeValuesFrom(:u :A))))
ObjectPropertyRange(:u :F)
)
";

#[test]
fn and_wrapped_nested_existential_reports_label_not_closed() {
    let internal = load(AND_WRAPPED_NESTED_RANGE);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let mut model = FiniteModel::seed(&internal, &subs, &facts);
    let reasons = model.expand(&internal, &subs, &facts, &eff, &Bounds::default());
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { role, .. } if *role == u)),
        "the inner ∃u.A target is not range-folded by this Tseitin path either, so \
         expand must report rather than truncate: got {reasons:?}"
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

// --- Review fix: base closure must survive an unclosable range augmentation -
//
// Fix-round-1 finding: `materialise_exists`'s `Some` arm looked up each
// required atom individually via `target_label`, and on `Err` (an unclosable
// range augmentation) dropped the atom's OWN base closure too, not just the
// disputed range classes. `C ⊑ ∃t.∃u.A` + `Range(u,X)` with no `A ⊑ X` is the
// reviewer's exact reproducer: the leaf witness's label came back completely
// EMPTY, so `in_concept(leaf, A)` was `false` even though the axiom trivially
// entails the witness is an `A`. Fixed by extending `label` with
// `subs.subsumers_of(*a)` on the `Err` arm too, while still reporting
// `LabelNotClosed` for the augmentation that could not be closed.
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
fn axiom_driven_expansion_keeps_base_closure_when_the_range_augmentation_is_unclosable() {
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
         survive even though the range augmentation (X) could not be closed"
    );
    assert!(
        !model.in_concept(w, x),
        "the unclosable range class X must NOT be force-added — that would be \
         inventing an entailment, not recovering a real one"
    );
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { role, .. } if *role == u)),
        "the unclosed range augmentation must still be reported: {reasons:?}"
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

// --- cascade.ofn: MEASURED CORRECTION of a design-doc prediction ----------
//
// The design spec (`docs/superpowers/specs/2026-08-27-negative-certificates-
// phase1-design.md`) states as fact that "cascade.ofn needs three [rounds]"
// and this task's own brief repeated that claim verbatim in this test's
// original form (asserting `BoundTripped` at `max_rounds: 1`). MEASURED
// against this implementation, that is wrong on this fixture: `build_model`
// converges in round 1, at ANY `max_rounds >= 1`.
//
// Root cause, confirmed by calling `expand`/`expand_from_axioms` separately
// (see the task-5 report): `expand_from_axioms` alone reports ZERO
// `LabelNotClosed` on this fixture's first round — there is no injectable
// gap for `inject_conjunction` to close at all. The ONLY `LabelNotClosed`
// entries come from `expand`'s fact-driven path, and they name a Tseitin
// MARKER (`saturate_with_exists_facts` targets a nested-existential fact at
// its marker), not a real class. A marker has no IRI to key an injected `Q`
// on, and — because injecting a class shifts `working.vocabulary.
// num_classes()`, which is where `TseitinAllocator` bases the NEXT round's
// marker ids — the same marker-targeted gap would get a DIFFERENT class id
// every round even if injection were attempted, so it could never converge
// via this mechanism. `build_model` therefore excludes marker-targeted
// `LabelNotClosed` from `pending` (so it cannot block convergence) while
// still surfacing it in the returned `reasons` (so it is not silently
// dropped — a marker-targeted report is genuine evidence that the
// fact-driven path could not close something, even though nothing here
// blocks the model from being built).
#[test]
fn cascade_converges_in_one_round_with_residual_marker_targeted_reports() {
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
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "the residual gap is real (a Tseitin-marker target the fact-driven path \
         cannot close) and must still be reported, not silently dropped: {reasons:?}"
    );
}

// --- Review finding 3 (Task 5, round 1): a genuine positive RunDelta fixture -
//
// Constructed (not lifted from the design spec, since Finding 2 established
// `unsatnested.ofn` does not exercise the AUTOMATIC injection path): `C ⊑
// ∃t.∃u.A`, `Range(u,F)`, `DisjointClasses(A,F)`. Round 1 reports
// `LabelNotClosed{A,u}` (aug=[F]) exactly like `label-closure-range-sub.ofn`.
// Round 1's injection adds `Q ≡ A ⊓ F` — but here `A` and `F` are told-disjoint,
// so `Q` re-saturates UNSATISFIABLE. Round 2's `lookup_injected` finds `Q`,
// sees it is unsatisfiable, and both `expand` and `expand_from_axioms` push
// `RunDelta { class: A }` instead of `LabelNotClosed` — this is a genuine,
// reproducible firing of the injected-Q-unsatisfiable branch inside
// `materialise_exists`/`expand` (via the shared `lookup_injected` helper).
//
// This does NOT exercise `run_deltas` (the SEPARATE, top-level function in
// `lib.rs` comparing `first_subs`/`final_subs` over every ORIGINAL class):
// checked directly (`first_subs.is_unsatisfiable(a)` vs the round-2 `subs.
// is_unsatisfiable(a)`, and likewise for `C`) — neither flips. This matches
// the architecture: `aug` is constructed (in `inject_conjunction`) to be
// exactly the range classes `y` does NOT already subsume, so the EL
// saturator's own conjunction-introduction rule (`y ⊓ aug ⊑ Q`) can never
// fire on `y` alone — only the synthetic witness `Q` ever satisfies both
// sides at once. I could not find, after this attempt plus reasoning through
// why it can't work, a fixture where injecting `Q` changes an ORIGINAL
// class's own `is_unsatisfiable` verdict under the current mechanism.
// `run_deltas`'s top-level comparison therefore has NO known-firing fixture;
// it is exercised on every existing test only in the sense of "compared and
// found no delta," never "compared and found one." Documented here rather
// than left silently looking covered.
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
fn injected_q_unsatisfiable_reports_run_delta_not_label_not_closed() {
    let internal = load(RUN_DELTA_FIXTURE);
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let (_m, reasons) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::RunDelta { class } if *class == a)),
        "the injected Q ≡ A ⊓ F is unsatisfiable (A, F told-disjoint), so this must \
         be reported as RunDelta, not silently dropped or reported as LabelNotClosed \
         (which would wrongly imply a plain reporting limitation, not a defect): \
         {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { .. })),
        "once the injected Q's unsatisfiability is known, this specific gap must not \
         ALSO still be reported as an unresolved (not-yet-injected) LabelNotClosed: \
         {reasons:?}"
    );
}

#[test]
fn chain_edges_are_materialised_onto_the_declared_super_role() {
    let ofn = std::fs::read_to_string("tests/fixtures/chainpoison.ofn").expect("fixture");
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

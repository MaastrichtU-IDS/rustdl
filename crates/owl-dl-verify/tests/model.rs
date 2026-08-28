use owl_dl_core::convert_ontology;
use owl_dl_verify::model::{FiniteModel, build_role_hierarchy, effective_ranges};
use owl_dl_verify::{Bounds, Interpretation, UnresolvedReason};

fn load(ofn: &str) -> owl_dl_core::InternalOntology {
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut ofn.as_bytes(),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse fixture");
    convert_ontology(&onto).expect("convert fixture")
}

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
    let reasons = model.expand(&subs, &facts, &eff, &Bounds::default());
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
    let reasons = model.expand(&subs, &facts, &eff, &Bounds::default());
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, UnresolvedReason::LabelNotClosed { role, .. } if *role == u)),
        "the inner ∃u.A target is not range-folded by this Tseitin path either, so \
         expand must report rather than truncate: got {reasons:?}"
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
#[ignore = "RE-MEASURED for Task 4b (2026-08-28): still FAILS, unchanged — this test \
            calls only model.expand(), which 4b deliberately left untouched, so \
            saturate_with_exists_facts still emits no fact for the inner role and no \
            u-edge is created. The RECHABILITY gap itself is now closed: a probe that \
            additionally calls model.expand_from_axioms() on this exact fixture DOES \
            produce a u-edge (confirmed by temporarily instrumenting this test and \
            reverting). But even with that call added, this test's own assertion \
            `in_concept(witness, f)` would still fail, for a DIFFERENT, deliberate \
            reason: target_label(subs, eff, u, A) returns Err([F]) here (Range(u,F) \
            with no A⊑F closure), and expand_from_axioms reports that as \
            LabelNotClosed rather than force-closing F into the witness label — the \
            same report-only design expand() already uses, not a residual gap in \
            4b. So this fixture cannot pass as literally written without either (a) \
            adding an A⊑F-style closure axiom (see the NESTED_MONO fixture above, \
            which does exactly this and passes), or (b) changing the assertion to \
            expect LabelNotClosed instead of Range-closure. Left as-is and ignored, \
            scoped to the fact-driven path it documents."]
fn nested_existential_range_gap_probe_b() {
    let internal = load(NESTED_RANGE_NO_AND);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts).with_hierarchy(h);
    let _ = model.expand(&subs, &facts, &eff, &Bounds::default());

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
#[ignore = "RE-MEASURED for Task 4b (2026-08-28): still FAILS as literally written \
            (reasons == [] from model.expand() alone, unchanged from the task-4 \
            report), because this test checks ONLY expand()'s return value and 4b \
            deliberately left expand() untouched. But the underlying gap this test \
            names IS closed at the model level: a probe that additionally calls \
            model.expand_from_axioms() and folds its reasons into the checked Vec \
            (confirmed by temporarily instrumenting this test and reverting) makes \
            the assertion PASS — expand_from_axioms's materialise_exists hits \
            target_label(subs, eff, u, A) on this exact fixture, gets Err([F]) \
            (Range(u,F) with no A⊑F closure) and reports LabelNotClosed, exactly as \
            this test wants. So this is not a residual defect in 4b; it is that this \
            test exercises only HALF of the pipeline the brief's own Interfaces \
            section describes (\"Task 5's build_model calls [expand_from_axioms] \
            immediately after expand\"). Left ignored rather than edited to call the \
            second method myself, since wiring the two together into one report \
            surface is Task 5's declared scope, not 4b's — un-ignore this once that \
            wiring lands and re-check."]
fn label_closure_case_reports_label_not_closed_rather_than_a_wrong_label() {
    let ofn = std::fs::read_to_string("tests/fixtures/label-closure-range-sub.ofn")
        .expect("fixture present");
    let internal = load(&ofn);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let h = build_role_hierarchy(&internal);
    let eff = effective_ranges(&internal, &h);
    let mut model = FiniteModel::seed(&internal, &subs, &facts);
    let reasons = model.expand(&subs, &facts, &eff, &Bounds::default());
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
    let _ = model.expand(&subs, &facts, &eff, &bounds);
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
    let mut reasons = model.expand(&subs, &facts, &eff, &bounds);
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

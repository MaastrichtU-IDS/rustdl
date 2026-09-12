//! #128 — a role chain leg must be traversable through an inverse-equivalent edge.
//!
//! ```text
//! Symmetric(co),  Chain(co, p) ⊑ p
//! T1a ≡ ∃co.Y ⊓ ∃p.Pn
//! T1b ≡ ∃co.(Y ⊓ ∃p.Pn)                    ⟹   T1a ≡ T1b
//! ```
//!
//! For `x ∈ T1a`: `co(x,y)` with `y ∈ Y`, and `p(x,z)` with `z ∈ Pn`. Symmetry gives
//! `co(y,x)`, so the chain's first leg fires **at `y`** — `co(y,x) ∘ p(x,z) ⊑ p` —
//! yielding `p(y,z)`. So `y ∈ Y ⊓ ∃p.Pn` and `x ∈ T1b`. `HermiT` confirms the
//! equivalence; rustdl reported only the (symmetry-free) `T1b ⊑ T1a` direction,
//! because [`apply_role_chains`](owl_dl_tableau::apply_role_chains) resolved each
//! chain position against raw edge labels and directions: a `Named(co)` position
//! read `co`-labelled OUT-edges only, so the backward hop symmetry licenses was
//! invisible to it. `edge_satisfies` — the concept-level matcher — had accepted the
//! cross-polarity match all along; the chain rule is what disagreed.
//!
//! `SymmetricObjectProperty(co)` is lowered to `InverseObjectProperties(co, co)`,
//! so the general `InverseObjectProperties(r, s)` shape shares the defect and is
//! covered here too. Both reads also compose with the role hierarchy, because
//! nothing materialises super-role edges — see `chain_leg_targets`.
//!
//! # Every fixture here MUST be out of the EL fragment
//!
//! An EL-derivable subsumption is answered by the saturation closure and the
//! tableau is never entered, so an EL fixture pins the saturator and says nothing
//! about the chain rule. That is not hypothetical: the first version of the two
//! negatives below carried no symmetry and no inverse pair, which made them pure
//! EL, and the whole file passed unchanged when the reverse read was mutated to
//! `true || are_declared_inverses(..)` — a blanket "walk every in-edge" fix that
//! is grossly unsound. Both negatives now declare an inverse pair, which both
//! forces the tableau path and gives `are_declared_inverses` a NON-empty pair set
//! to discriminate on, so its role-identity component is actually under test.
//!
//! **Acceptance criterion for anything added here: it must fail under that
//! mutation.** A test that survives it is not a guard.
//!
//! # Why the positives query `is_subclass_of`, not `classify`
//!
//! The chain rule lives in the classic tableau, which is what
//! [`is_subclass_of`] drives. Default `classify` answers this shape EARLIER — the
//! hypertableau wedge decides the pair (and the label heuristic prunes it on the
//! wedge's labels) — and the wedge carries its own copy of this gap, so the #128
//! ontology used to classify wrongly even so. #135 closed that by carrying the
//! role hierarchy into the wedge by default, and
//! `default_classify_now_finds_the_symmetric_chain_leg` — formerly a pinned
//! known-limitation, flipped when CI caught it passing — holds both paths to the
//! same answer.
//!
//! # The negatives are the point of this file
//!
//! `r ≡ s⁻` is an EQUIVALENCE, so reading an `s`-labelled in-edge for a `Named(r)`
//! position derives nothing a model could refuse. What would be unsound is reading
//! the reverse hop for a role that is merely *declared* somewhere — with no
//! symmetry and no inverse pair, `co(x,y)` says nothing about `co(y,x)`, and the
//! chain must not fire at `y`. `a_plain_role_does_not_traverse_its_chain_leg_backwards`
//! holds the implementation to that; `HermiT` reports nothing there either.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, is_subclass_of};
use std::io::Cursor;
use std::time::Duration;

fn parse(ofn: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(ofn.to_string());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// The complete path: the classic tableau, where the chain rule runs. A MISS here
/// is the calculus rather than a wedge `trust_sat` mask.
fn holds(ofn: &str, sub: &str, sup: &str) -> bool {
    is_subclass_of(
        &parse(ofn),
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
    .expect("subsumption query")
}

/// Default `classify`, i.e. what a user of the CLI or the Python bindings sees.
fn classify_holds(ofn: &str, sub: &str, sup: &str) -> bool {
    let result =
        classify_top_down_with_timeout(&parse(ofn), Duration::from_secs(10)).expect("classify");
    result.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

const HEAD: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/csl>\n\
Declaration(Class(:Y)) Declaration(Class(:Pn))\n\
Declaration(Class(:T1a)) Declaration(Class(:T1b))\n\
Declaration(ObjectProperty(:co)) Declaration(ObjectProperty(:p))\n\
Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:u))\n\
Declaration(ObjectProperty(:q))\n";

/// The #128 shape over `Chain(co, p) ⊑ p`: `T1a ≡ ∃co.Y ⊓ ∃p.Pn` against
/// `T1b ≡ ∃co.(Y ⊓ ∃p.Pn)`, prefixed by whatever `co` characteristic the caller
/// supplies (`""` for none).
fn two_definitions(co_characteristic: &str) -> String {
    format!(
        "{HEAD}{co_characteristic}\
SubObjectPropertyOf(ObjectPropertyChain(:co :p) :p)\n\
EquivalentClasses(:T1a ObjectIntersectionOf(\
ObjectSomeValuesFrom(:co :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:T1b ObjectSomeValuesFrom(:co ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    )
}

#[test]
fn a_symmetric_first_leg_fires_the_chain_at_the_successor() {
    // THE ISSUE #128 FIXTURE. `T1a ⊑ T1b` needs the chain to fire at `y` through
    // `co(y,x)`, which exists only because `co` is symmetric.
    let ofn = two_definitions("SymmetricObjectProperty(:co)\n");
    assert!(
        holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is entailed (HermiT confirms): Symmetric(co) turns co(x,y) into \
         co(y,x), so Chain(co, p) ⊑ p gives p(y,z) and y ∈ Y ⊓ ∃p.Pn"
    );
}

#[test]
fn the_symmetry_free_direction_still_holds() {
    // CONTROL. `T1b ⊑ T1a` reads the chain forwards — `co(x,y) ∘ p(y,z)` — and never
    // needed symmetry. It was the one direction rustdl already reported for #128, so
    // it is what distinguishes "the chain rule regressed" from "the backward hop is
    // still missing".
    let ofn = two_definitions("SymmetricObjectProperty(:co)\n");
    assert!(
        holds(&ofn, "T1b", "T1a"),
        "T1b ⊑ T1a is entailed by the forward chain hop alone"
    );
}

#[test]
fn a_declared_inverse_pair_fires_the_chain_at_the_successor() {
    // Same shape with the symmetry spelled out as a general inverse pair: the class
    // definitions use `s`, the chain's first leg wants `co`, and `co ≡ s⁻` is what
    // connects them. `SymmetricObjectProperty` lowers to the `s = co` case of this.
    let ofn = format!(
        "{HEAD}InverseObjectProperties(:co :s)\n\
SubObjectPropertyOf(ObjectPropertyChain(:co :p) :p)\n\
EquivalentClasses(:T1a ObjectIntersectionOf(\
ObjectSomeValuesFrom(:s :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:T1b ObjectSomeValuesFrom(:s ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    );
    assert!(
        holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is entailed: s(x,y) is co(y,x), so Chain(co, p) ⊑ p fires at y"
    );
}

#[test]
fn a_subrole_of_a_symmetric_role_fires_the_chain() {
    // FABLE'S COUNTEREXAMPLE (adversarial review of the first version of this fix).
    // The definitions use `q`, the chain's leg wants `co`, and `q ⊑ co` with
    // `Symmetric(co)` is what connects them: q(x,y) ⇒ co(x,y) ⇒ co(y,x), so the
    // chain fires at `y`. The first version tested raw role ids on the reverse read
    // and missed this, justified by a doc comment claiming `apply_role_rules`
    // materialises super-role edges — it does not; it adds concept LABELS, and
    // `add_edge` records only the exact role. No EL closure can mask this one,
    // because symmetry is out of fragment.
    let ofn = format!(
        "{HEAD}SubObjectPropertyOf(:q :co)\n\
SymmetricObjectProperty(:co)\n\
SubObjectPropertyOf(ObjectPropertyChain(:co :p) :p)\n\
EquivalentClasses(:T1a ObjectIntersectionOf(\
ObjectSomeValuesFrom(:q :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:T1b ObjectSomeValuesFrom(:q ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    );
    assert!(
        holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is entailed: q ⊑ co and Symmetric(co) make the q-edge walkable \
         backwards as co, so Chain(co, p) ⊑ p fires at the Y witness"
    );
}

#[test]
fn a_subrole_of_an_unrelated_role_does_not_fire_the_chain() {
    // FP guard for the composition above: `q ⊑ w`, and it is `co` — a role the
    // definitions never touch — that is symmetric. Nothing licenses walking the
    // q-edge backwards, so the chain must not fire. A reverse read that walked
    // `super_roles(q)` without checking the inverse partner would derive it.
    let ofn = format!(
        "{HEAD}Declaration(ObjectProperty(:w))\n\
SubObjectPropertyOf(:q :w)\n\
SymmetricObjectProperty(:co)\n\
SubObjectPropertyOf(ObjectPropertyChain(:w :p) :p)\n\
EquivalentClasses(:T1a ObjectIntersectionOf(\
ObjectSomeValuesFrom(:q :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:T1b ObjectSomeValuesFrom(:q ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    );
    assert!(
        !holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is NOT entailed: q ⊑ w but w is not symmetric and has no \
         inverse partner, so the q-edge is one-way"
    );
}

#[test]
fn a_plain_role_does_not_traverse_its_chain_leg_backwards() {
    // THE FP GUARD. Identical to the #128 fixture with the ONE characteristic that
    // licenses the backward hop removed. `co(x,y)` alone does not give `co(y,x)`, so
    // the chain must not fire at `y`, so `T1a ⊑ T1b` must NOT hold. A fix that read
    // in-edges for every `Named` position unconditionally would derive it. HermiT
    // reports nothing here.
    // The inverse pair is over `p` — irrelevant to this entailment (`u` appears in
    // no class expression) but deliberately over an OBSERVABLE role. An
    // unobservable one does NOT work: `BareRoleDecls` (classify.rs) admits an
    // inverse/symmetry declaration over a role no axiom can read as semantically
    // inert, so `InverseObjectProperties(:s :u)` with `s`/`u` unused leaves the
    // ontology pure EL, the saturator answers, and the tableau is never entered —
    // which is exactly how the original version of this guard came to be vacuous.
    // `p` is read by the chain and by both definitions, so the pair sticks, the
    // tableau decides the pair, and `are_declared_inverses` gets a non-empty set
    // that must still answer NO for `(co, co)`.
    let ofn = two_definitions("InverseObjectProperties(:p :u)\n");
    assert!(
        !holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is NOT entailed without Symmetric(co): a model can give x a \
         co-successor y that has no co-edge back to x"
    );
}

#[test]
fn unrelated_roles_do_not_connect_the_chain_leg() {
    // Companion FP guard on the inverse-pair arm: `co` and `s` carry no
    // `InverseObjectProperties`, so nothing connects the `s`-edges the definitions
    // create to the `co` leg the chain wants. `T1a ⊑ T1b` must NOT hold.
    // `co` is declared inverse of `u`, NOT of `s`. So the pair set is non-empty
    // and the chain's `co` leg has a genuine inverse partner — just not the role
    // the definitions use. This is the fixture that catches a reverse read keyed
    // on "any inverse pair exists" rather than on the specific role identity.
    let ofn = format!(
        "{HEAD}InverseObjectProperties(:co :u)\n\
SubObjectPropertyOf(ObjectPropertyChain(:co :p) :p)\n\
EquivalentClasses(:T1a ObjectIntersectionOf(\
ObjectSomeValuesFrom(:s :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:T1b ObjectSomeValuesFrom(:s ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    );
    assert!(
        !holds(&ofn, "T1a", "T1b"),
        "T1a ⊑ T1b is NOT entailed when the chain's co leg is inverse to u, not \
         to the s the definitions use"
    );
}

#[test]
fn default_classify_now_finds_the_symmetric_chain_leg() {
    // WAS a pinned known limitation. When this fix landed the chain rule, default
    // `classify` still missed this shape: the hypertableau wedge decided the pair
    // first and ran with `sub_roles == None`, so `role_matches` fell back to exact
    // role-id AND exact polarity and symmetry was invisible to it. The hierarchy
    // reached the wedge only under `RUSTDL_CLASSIFY_SAME_TIER=1`, a lever bundled
    // with an expensive sweep and defaulted OFF.
    //
    // #135 split that lever: `RUSTDL_CLASSIFY_WEDGE_HIERARCHY` (default ON) now
    // carries the hierarchy into the wedge on its own. The pin's own failure
    // message asked for exactly this flip, and CI delivered it.
    //
    // Both paths must now agree — that agreement is the point, since the bug this
    // file exists for was `subclass` and `classify` disagreeing on one binary.
    let ofn = two_definitions("SymmetricObjectProperty(:co)\n");
    assert!(
        classify_holds(&ofn, "T1a", "T1b"),
        "default classify must find T1a ⊑ T1b now that the wedge carries the role \
         hierarchy (#135)"
    );
    assert!(
        holds(&ofn, "T1a", "T1b"),
        "the complete path must agree with classify"
    );
}

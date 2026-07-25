//! Canaries for issue #40: the wedge clausifier's `Axiom::DisjointUnion` arm
//! emitted pairwise disjointness + `member ⊑ class` (Horn) but explicitly
//! DEFERRED the covering half `class ⊑ ⊔members`. `classify()` (which the
//! Protégé plugin's class hierarchy uses) routes through the wedge, so it
//! MISSED covering-dependent subsumptions that the direct tableau
//! (`is_subclass_of`/`explain`) already derived. See
//! `docs/superpowers/specs/2026-07-25-disjointunion-wedge-covering-design.md`.
//!
//! NOTE on the main canary's fixture: `classify()`'s default top-down
//! tier-walk has a pre-existing, orthogonal "same-tier siblings" blind spot
//! — two classes placed in the same closure-subsumer-count tier never test
//! each other directly (see `find_direct_parents_top_down` /
//! `classify.rs`'s tier-grouping comment). In the *minimal* `DisjointUnion`
//! fixture (`C`, `D1`, `D2`, `E` with no other axioms), `C` and `E` land in
//! the same tier (both have a told-subsumer count of 1: themselves), so the
//! walk never probes the `(C, E)` pair at all — independent of whether the
//! covering clause is emitted. `classify_n2` (full `n²` pairwise) and
//! `is_subclass_of` (direct tableau) both already confirm the fix is
//! correct at the engine level regardless of this. To exercise the fix
//! through the *default* `classify()` entry point specifically (as the task
//! requires), the canary below adds one inert axiom (`SubClassOf(:C :Q)`)
//! that bumps `C`'s subsumer count so it lands in a *later* tier than `E` —
//! at which point the walk's frontier includes `E` (already placed
//! top-level) and the pair is genuinely probed. Confirmed empirically: this
//! fixture reproduces the RED (classify default: C⊑E = false pre-fix) and
//! GREEN (true post-fix) via `RUSTDL_DEBUG_TIERS=1` tier tracing (temporary,
//! not shipped).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

const HEADER: &str = "Prefix(:=<http://ex/#>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

/// RED→GREEN: `DisjointUnion(:C :D1 :D2)` + `D1 ⊑ E` + `D2 ⊑ E` entails
/// `C ⊑ E` (since `C ⊑ D1⊔D2 ⊑ E` via the covering direction). Before the
/// fix, `classify()` misses this — the wedge clausifier deferred the
/// covering clause `C(X) → D1(X) ∨ D2(X)`. `SubClassOf(:C :Q)` is an inert
/// axiom (see the module doc) that shifts `C` into a tier processed after
/// `E`, so the default top-down walk actually probes the `(C, E)` pair
/// instead of hitting the unrelated same-tier blind spot.
#[test]
fn disjoint_union_covering_entails_superclass_subsumption() {
    let src = format!(
        "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D1))\nDeclaration(Class(:D2))\n\
Declaration(Class(:E))\nDeclaration(Class(:Q))\n\
DisjointUnion(:C :D1 :D2)\n\
SubClassOf(:D1 :E)\nSubClassOf(:D2 :E)\nSubClassOf(:C :Q)\n)\n"
    );
    let onto = load(&src);
    let c = classify(&onto).expect("classify");
    assert!(
        c.is_subclass("http://ex/#C", "http://ex/#E"),
        "expected C ⊑ E via the DisjointUnion covering direction \
         (C ⊑ D1⊔D2 ⊑ E)"
    );
}

/// The literal minimal fixture from the design doc (no tier-shifting
/// filler): confirms the fix is correct at the ENGINE level via
/// `classify_n2` (full pairwise) and `is_subclass_of` (direct tableau),
/// decoupled from the default `classify()` top-down walk's unrelated
/// same-tier blind spot documented above.
#[test]
fn disjoint_union_covering_entails_superclass_subsumption_engine_level() {
    let src = format!(
        "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D1))\nDeclaration(Class(:D2))\n\
Declaration(Class(:E))\n\
DisjointUnion(:C :D1 :D2)\n\
SubClassOf(:D1 :E)\nSubClassOf(:D2 :E)\n)\n"
    );
    let onto = load(&src);
    let c_n2 = owl_dl_reasoner::classify_n2(&onto).expect("classify_n2");
    assert!(
        c_n2.is_subclass("http://ex/#C", "http://ex/#E"),
        "expected C ⊑ E via classify_n2 (full pairwise)"
    );
    let direct = owl_dl_reasoner::is_subclass_of(&onto, "http://ex/#C", "http://ex/#E")
        .expect("is_subclass_of");
    assert!(direct, "expected C ⊑ E via the direct tableau probe");
}

/// Regression (a): a covering-dependent UNSAT case. `DisjointUnion(:C :D1
/// :D2)` + `D1 ⊑ ⊥` + `D2 ⊑ ⊥` ⟹ `C` is unsatisfiable (C ⊑ D1⊔D2 ⊑ ⊥⊔⊥ = ⊥).
#[test]
fn disjoint_union_covering_empty_members_makes_class_unsatisfiable() {
    let src = format!(
        "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D1))\nDeclaration(Class(:D2))\n\
DisjointUnion(:C :D1 :D2)\n\
SubClassOf(:D1 owl:Nothing)\nSubClassOf(:D2 owl:Nothing)\n)\n"
    );
    let onto = load(&src);
    let c = classify(&onto).expect("classify");
    assert!(
        c.unsatisfiable_classes().contains(&"http://ex/#C"),
        "expected C unsatisfiable (D1 and D2 both empty ⟹ their union is \
         empty, and C is covered by that union); unsatisfiable classes: {:?}",
        c.unsatisfiable_classes()
    );
}

/// Regression (b): retain the pairwise-disjoint half — a class forced into
/// `D1 ⊓ D2` is unsatisfiable regardless of the covering fix.
#[test]
fn disjoint_union_pairwise_disjoint_still_fires() {
    let src = format!(
        "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D1))\nDeclaration(Class(:D2))\n\
Declaration(Class(:G))\n\
DisjointUnion(:C :D1 :D2)\n\
SubClassOf(:G ObjectIntersectionOf(:D1 :D2))\n)\n"
    );
    let onto = load(&src);
    let c = classify(&onto).expect("classify");
    assert!(
        c.unsatisfiable_classes().contains(&"http://ex/#G"),
        "expected G unsatisfiable (G ⊑ D1 ⊓ D2, and D1/D2 are pairwise \
         disjoint members of the DisjointUnion); unsatisfiable classes: {:?}",
        c.unsatisfiable_classes()
    );
}

/// Regression (c): a `DisjointUnion` with a non-atomic member (an
/// `ObjectSomeValuesFrom`) must classify without panicking — `class_id_of`
/// returns `None` for it, so the covering clause is soundly deferred rather
/// than emitted with a bogus atom.
#[test]
fn disjoint_union_complex_member_defers_without_panic() {
    let src = format!(
        "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D1))\nDeclaration(Class(:F))\n\
Declaration(ObjectProperty(:r))\n\
DisjointUnion(:C :D1 ObjectSomeValuesFrom(:r :F))\n)\n"
    );
    let onto = load(&src);
    let c = classify(&onto).expect("classify should not panic on a complex DisjointUnion member");
    // No specific entailment asserted here — just soundness (no panic) and
    // that classify still produces a usable result.
    assert!(!c.classes().is_empty());
}

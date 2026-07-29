//! Canaries for `X ⊓ Y ⊑ ⊥` (the lowered-`⊥` disjointness GCI) in the EL saturator.
//!
//! Lever 1b (commit 3e3a731) admitted this form to the fragment gate, but the
//! saturator's rule collector derived heads only from an atomic or existential
//! RHS — with `sup = Bot` both are empty, so the axiom was SILENTLY DROPPED while
//! the gate certified the closure complete (the D10 unsound-completeness class).
//!
//! Run: `cargo test -p owl-dl-reasoner --test conjunctive_unsat`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Classify `body` and return the sorted list of unsatisfiable class IRIs.
fn unsat_of(body: &str) -> Vec<String> {
    let onto = parse(body);
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

const DECLS: &str = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
";

/// THE BUG REPRODUCER. `C ⊑ A`, `C ⊑ B`, `A ⊓ B ⊑ ⊥` ⟹ `C` unsatisfiable.
/// Before the fix this returns an EMPTY unsat set while printing
/// "pure-EL — saturator alone is complete".
#[test]
fn conjunctive_bot_derives_unsat() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "C ⊑ A, C ⊑ B, A ⊓ B ⊑ ⊥ entails C ⊑ ⊥"
    );
}

/// SPELLING DIFFERENTIAL — the direct gate for the bug. The same ontology
/// written `A ⊓ B ⊑ ⊥` and `DisjointClasses(A B)` must classify identically.
#[test]
fn conjunctive_bot_matches_disjoint_classes_spelling() {
    let and_bot = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let disjoint = format!(
        "{DECLS}    DisjointClasses(:A :B)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    assert_eq!(
        unsat_of(&and_bot),
        unsat_of(&disjoint),
        "the two spellings of disjointness must produce the same closure"
    );
}

/// FP GUARD (negatives-first). A class with only ONE of the two conjuncts must
/// stay satisfiable. Guards against a rule that fires on a partial body match.
#[test]
fn conjunctive_bot_does_not_over_fire() {
    let body = format!(
        "{DECLS}    Declaration(Class(:D))
    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)
    SubClassOf(:D :A)"
    );
    assert_eq!(
        unsat_of(&body),
        vec!["http://t/C".to_string()],
        "D has only A, so D must remain satisfiable"
    );
}

// ── Bug 2b-1: `⊤ ⊑ ⊥` (Top LHS, sup = Bot) ────────────────────────────────

/// BUG REPRODUCER for `⊤ ⊑ ⊥`.
/// A KB containing `SubClassOf(owl:Thing owl:Nothing)` is globally inconsistent:
/// the empty-domain axiom forces every named class to be unsatisfiable.
/// Convention (established by `classify_inconsistent` in `classify.rs` and the
/// `ABox` pre-check path): every named class is reported as unsatisfiable when the
/// ontology is inconsistent.
/// Before the fix the `Top` LHS arm's `atomic_operands_on_right(Bot, pool)` returns
/// empty ⟹ axiom silently DROPPED ⟹ classifier reports zero unsatisfiable classes
/// while printing "pure-EL (`trust_sat` sound by construction)".
#[test]
fn top_bot_all_classes_unsat() {
    // Two named classes; both must appear in the unsat set.
    let body = "    Declaration(Class(:A))
    Declaration(Class(:C))
    SubClassOf(owl:Thing owl:Nothing)
    SubClassOf(:C :A)";
    let mut got = unsat_of(body);
    got.sort();
    assert_eq!(
        got,
        vec!["http://t/A".to_string(), "http://t/C".to_string()],
        "⊤ ⊑ ⊥ is a globally inconsistent KB: every named class must be unsatisfiable"
    );
}

/// FP GUARD for `⊤ ⊑ ⊥`: the SAME ontology WITHOUT the `owl:Thing ⊑ owl:Nothing`
/// axiom must have ZERO unsatisfiable classes.
#[test]
fn top_bot_no_fp_without_global_axiom() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:C))
    SubClassOf(:C :A)";
    assert_eq!(
        unsat_of(body),
        Vec::<String>::new(),
        "without ⊤ ⊑ ⊥, no class should become unsatisfiable"
    );
}

// ── Bug 2b-2: `∃r.A ⊑ ⊥` (Some LHS, sup = Bot) ────────────────────────────

/// BUG REPRODUCER for `∃r.A ⊑ ⊥`.
/// `SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)` means nothing may have an
/// r-successor typed A.  Combined with `SubClassOf(:C ObjectSomeValuesFrom(:r :A))`
/// this forces C to be unsatisfiable.  B has no r-connection and must stay satisfiable.
/// Before the fix the `Some` LHS arm has no `sup = Bot` case ⟹ axiom silently DROPPED.
#[test]
fn some_bot_derives_unsat() {
    // Explicit object-property declaration required for the parser.
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "C ⊑ ∃r.A and ∃r.A ⊑ ⊥ entails C ⊑ ⊥"
    );
}

/// FP GUARD for `∃r.A ⊑ ⊥`: classes with an unrelated existential must stay
/// satisfiable, and the filler class A itself must stay satisfiable.
#[test]
fn some_bot_does_not_over_fire() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))
    SubClassOf(:D ObjectSomeValuesFrom(:r :B))";
    let mut got = unsat_of(body);
    got.sort();
    // Only C is unsat: D has ∃r.B (unrelated filler), A is just a class.
    assert_eq!(
        got,
        vec!["http://t/C".to_string()],
        "D (∃r.B) and A must stay satisfiable; only C (∃r.A) is unsat"
    );
}

// ── Bug 2b-3: `∃r.⊤ ⊑ ⊥` (Some LHS with Top body, sup = Bot) ───────────────

/// BUG REPRODUCER for `∃r.⊤ ⊑ ⊥`.
/// `SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Nothing)` means nothing may
/// have ANY r-successor (role r is completely empty).  Combined with
/// `SubClassOf(:C ObjectSomeValuesFrom(:r :A))` this forces C to be unsatisfiable.
/// Before the fix the `Some` LHS `Top`-body arm returns early before the `sup=Bot`
/// check ⟹ axiom silently DROPPED while the gate certifies the closure complete.
#[test]
fn some_top_bot_derives_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "C ⊑ ∃r.A and ∃r.⊤ ⊑ ⊥ entails C ⊑ ⊥"
    );
}

/// FP GUARD for `∃r.⊤ ⊑ ⊥`: a class with an existential on an UNRELATED role
/// must stay satisfiable, and the filler class A must stay satisfiable.
#[test]
fn some_top_bot_does_not_over_fire() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))
    SubClassOf(:D ObjectSomeValuesFrom(:s :B))";
    let mut got = unsat_of(body);
    got.sort();
    // D has ∃s.B (unrelated role), A and B are just classes — all must stay sat.
    assert_eq!(
        got,
        vec!["http://t/C".to_string()],
        "D (∃s.B, unrelated role) and A and B must stay satisfiable; only C is unsat"
    );
}

// ── Bug 2b-4: `ObjectPropertyDomain(:r owl:Nothing)` ────────────────────────

/// BUG REPRODUCER for `ObjectPropertyDomain(:r owl:Nothing)`.
/// `ObjectPropertyDomain(:r owl:Nothing)` means no individual may be an r-source
/// (identical semantics to `∃r.⊤ ⊑ ⊥`).  Combined with
/// `SubClassOf(:C ObjectSomeValuesFrom(:r :A))` this forces C to be unsatisfiable.
/// Before the fix the domain-collection pass only handles atomic domains, so a
/// `Bot` domain is silently DROPPED while the gate certifies the closure complete.
#[test]
fn domain_bot_derives_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "Domain(r)=⊥ and C ⊑ ∃r.A entails C ⊑ ⊥"
    );
}

/// FP GUARD for `ObjectPropertyDomain(:r owl:Nothing)`: a class with NO
/// r-existential must stay satisfiable; the filler class A must stay satisfiable.
#[test]
fn domain_bot_does_not_over_fire() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))
    SubClassOf(:D ObjectSomeValuesFrom(:s :B))";
    let mut got = unsat_of(body);
    got.sort();
    // D has ∃s.B (role s is not poisoned), A and B are just classes — all sat.
    assert_eq!(
        got,
        vec!["http://t/C".to_string()],
        "D (∃s.B) and A and B must stay satisfiable; only C is unsat"
    );
}

// ── Bug 2b-5: `ObjectPropertyRange(:r owl:Nothing)` ─────────────────────────

/// BUG REPRODUCER for `ObjectPropertyRange(:r owl:Nothing)`.
/// `ObjectPropertyRange(:r owl:Nothing)` means no individual may be an r-target
/// (the r-range is empty ⟹ no r-edge can exist).  Combined with
/// `SubClassOf(:C ObjectSomeValuesFrom(:r :A))` this forces C to be unsatisfiable.
/// Before the fix the range-collection pass only handles atomic ranges, so a
/// `Bot` range is silently DROPPED while the gate certifies the closure complete.
#[test]
fn range_bot_derives_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    ObjectPropertyRange(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "Range(r)=⊥ and C ⊑ ∃r.A entails C ⊑ ⊥"
    );
}

/// FP GUARD for `ObjectPropertyRange(:r owl:Nothing)`: a class with NO
/// r-existential must stay satisfiable; the filler class A must stay satisfiable.
#[test]
fn range_bot_does_not_over_fire() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    ObjectPropertyRange(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))
    SubClassOf(:D ObjectSomeValuesFrom(:s :B))";
    let mut got = unsat_of(body);
    got.sort();
    // D has ∃s.B (role s is not poisoned), A and B are just classes — all sat.
    assert_eq!(
        got,
        vec!["http://t/C".to_string()],
        "D (∃s.B) and A and B must stay satisfiable; only C is unsat"
    );
}

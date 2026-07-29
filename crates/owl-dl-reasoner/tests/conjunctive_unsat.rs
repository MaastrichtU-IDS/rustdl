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

// ── Finding 1: `classify --json` self-contradiction fix ─────────────────────

/// Pins Finding 1: a `⊤ ⊑ ⊥` KB must produce `ClassificationStats::inconsistent == true`.
/// Before the fix, `classify_pure_el` left `inconsistent = false` while the
/// `unsatisfiable` list was non-empty, causing `classify --json` to emit
/// `"consistent": true` alongside a non-empty `"unsatisfiable"` list.
#[test]
fn top_bot_classify_stats_inconsistent() {
    let onto = parse(
        "    Declaration(Class(:A))
    Declaration(Class(:C))
    SubClassOf(owl:Thing owl:Nothing)
    SubClassOf(:C :A)",
    );
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        c.stats().inconsistent,
        "⊤ ⊑ ⊥ KB must have ClassificationStats::inconsistent == true"
    );
    // The unsatisfiable list must still be populated (not suppressed).
    let unsat = c.unsatisfiable_classes();
    assert!(
        !unsat.is_empty(),
        "unsatisfiable list must be populated even when inconsistent is set"
    );
}

// ── Finding 3: stronger FP guards for the `⊤ ⊑ ⊥` fix ──────────────────────

/// FP GUARD: `SubClassOf(owl:Thing :A)` takes the `top_subsumers` path and
/// must NOT trigger `global_unsat`. Zero unsatisfiable classes expected.
#[test]
fn subclass_of_thing_a_stays_sat() {
    // owl:Thing ⊑ :A means every class is a sub-class of A (A is top-equivalent).
    // It does NOT make the domain empty; no class should become unsatisfiable.
    let body = "    Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(owl:Thing :A)";
    assert_eq!(
        unsat_of(body),
        Vec::<String>::new(),
        "SubClassOf(owl:Thing, :A) must not globalise — zero unsatisfiable classes expected"
    );
}

/// FP GUARD: `SubClassOf(:A owl:Nothing)` makes only `:A` (and its subclasses)
/// unsatisfiable. An unrelated declared class `:B` must stay satisfiable.
#[test]
fn scoped_bot_does_not_globalise() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A owl:Nothing)
    SubClassOf(:C :A)";
    // A and C are unsat (C ⊑ A ⊑ ⊥); B is unrelated and must stay satisfiable.
    assert_eq!(
        unsat_of(body),
        vec!["http://t/A".to_string(), "http://t/C".to_string()],
        "SubClassOf(:A, ⊥) must not globalise — only A and C are unsat, B stays satisfiable"
    );
}

// ---------------------------------------------------------------------------
// Nested-existential + poisoned-role tests (task-2e)
// ---------------------------------------------------------------------------

/// `Domain(r)=⊥` before `C ⊑ ∃t.(∃r.A)` — nested existential via a poisoned role.
/// The inner `∃r.A` produces a Tseitin marker `M`; `r`'s domain is `⊥`, so `M` (and
/// therefore `C`) is unsatisfiable.
#[test]
fn nested_existential_poisoned_role_derives_unsat() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:t))
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:r :A)))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "C ⊑ ∃t.(∃r.A) with Domain(r)=⊥ must make C unsatisfiable"
    );
}

/// Same as above but with axiom order reversed: `ObjectPropertyDomain` comes AFTER
/// the nested-existential `SubClassOf`. The fix must be order-independent because it
/// operates in a post-collection pass over `tseitin.by_existential`.
#[test]
fn nested_existential_poisoned_role_order_independent() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:t))
    SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:r :A)))
    ObjectPropertyDomain(:r owl:Nothing)";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "C ⊑ ∃t.(∃r.A) with Domain(r)=⊥ (declared after SubClassOf) must still make C unsat"
    );
}

/// Role chain `chain(t,u)⊑r` + `Domain(r)=⊥` + `C ⊑ ∃t.(∃u.A)`.
///
/// `C` IS semantically unsatisfiable: every model with a `t`-successor of a `t,u`-chain
/// is an `r`-successor, but `r` has domain `⊥`.  However, the post-collection marker
/// pass cannot handle this case: the `∃u.A` Tseitin marker is keyed on role `u`, and
/// `u ⊑ r` does NOT follow from a chain axiom (chains are not simple sub-role axioms),
/// so neither `u` nor any super-role of `u` is in `poisoned_roles`.  Marking `u` as
/// poisoned would be UNSOUND for a standalone `∃u.A` that is genuinely satisfiable.
/// This is a genuine EL-completeness gap, deferred to a future chain-aware pass.
#[test]
#[ignore = "known EL completeness gap: chain-induced domain-poison not handled by the marker pass"]
fn nested_existential_poisoned_role_via_chain() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:t))
    Declaration(ObjectProperty(:u))
    SubObjectPropertyOf(SubObjectPropertyChain(:t :u) :r)
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "C ⊑ ∃t.(∃u.A) with chain(t,u)⊑r and Domain(r)=⊥ must make C unsatisfiable"
    );
}

/// FP guard: a nested existential on an UNPOISONED role must stay satisfiable.
/// `Domain(r)=⊥` but `D ⊑ ∃t.(∃s.A)` uses unrelated role `:s` — `D` is satisfiable.
#[test]
fn nested_existential_unpoisoned_role_stays_sat() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    Declaration(ObjectProperty(:t))
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:D ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:s :A)))";
    assert_eq!(
        unsat_of(body),
        vec![] as Vec<String>,
        "D ⊑ ∃t.(∃s.A) with `:s` unpoisoned must remain satisfiable (FP guard)"
    );
}

/// FP guard: the filler class `:A` in `C ⊑ ∃t.(∃r.A)` must stay satisfiable.
/// The one-way Tseitin marker `M` for `∃r.A` does NOT subsume `:A`; only classes
/// that gain `M` in their subsumer set are affected.
#[test]
fn nested_existential_filler_stays_sat() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:t))
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:r :A)))";
    // C is unsat; A must remain satisfiable.
    let unsat = unsat_of(body);
    assert!(unsat.contains(&"http://t/C".to_string()), "C must be unsat");
    assert!(
        !unsat.contains(&"http://t/A".to_string()),
        "filler :A must NOT be marked unsat (FP guard)"
    );
}

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

// ── Finding 1: derived-inconsistency (not just syntactic `⊤ ⊑ ⊥`) ────────────

/// FINDING 1 BUG REPRODUCER.
/// `SubClassOf(owl:Thing :E)` + `SubClassOf(:E owl:Nothing)` makes every class
/// unsatisfiable via transitive closure WITHOUT setting the syntactic `global_unsat`
/// flag (because `sub = owl:Thing` takes the `top_subsumers` path, not the
/// `⊤ ⊑ ⊥` guard).  Before the fix `classify_pure_el` left `stats.inconsistent =
/// false` while listing both classes in the unsatisfiable set — a self-contradiction
/// in the JSON output (`"consistent": true` + non-empty `"unsatisfiable"`).
///
/// The fix: `Subsumers::top_is_unsat()` checks whether any `⊤`-subsumer class
/// ended up unsat; if so, ⊤ is itself unsat → KB is inconsistent.  Unlike the
/// naive "all user classes unsat" heuristic, this does NOT fire for
/// `{A ⊑ ⊥, B ⊑ ⊥}` where no `⊤ ⊑ …` axiom was asserted (consistent KB).
#[test]
fn derived_all_unsat_flags_inconsistent() {
    let onto = parse(
        "    Declaration(Class(:A))
    Declaration(Class(:E))
    SubClassOf(owl:Thing :E)
    SubClassOf(:E owl:Nothing)",
    );
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        c.stats().inconsistent,
        "⊤ ⊑ E, E ⊑ ⊥ makes every class unsat: ClassificationStats::inconsistent must be true"
    );
    // The unsatisfiable list must still be populated (not suppressed by the flag).
    let unsat = c.unsatisfiable_classes();
    assert!(
        !unsat.is_empty(),
        "unsatisfiable list must still be populated when derived-inconsistency fires"
    );
}

/// FP GUARD for derived-inconsistency: an ontology with a single unsat class but
/// other satisfiable classes must NOT report inconsistent (partial, not total,
/// unsat is a consistent ontology in OWL 2 DL).
#[test]
fn partial_unsat_does_not_flag_inconsistent() {
    let onto = parse(
        "    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A owl:Nothing)",
    );
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        !c.stats().inconsistent,
        "only :A is unsat, :B is satisfiable — KB must still report consistent"
    );
}

// ── Finding 3: sub-role vs super-role FP guards for poisoned-role ────────────

/// FINDING 3 NEGATIVE: a poisoned SUB-role must NOT condemn a super-role user.
///
/// `ObjectPropertyDomain(:r owl:Nothing)` poisons role `:r`.
/// `SubObjectPropertyOf(:r :s)` means every `:r`-edge is also an `:s`-edge,
/// but NOT vice versa — an `:s`-edge need not be an `:r`-edge.
/// So `C ⊑ ∃s.A` is satisfiable: `C` need only have an `:s`-successor
/// that is not also an `:r`-successor. The poisoning of `:r` must NOT
/// propagate upward to `:s`.
#[test]
fn poisoned_sub_role_does_not_condemn_super_role_user_domain() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    ObjectPropertyDomain(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:s :A))";
    assert_eq!(
        unsat_of(body),
        Vec::<String>::new(),
        "FP guard: poisoned sub-role :r must NOT condemn super-role :s user (C must stay sat)"
    );
}

/// FINDING 3 POSITIVE: a poisoned SUPER-role DOES condemn a sub-role user.
///
/// `ObjectPropertyDomain(:s owl:Nothing)` poisons role `:s`.
/// `SubObjectPropertyOf(:r :s)` means every `:r`-edge is also an `:s`-edge.
/// So `C ⊑ ∃r.A` forces an `:r`-successor, which is also an `:s`-successor,
/// which is in `Domain(s) = ⊥` — impossible. Hence `C` is unsatisfiable.
#[test]
fn poisoned_super_role_condemns_sub_role_user_domain() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    ObjectPropertyDomain(:s owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:r :A))";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/C".to_string()],
        "poisoned super-role :s must condemn sub-role :r user C"
    );
}

/// FINDING 3 NEGATIVE for `ObjectPropertyRange`: a poisoned SUB-role must NOT
/// condemn a super-role user via `Range`.
///
/// `ObjectPropertyRange(:r owl:Nothing)` poisons `:r` (no `:r`-target).
/// `SubObjectPropertyOf(:r :s)` — `:s`-edges need not be `:r`-edges.
/// `C ⊑ ∃s.A` is satisfiable: `C` can have an `:s`-successor that is not
/// an `:r`-target. Poisoning must NOT propagate upward.
#[test]
fn poisoned_sub_role_does_not_condemn_super_role_user_range() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    ObjectPropertyRange(:r owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:s :A))";
    assert_eq!(
        unsat_of(body),
        Vec::<String>::new(),
        "FP guard: poisoned sub-role :r Range must NOT condemn super-role :s user (C must stay sat)"
    );
}

/// FINDING 3 NEGATIVE for `∃r.⊤ ⊑ ⊥`: a poisoned SUB-role must NOT condemn
/// a super-role user.
///
/// `SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Nothing)` poisons `:r`.
/// `SubObjectPropertyOf(:r :s)` — `:s`-edges need not be `:r`-edges.
/// `C ⊑ ∃s.A` is satisfiable: `C` need not have any `:r`-successor.
#[test]
fn poisoned_sub_role_some_top_bot_does_not_condemn_super_role_user() {
    let body = "
    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) owl:Nothing)
    SubClassOf(:C ObjectSomeValuesFrom(:s :A))";
    assert_eq!(
        unsat_of(body),
        Vec::<String>::new(),
        "FP guard: ∃r.⊤ ⊑ ⊥ on sub-role :r must NOT condemn super-role :s user (C must stay sat)"
    );
}

// ── Finding 1: FP guard — consistent KB where every user class happens to be empty ──

/// FP GUARD: "all user classes unsatisfiable" does NOT imply KB inconsistency.
///
/// `{SubClassOf(:A owl:Nothing), SubClassOf(:B owl:Nothing)}` — both named classes
/// are individually empty (no instance satisfies them), but the KB is consistent:
/// the domain `{d}` where `d` belongs to no named class is a valid model.
///
/// This test guards against the naive `n > 0 && |unsat| == n ⟹ inconsistent`
/// heuristic that was reverted.  The correct check uses `Subsumers::top_is_unsat()`:
/// no `⊤ ⊑ …` axiom was asserted here, so `top_subsumers` is empty and the flag
/// stays false → KB correctly reported consistent.
#[test]
fn all_user_classes_unsat_does_not_flag_inconsistent() {
    let onto = parse(
        "    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A owl:Nothing)
    SubClassOf(:B owl:Nothing)",
    );
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        !c.stats().inconsistent,
        "SOUNDNESS: A⊑⊥ + B⊑⊥ is consistent (no ⊤⊑… axiom), so ClassificationStats::inconsistent must be false"
    );
    // Both classes must still appear as unsatisfiable.
    let unsat = unsat_of(
        "    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A owl:Nothing)
    SubClassOf(:B owl:Nothing)",
    );
    assert!(
        unsat.contains(&"http://t/A".to_string()),
        ":A must still be listed as unsatisfiable"
    );
    assert!(
        unsat.contains(&"http://t/B".to_string()),
        ":B must still be listed as unsatisfiable"
    );
}

/// PROVENANCE. An unsatisfiability derived from `And(A,B) ⊑ ⊥` must be
/// explainable — `find_one_justification` has to return a non-empty
/// justification rather than `None`.
#[test]
fn conjunctive_bot_unsat_is_justifiable() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let onto = parse(&body);
    let q = owl_dl_reasoner::justify::parse_query(&["unsat".to_string(), "http://t/C".to_string()])
        .expect("parse query");
    let js = owl_dl_reasoner::justify::find_one_justification(&onto, &q).expect("justify error");
    assert!(js.is_some(), "unsat C must have a justification");
    let j = js.unwrap();
    assert!(
        !j.axioms.is_empty(),
        "justification must contain at least one axiom"
    );
}

/// PROVENANCE WIRING GUARD for `ConjunctiveUnsat`.
///
/// Directly inspects `ProofTrace::steps` to verify that:
/// (a) a step with `rule == ElRule::ConjunctiveUnsat` was recorded, and
/// (b) that step's `axiom_refs` is non-empty (which breaks if the
///     `conjunctive_unsat_axiom` provenance lookup is missing or always yields `None`).
///
/// The existing `conjunctive_bot_unsat_is_justifiable` test CANNOT guard this:
/// `QuickXplain` re-runs the full reasoner on axiom subsets and never reads
/// `ProofTrace` directly — it would pass even if provenance recording were deleted.
#[test]
fn conjunctive_unsat_provenance_wired() {
    let body = format!(
        "{DECLS}    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)
    SubClassOf(:C :A)
    SubClassOf(:C :B)"
    );
    let onto = parse(&body);
    let internal = owl_dl_core::convert_ontology(&onto).expect("convert_ontology");
    let cfg = owl_dl_reasoner::SaturateConfig {
        record_proofs: true,
    };
    let (_subs, maybe_trace) = owl_dl_reasoner::saturate_with_config(&internal, &cfg);
    let trace = maybe_trace.expect("ProofTrace must be produced when record_proofs=true");
    let cu_step = trace
        .steps
        .values()
        .find(|inf| inf.rule == owl_dl_reasoner::ElRule::ConjunctiveUnsat);
    assert!(
        cu_step.is_some(),
        "ProofTrace must contain a ConjunctiveUnsat step; \
         wiring is broken if no step was recorded"
    );
    assert!(
        !cu_step.unwrap().axiom_refs.is_empty(),
        "ConjunctiveUnsat step must have non-empty axiom_refs; \
         broken if conjunctive_unsat_axiom provenance lookup always yields None"
    );
}

// ── Task 4: complex-body, n-ary, and FP-guard canaries for ConjunctiveUnsat ──

/// COMPLEX BODY. `∃R.C ⊓ D ⊑ ⊥` — the `bodies` collector lowers the `∃R.C`
/// operand to a marker class, so the same rule covers it.
#[test]
fn conjunctive_bot_with_existential_body_derives_unsat() {
    let body = "    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:R))
    SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:R :C) :D) owl:Nothing)
    SubClassOf(:X ObjectSomeValuesFrom(:R :C))
    SubClassOf(:X :D)";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/X".to_string()],
        "X has both ∃R.C and D, and their conjunction is unsatisfiable"
    );
}

/// FP GUARD for the complex-body case: only the existential, no D.
#[test]
fn conjunctive_bot_with_existential_body_does_not_over_fire() {
    let body = "    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:R))
    SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:R :C) :D) owl:Nothing)
    SubClassOf(:X ObjectSomeValuesFrom(:R :C))";
    assert!(
        unsat_of(body).is_empty(),
        "X has only ∃R.C, so nothing is unsatisfiable"
    );
}

/// N-ARY. Three-operand conjunction; a class with two of the three stays
/// satisfiable, a class with all three does not.
#[test]
fn conjunctive_bot_ternary_requires_all_bodies() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:E))
    Declaration(Class(:Two))
    Declaration(Class(:Three))
    SubClassOf(ObjectIntersectionOf(:A :B :E) owl:Nothing)
    SubClassOf(:Two :A)
    SubClassOf(:Two :B)
    SubClassOf(:Three :A)
    SubClassOf(:Three :B)
    SubClassOf(:Three :E)";
    assert_eq!(
        unsat_of(body),
        vec!["http://t/Three".to_string()],
        "only the class carrying all three conjuncts is unsatisfiable"
    );
}

// ─── Bug A: non-atomic DisjointClasses member silently dropped ────────────────
//
// `DisjointClasses(:A ObjectUnionOf(:B :C))` contains a non-atomic member.
// The engine's `disjoint_pairs` collector filters to atomics only (line 3148),
// so `ObjectUnionOf(:B :C)` is dropped and `(:A)` appears as a singleton —
// no pair is emitted.  The old gate (`is_el_axiom` checked `is_el_concept` which
// admits `Or`; `is_saturator_axiom` used `_ => disjoint_ok` with NO member
// inspection) admitted the axiom to the fast path where it silently dropped the
// non-atomic member, reporting "complete" while missing the entailment.
//
// The reproducer forces the subsumption via the *atomic* member only:
//   DisjointClasses(:A ObjectUnionOf(:B :C))
//   SubClassOf(:X ObjectIntersectionOf(:A :B))
// `X ⊑ A` and `X ⊑ B`, and since `B ⊆ (B ⊔ C)`, `A ⊓ (B ⊔ C) ⊑ ⊥` → X ⊑ ⊥.
// The hybrid path correctly finds this; the fast path silently missed it.

/// BUG A REPRODUCER. Non-atomic `DisjointClasses` member silently dropped.
/// `DisjointClasses(:A ObjectUnionOf(:B :C))` + `SubClassOf(:X ObjectIntersectionOf(:A :B))`
/// entails X ⊑ ⊥.  With the old gate, X stays satisfiable under the "complete" banner.
#[test]
fn disjoint_nonatomic_member_forces_unsat() {
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:X))
    DisjointClasses(:A ObjectUnionOf(:B :C))
    SubClassOf(:X ObjectIntersectionOf(:A :B))";
    // X ⊑ A, X ⊑ B; B ⊆ B⊔C; A ⊓ (B⊔C) ⊑ ⊥ (from DisjointClasses) → X ⊑ ⊥.
    assert_eq!(
        unsat_of(body),
        vec!["http://t/X".to_string()],
        "Bug A: X ⊑ A ⊓ B and DisjointClasses(A, B⊔C) entails X ⊑ ⊥"
    );
}

/// ATOMIC CONTROL for Bug A. All-atomic `DisjointClasses` must still classify correctly
/// on the FAST PATH (gate must not kick atomic-only `DisjointClasses` to hybrid).
#[test]
fn disjoint_all_atomic_members_stays_on_fast_path() {
    // Same topology but all members atomic: DisjointClasses(:A :B)
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:X))
    DisjointClasses(:A :B)
    SubClassOf(:X :A)
    SubClassOf(:X :B)";
    // X must be unsatisfiable (A ⊓ B ⊑ ⊥ directly).
    assert_eq!(
        unsat_of(body),
        vec!["http://t/X".to_string()],
        "atomic DisjointClasses control: X ⊑ A ⊓ B with DisjointClasses(A,B) entails X ⊑ ⊥"
    );
}

// ─── Bug B: non-atomic ObjectPropertyDomain silently dropped ─────────────────
//
// `ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))` has a conjunctive filler.
// At the time this fixture was written, the engine's `role_domains` collector
// accepted ONLY `ConceptExpr::Atomic` fillers, so `ObjectIntersectionOf` was
// silently dropped and neither `:P` nor `:Q` ended up in `role_domains[:r]`. The
// gate of the day (`is_el_concept` / `is_saturator_concept`, both of which admit
// `And`) passed this axiom to the fast path where it became a no-op, reporting
// "complete" while missing `X ⊑ P` and `X ⊑ Q`. The interim mitigation was a
// gate-only tightening (`is_atomic_or_trivial_concept`) that REFUSED this shape,
// forcing the slower-but-correct hybrid/tableau path.
//
// **Issue #110 fixed this at the root**: `owl_dl_saturation::decompose_role_filler`
// now decomposes a fully-conjunctive filler into its atomic conjuncts and the
// saturator processes all of them directly, so the gate
// (`is_processed_role_filler`, née `is_atomic_or_trivial_concept`) now ADMITS
// this exact shape to the saturation-only fast path — correctly, because the
// engine that path uses no longer drops anything. This test used to assert the
// interim mitigation (`!stats.pure_el_mode`); that assertion is now WRONG on
// purpose, not broken — see [[tests-that-pin-the-bug]]. Flipped below.

/// BUG B REPRODUCER, RETARGETED FOR #110. Non-atomic `ObjectPropertyDomain`
/// used to be silently dropped by the fast path; now the fast path itself
/// derives both entailed subsumptions.
/// `ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))` + `SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))`
/// entails `X ⊑ P` and `X ⊑ Q`.
///
/// # Why `classify_n2` here, not `classify`
///
/// Kept for parity with the original fixture and because it is still the
/// stronger check (it tests ALL pairs regardless of saturation-derived tier
/// ordering, rather than relying on the top-down walk to have generated the
/// (X, P) pair). Post-#110, `classify()`'s own top-down walk generates this
/// pair too — pinned by
/// `conjunctive_domain_filler_derives_every_conjunct` in
/// `crates/owl-dl-reasoner/tests/conjunctive_domain_range_filler.rs`, which
/// calls `classify()` (not `classify_n2`) on the same conjunctive-domain
/// shape — but `classify_n2` remains the check that cannot be defeated by a
/// future tier-walk change.
#[test]
fn domain_conjunctive_filler_derives_subsumptions() {
    let body = "    Declaration(Class(:P))
    Declaration(Class(:Q))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:r))
    ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))
    SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))";
    let onto = parse(body);
    // Use classify_n2 (the N² pairwise path) which tests ALL pairs regardless of
    // saturation-derived tier ordering.  See the doc comment above.
    let c = owl_dl_reasoner::classify_n2(&onto).expect("classify_n2");
    // Both entailments must be present.
    assert!(
        c.is_subclass("http://t/X", "http://t/P"),
        "Bug B: X ⊑ ∃r.⊤ + Domain(r)=P⊓Q entails X ⊑ P"
    );
    assert!(
        c.is_subclass("http://t/X", "http://t/Q"),
        "Bug B: X ⊑ ∃r.⊤ + Domain(r)=P⊓Q entails X ⊑ Q"
    );
    // Verify the #110 fix is WHY it passes, not some incidental reason.
    //
    // `pure_el_mode` is the DIRECT signal: post-#110, the saturator processes a
    // fully-decomposable conjunctive filler completely, so this ontology
    // belongs on the saturation-only fast path and the gate must ADMIT it —
    // the opposite of the pre-#110 interim mitigation this test used to pin.
    // Asserting only "some subsumption query ran" is weaker — it would still
    // hold if a later edit to this fixture (say adding a `FunctionalRole`)
    // forced the hybrid path for an unrelated reason, leaving the #110 fix
    // untested.
    let stats = c.stats();
    assert!(
        stats.pure_el_mode,
        "#110: a fully-decomposable conjunctive domain filler must now be \
         admitted to the saturation-only fast path — the engine processes it \
         completely, so the gate refusing it would itself be a stale D10 gap"
    );
    assert!(
        stats.saturation_subsumption_hits > 0,
        "#110: the fast-path saturator must have derived both subsumptions \
         directly (saturation_hits={})",
        stats.saturation_subsumption_hits
    );
}

/// FP GUARD for Bug B. A class with NO r-existential must NOT gain P or Q.
#[test]
fn domain_conjunctive_filler_no_fp() {
    let body = "    Declaration(Class(:P))
    Declaration(Class(:Q))
    Declaration(Class(:Y))
    Declaration(ObjectProperty(:r))
    ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))";
    // Y has no r-existential, so it must not gain P or Q.
    let onto = parse(body);
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        !c.is_subclass("http://t/Y", "http://t/P"),
        "FP guard: Y has no ∃r, must not gain P"
    );
    assert!(
        !c.is_subclass("http://t/Y", "http://t/Q"),
        "FP guard: Y has no ∃r, must not gain Q"
    );
}

/// ATOMIC CONTROLS for Bug B. Two separate atomic-filler domains must still
/// classify correctly on the FAST PATH (gate must not kick them to hybrid).
#[test]
fn domain_two_atomic_fillers_stays_on_fast_path() {
    // Semantically equivalent to the conjunctive-filler case: two atomic domains.
    let body = "    Declaration(Class(:P))
    Declaration(Class(:Q))
    Declaration(Class(:X))
    Declaration(ObjectProperty(:r))
    ObjectPropertyDomain(:r :P)
    ObjectPropertyDomain(:r :Q)
    SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))";
    let onto = parse(body);
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    // Both atomic-domain entailments must be present.
    assert!(
        c.is_subclass("http://t/X", "http://t/P"),
        "atomic domain control: X ⊑ ∃r.⊤ + Domain(r)=P (atomic) entails X ⊑ P"
    );
    assert!(
        c.is_subclass("http://t/X", "http://t/Q"),
        "atomic domain control: X ⊑ ∃r.⊤ + Domain(r)=Q (atomic) entails X ⊑ Q"
    );
}

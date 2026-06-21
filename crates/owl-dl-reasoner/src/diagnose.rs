//! Root/derived unsatisfiability diagnosis: partition the classified
//! unsatisfiable classes into *root* causes and *derived* collateral, using a
//! stingy structural dependency graph (edges only for unsat-forcing positions).
//! Read-only over classification — adds no entailments, so FP=0 is untouched.

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{ClassExpression, Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;

/// A derived unsatisfiable class and the root cause(s) it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedClass {
    /// IRI of the derived (collateral) unsatisfiable class.
    pub iri: String,
    /// IRIs of the root class(es) it transitively depends on.
    pub roots: Vec<String>,
}

/// The result of diagnosing an ontology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// Whether the ontology is consistent. When `false`, `roots`/`derived` are
    /// empty and the caller should justify the inconsistency directly.
    pub consistent: bool,
    /// Root unsatisfiable classes (IRIs), sorted. Empty if consistent and coherent.
    pub roots: Vec<String>,
    /// Derived unsatisfiable classes, each with its root(s), sorted by IRI.
    pub derived: Vec<DerivedClass>,
    /// Every unsatisfiable class (roots ∪ derived), sorted — the conservation set.
    pub all_unsat: Vec<String>,
    /// For each root IRI, the derived classes that depend on it, sorted.
    pub root_derives: BTreeMap<String, Vec<String>>,
}

/// Diagnose `onto`: consistency, then the root/derived unsatisfiability partition.
///
/// Read-only over classification; never mutates the ontology.
pub fn diagnose<A: ForIRI>(onto: &SetOntology<A>) -> Result<Diagnosis, ReasonError> {
    // Filled in by Task 4.
    let _ = onto;
    Ok(Diagnosis {
        consistent: true,
        roots: Vec::new(),
        derived: Vec::new(),
        all_unsat: Vec::new(),
        root_derives: BTreeMap::new(),
    })
}

/// Collect the named classes that occur in *unsat-forcing* positions of `ce`.
///
/// Forcing positions (a `⊥` here forces the whole expression to `⊥`):
/// - a named class at top level (`C ⊑ D`),
/// - every conjunct of an intersection (`C ⊑ D ⊓ …`),
/// - the filler of an existential (`C ⊑ ∃r.D`, since `∃r.⊥ ≡ ⊥`),
/// - recursively, the same inside nested `⊓`/`∃`.
///
/// Every other constructor (`∀`, `⊔`, `¬`, cardinality, `hasValue`, data) is
/// deliberately NOT recursed: its filler being `⊥` does not force the parent
/// `⊥`. Omitting such an edge only ever over-reports roots (safe); including a
/// spurious one would hide a root (the dangerous failure we avoid).
#[allow(dead_code)] // wired into diagnose() in Task 4; allow removed there
fn unsat_forcing_classes<A: ForIRI>(ce: &ClassExpression<A>, out: &mut BTreeSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectIntersectionOf(cs) => {
            for c in cs {
                unsat_forcing_classes(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { bce, .. } => {
            unsat_forcing_classes(bce, out);
        }
        // All other constructors are not unsat-forcing — stop.
        _ => {}
    }
}

/// Build the dependency graph over the unsatisfiable set `unsat`.
///
/// `edges[C]` = the set of unsatisfiable classes `D ≠ C` such that `C`'s
/// unsatisfiability is *forced* by `D`'s (an edge `C → D`). Only `SubClassOf`
/// (named-class LHS) and `EquivalentClasses` axioms contribute; only targets in
/// `unsat` are kept; self-edges are dropped.
#[allow(dead_code)] // wired into diagnose() in Task 4; allow removed there
fn build_dep_edges<A: ForIRI>(
    unsat: &BTreeSet<String>,
    logical: &[Component<A>],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut add = |src: &str, targets: BTreeSet<String>| {
        if !unsat.contains(src) {
            return;
        }
        for t in targets {
            if t != src && unsat.contains(&t) {
                edges.entry(src.to_string()).or_default().insert(t);
            }
        }
    };

    for c in logical {
        match c {
            Component::SubClassOf(a) => {
                if let ClassExpression::Class(sub) = &a.sub {
                    let mut forced = BTreeSet::new();
                    unsat_forcing_classes(&a.sup, &mut forced);
                    add(sub.0.as_ref(), forced);
                }
            }
            Component::EquivalentClasses(a) => {
                // Each named-class member C ⊑ every other member: edges from C to
                // the forcing classes of every other member.
                for (i, member) in a.0.iter().enumerate() {
                    let ClassExpression::Class(c) = member else {
                        continue;
                    };
                    let mut forced = BTreeSet::new();
                    for (j, other) in a.0.iter().enumerate() {
                        if i != j {
                            unsat_forcing_classes(other, &mut forced);
                        }
                    }
                    add(c.0.as_ref(), forced);
                }
            }
            _ => {}
        }
    }

    edges
}

#[cfg(test)]
mod edge_tests {
    use super::*;
    use horned_owl::model::Build;
    use horned_owl::model::MutableOntology;
    use horned_owl::ontology::set::SetOntology;

    type B = Build<std::rc::Rc<str>>;

    fn b() -> B {
        Build::new_rc()
    }

    fn class(b: &B, iri: &str) -> ClassExpression<std::rc::Rc<str>> {
        ClassExpression::Class(b.class(iri))
    }

    // C SubClassOf D  (told subsumption) → edge C->D
    #[test]
    fn told_subsumption_is_edge() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup: class(&b, "urn:D"),
        });
        let unsat: BTreeSet<String> = ["urn:C", "urn:D"].iter().map(ToString::to_string).collect();
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
    }

    // C SubClassOf exists r.D → edge C->D
    #[test]
    fn existential_filler_is_edge() {
        let b = b();
        let sup = ClassExpression::ObjectSomeValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(class(&b, "urn:D")),
        };
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup,
        });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
    }

    // C SubClassOf (D and E) → edges C->D, C->E (both unsat)
    #[test]
    fn conjunct_is_edge() {
        let b = b();
        let sup =
            ClassExpression::ObjectIntersectionOf(vec![class(&b, "urn:D"), class(&b, "urn:E")]);
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup,
        });
        let unsat = set(&["urn:C", "urn:D", "urn:E"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D", "urn:E"])));
    }

    // NEGATIVE: C SubClassOf forall r.D → NO edge (∀ does not force C unsat)
    #[test]
    fn forall_is_not_edge() {
        let b = b();
        let sup = ClassExpression::ObjectAllValuesFrom {
            ope: b.object_property("urn:r").into(),
            bce: Box::new(class(&b, "urn:D")),
        };
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup,
        });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), None);
    }

    // NEGATIVE: DisjointClasses(C, D) → NO edge
    #[test]
    fn disjoint_is_not_edge() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::DisjointClasses(vec![
            class(&b, "urn:C"),
            class(&b, "urn:D"),
        ]));
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert!(edges.is_empty());
    }

    // NEGATIVE: target D not unsat → NO edge (only edges to unsat classes count)
    #[test]
    fn edge_target_must_be_unsat() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup: class(&b, "urn:D"),
        });
        let unsat = set(&["urn:C"]); // D is satisfiable
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert!(!edges.contains_key("urn:C"));
    }

    // NEGATIVE: self-edge suppressed (C SubClassOf (C and D))
    #[test]
    fn self_edge_suppressed() {
        let b = b();
        let sup =
            ClassExpression::ObjectIntersectionOf(vec![class(&b, "urn:C"), class(&b, "urn:D")]);
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::SubClassOf {
            sub: class(&b, "urn:C"),
            sup,
        });
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"]))); // C->C dropped
    }

    // EquivalentClasses(C, D) where both unsat → edge C->D and D->C
    #[test]
    fn equivalent_classes_both_directions() {
        let b = b();
        let mut o = SetOntology::new();
        o.insert(horned_owl::model::EquivalentClasses(vec![
            class(&b, "urn:C"),
            class(&b, "urn:D"),
        ]));
        let unsat = set(&["urn:C", "urn:D"]);
        let edges = build_dep_edges(&unsat, &logical(&o));
        assert_eq!(edges.get("urn:C"), Some(&set(&["urn:D"])));
        assert_eq!(edges.get("urn:D"), Some(&set(&["urn:C"])));
    }

    // helpers
    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(ToString::to_string).collect()
    }
    fn logical(o: &SetOntology<std::rc::Rc<str>>) -> Vec<Component<std::rc::Rc<str>>> {
        crate::justify::logical_axioms(o).1
    }
}

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

/// Forward-reachable set of `start` (including `start`) over `edges`.
#[allow(dead_code)] // wired into diagnose() in Task 4; allow removed there
fn reachable(start: &str, edges: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(n) = stack.pop() {
        if seen.insert(n.clone())
            && let Some(succ) = edges.get(&n)
        {
            for s in succ {
                if !seen.contains(s) {
                    stack.push(s.clone());
                }
            }
        }
    }
    seen
}

/// Partition `unsat` into roots and derived using the dependency `edges`.
///
/// A class `C` is a **root** iff every node reachable from `C` reaches `C` back
/// (i.e. `C`'s reachable set is contained in its strongly-connected component —
/// `C` is a sink-SCC member, depending on nothing strictly more root). Cycle
/// members with no outside dependency are therefore all roots.
#[allow(dead_code)] // wired into diagnose() in Task 4; allow removed there
fn partition(unsat: &BTreeSet<String>, edges: &BTreeMap<String, BTreeSet<String>>) -> Diagnosis {
    // Precompute forward reachability for every unsat node.
    let reach: BTreeMap<String, BTreeSet<String>> = unsat
        .iter()
        .map(|c| (c.clone(), reachable(c, edges)))
        .collect();

    let is_root = |c: &str| -> bool {
        let rc = &reach[c];
        rc.iter().all(|d| reach[d].contains(c))
    };

    let mut roots: Vec<String> = Vec::new();
    let mut derived: Vec<DerivedClass> = Vec::new();

    for c in unsat {
        if is_root(c) {
            roots.push(c.clone());
        } else {
            // Roots this derived class depends on = reachable roots (excluding self).
            let mut dr: Vec<String> = reach[c]
                .iter()
                .filter(|d| d.as_str() != c.as_str() && is_root(d))
                .cloned()
                .collect();
            dr.sort();
            derived.push(DerivedClass {
                iri: c.clone(),
                roots: dr,
            });
        }
    }

    roots.sort();
    derived.sort_by(|a, b| a.iri.cmp(&b.iri));

    // Invert: for each root, the derived classes that reach it.
    let mut root_derives: BTreeMap<String, Vec<String>> =
        roots.iter().map(|r| (r.clone(), Vec::new())).collect();
    for d in &derived {
        for r in &d.roots {
            root_derives
                .entry(r.clone())
                .or_default()
                .push(d.iri.clone());
        }
    }
    for v in root_derives.values_mut() {
        v.sort();
    }

    let all_unsat: Vec<String> = unsat.iter().cloned().collect();

    Diagnosis {
        consistent: true,
        roots,
        derived,
        all_unsat,
        root_derives,
    }
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

#[cfg(test)]
mod partition_tests {
    use super::*;

    fn edges(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        pairs
            .iter()
            .map(|(s, ts)| (s.to_string(), ts.iter().map(ToString::to_string).collect()))
            .collect()
    }
    fn unsat(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(ToString::to_string).collect()
    }

    // Chain C->D->E: E is the only root; C,D derived ⇐ E.
    #[test]
    fn chain_has_single_root() {
        let u = unsat(&["C", "D", "E"]);
        let g = edges(&[("C", &["D"]), ("D", &["E"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["E".to_string()]);
        let derived: BTreeMap<_, _> = p
            .derived
            .iter()
            .map(|d| (d.iri.clone(), d.roots.clone()))
            .collect();
        assert_eq!(derived["C"], vec!["E".to_string()]);
        assert_eq!(derived["D"], vec!["E".to_string()]);
    }

    // Independent roots: A and B both depend on nobody.
    #[test]
    fn independent_roots() {
        let u = unsat(&["A", "B"]);
        let g = edges(&[]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["A".to_string(), "B".to_string()]);
        assert!(p.derived.is_empty());
    }

    // Cycle C<->D with no outside edge: both are co-roots.
    #[test]
    fn cycle_members_are_co_roots() {
        let u = unsat(&["C", "D"]);
        let g = edges(&[("C", &["D"]), ("D", &["C"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["C".to_string(), "D".to_string()]);
        assert!(p.derived.is_empty());
    }

    // Cycle C<->D that both depend on root E: C,D derived ⇐ E; E root.
    #[test]
    fn cycle_depending_on_root() {
        let u = unsat(&["C", "D", "E"]);
        let g = edges(&[("C", &["D", "E"]), ("D", &["C"])]);
        let p = partition(&u, &g);
        assert_eq!(p.roots, vec!["E".to_string()]);
        let iris: Vec<String> = p.derived.iter().map(|d| d.iri.clone()).collect();
        assert_eq!(iris, vec!["C".to_string(), "D".to_string()]);
    }

    // Conservation: roots ∪ derived == all_unsat, always.
    #[test]
    fn conservation_holds() {
        let u = unsat(&["A", "B", "C", "D"]);
        let g = edges(&[("A", &["B"]), ("C", &["D"])]);
        let p = partition(&u, &g);
        let mut union: BTreeSet<String> = p.roots.iter().cloned().collect();
        union.extend(p.derived.iter().map(|d| d.iri.clone()));
        assert_eq!(union, u);
    }

    // root_derives: root B is depended on by A.
    #[test]
    fn root_derives_populated() {
        let u = unsat(&["A", "B"]);
        let g = edges(&[("A", &["B"])]);
        let p = partition(&u, &g);
        assert_eq!(p.root_derives.get("B"), Some(&vec!["A".to_string()]));
    }
}

//! #600: `RUSTDL_CLAUSIFY_COMPLEX_DISJOINT` is opt-in. With it on, complex members
//! clausify with nothing deferred, up to the trigger-less cap; off (the default),
//! the old deferrals stay; on atomic members the flag changes nothing.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::{clausify_with_stats, deferred_census};
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn internal(axiom: &str) -> owl_dl_core::InternalOntology {
    let src = format!(
        "Prefix(:=<http://x/>)\nOntology(\nDeclaration(Class(:A)) Declaration(Class(:B)) \
         Declaration(Class(:C)) Declaration(ObjectProperty(:r))\n{axiom}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    convert_ontology(&onto).unwrap()
}

/// Runs `f` with the flag set to `value`, restoring it after.
#[allow(unsafe_code)]
fn with_flag<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = "RUSTDL_CLAUSIFY_COMPLEX_DISJOINT";
    let prior = std::env::var_os(key);
    // SAFETY: set_var is unsafe under edition 2024; serialised via ENV_MUTEX and
    // restored below.
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    let out = f();
    // SAFETY: see above.
    unsafe {
        match prior {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    out
}

#[test]
fn the_default_and_flag_off_keep_the_deferrals() {
    for (axiom, kinds) in [
        (
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))",
            vec!["disjoint-union-covering"],
        ),
        (
            "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B))",
            vec!["disjoint-union-covering", "disjoint-union-member"],
        ),
        (
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))",
            vec!["disjoint-antecedent"],
        ),
    ] {
        let o = internal(axiom);
        for value in [None, Some("0")] {
            let off: Vec<&str> = with_flag(value, || deferred_census(&o))
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            assert_eq!(off, kinds, "{axiom}: flag {value:?} (the default is off)");
        }
        assert!(
            with_flag(Some("1"), || deferred_census(&o)).is_empty(),
            "{axiom}: flag on"
        );
    }
}

#[test]
fn atomic_members_clausify_identically_either_way() {
    for axiom in ["DisjointUnion(:C :A :B)", "DisjointClasses(:A :B :C)"] {
        let o = internal(axiom);
        let on = with_flag(Some("1"), || clausify_with_stats(&o).0);
        let off = with_flag(None, || clausify_with_stats(&o).0);
        assert_eq!(on, off, "{axiom}");
    }
}

fn parse(body: &str) -> owl_dl_core::InternalOntology {
    let src = format!("Prefix(:=<http://x/>)\nOntology(\n{body}\n)\n");
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    convert_ontology(&onto).unwrap()
}

/// With the flag on, each complex `DisjointClasses` / `DisjointUnion` shape
/// clausifies with nothing deferred (off it defers `disjoint-antecedent`,
/// `disjoint-union-member` or `disjoint-union-covering`).
#[test]
fn complex_disjoint_members_are_not_deferred_when_on() {
    for axiom in [
        "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))",
        "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B))",
        "DisjointClasses(:A ObjectAllValuesFrom(:r :B))",
        "DisjointClasses(:A ObjectMaxCardinality(1 :r :B))",
        "DisjointClasses(:A ObjectMinCardinality(2 :r :B))",
        "DisjointClasses(:A ObjectIntersectionOf(:B ObjectAllValuesFrom(:r :B)))",
        "DisjointClasses(:A ObjectUnionOf(:B ObjectAllValuesFrom(:r :B)))",
    ] {
        let o = internal(axiom);
        let (_c, stats) = with_flag(Some("1"), || clausify_with_stats(&o));
        assert_eq!(stats.deferred, 0, "{axiom}: {stats:?}");
        let (_c, stats) = with_flag(None, || clausify_with_stats(&o));
        assert!(stats.deferred > 0, "{axiom}: the default defers: {stats:?}");
    }
}

/// #600 review: with the flag on, trigger-less fallbacks (no body-encodable conjunct,
/// so a `⊤`-bodied clause) are capped at `TRIGGERLESS_FALLBACK_CAP` per
/// clausification; the rest are deferred under their pre-#600 kind and counted.
/// Fallbacks with a soft trigger are not capped.
#[test]
fn trigger_less_fallbacks_are_capped() {
    use std::fmt::Write as _;
    let census = |body: &str| {
        let o = parse(body);
        with_flag(Some("1"), || {
            (clausify_with_stats(&o).1.deferred, deferred_census(&o))
        })
    };
    let axioms = |k: usize, f: &dyn Fn(usize) -> String| {
        (0..k).fold(String::new(), |mut acc, i| {
            let _ = writeln!(acc, "{}", f(i));
            acc
        })
    };
    let hard_pair = |i: usize| {
        format!("DisjointClasses(ObjectAllValuesFrom(:r{i} :B) ObjectMaxCardinality(1 :s{i} :C))")
    };
    let cap = owl_dl_core::clause::TRIGGERLESS_FALLBACK_CAP;
    assert_eq!(
        census(&axioms(cap, &hard_pair)).0,
        0,
        "up to the cap: all admitted"
    );
    let (n, kinds) = census(&axioms(cap + 3, &hard_pair));
    assert_eq!(n, 3, "{kinds:?}");
    assert_eq!(kinds, vec![("disjoint-antecedent", 3)]);
    // A soft trigger (the atomic member) is never capped.
    let soft = |i: usize| format!("DisjointClasses(:A{i} ObjectAllValuesFrom(:r{i} :B))");
    assert_eq!(census(&axioms(cap + 8, &soft)).0, 0);
    // `DisjointUnion` members (`∀r.B ⊑ U`) count against the same cap, and a
    // trigger-less union pair past it is counted, not skipped silently.
    let member = |i: usize| format!("DisjointUnion(:U{i} :A{i} ObjectAllValuesFrom(:r{i} :B))");
    let (n, kinds) = census(&axioms(cap + 2, &member));
    assert_eq!((n, kinds), (2, vec![("disjoint-union-member", 2)]));
    let union_pair = |i: usize| {
        format!(
            "DisjointUnion(:U{i} ObjectAllValuesFrom(:r{i} :B) ObjectMaxCardinality(1 :s{i} :C))"
        )
    };
    // Per axiom the pair comes first, then the two members: the first axiom's pair
    // and first member take the cap; everything after is deferred.
    let (_n, kinds) = census(&axioms(2, &union_pair));
    assert_eq!(
        kinds,
        vec![("disjoint-union-member", 3), ("disjoint-union-pair", 1)]
    );
}

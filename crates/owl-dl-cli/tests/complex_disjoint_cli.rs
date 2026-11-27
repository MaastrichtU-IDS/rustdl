//! #600: the wedge clausifier defers a `DisjointClasses` / `DisjointUnion` member it
//! cannot encode as a body (`∀`, `≤n`, `≥n`, …) and a `DisjointUnion` covering with
//! any complex member. Both wedges trust their `Sat`, so `consistent` and `classify`
//! miss inconsistencies, unsatisfiable classes and subsumptions the main tableau
//! (`sat`, `subclass`) finds. The fix, `RUSTDL_CLAUSIFY_COMPLEX_DISJOINT=1`, is
//! opt-in (default OFF) until its cost is guarded. Every expected answer below was
//! checked against HermiT (and Konclude for the issue's repros).
//!
//! Negatives first: each case runs through the command that showed the miss, with
//! the fix on, and at the default it must show the old miss.
//! Then non-entailment controls (a covering with a disjunct missing would be an
//! unsound positive), and justify's minimality, which the fix moves.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::Command;

const NS: &str = "http://ex/#";
/// #600's fix is opt-in (default OFF: its `⊤`-bodied fallback clauses have unbounded
/// cost on growing ontologies). Every "fixed" arm sets it; every "old miss" arm runs at
/// the default, so these tests also pin that the default still misses.
const ON: (&str, &str) = ("RUSTDL_CLAUSIFY_COMPLEX_DISJOINT", "1");

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-complex-disjoint-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                 Ontology(<http://ex/cd>\n\
                 Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                 Declaration(Class(:D)) Declaration(Class(:E))\n\
                 Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:c))\n\
                 {body}\n)"
            ),
        )
        .unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn json(args: &[&str], env: &[(&str, &str)]) -> serde_json::Value {
    let out = run(args, env);
    assert!(out.status.success(), "{args:?} {env:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `consistent --json`'s decided verdict, as #605 does it: only exit 0 with
/// `incomplete: false` is a verdict. Exit 3 is a give-up (printed as `consistent:
/// true`, its sound direction, but not a proof), and accepting it would make the FP
/// control and the default-flag arms vacuous. A give-up is retried (host load can cut the
/// wedge), up to 3 attempts; never decided fails. The fallback deadline is raised too.
fn consistent(f: &Fixture, env: &[(&str, &str)]) -> bool {
    let mut env = env.to_vec();
    env.push(("RUSTDL_CONSISTENCY_FALLBACK_MS", "600000"));
    let mut last = None;
    for attempt in 1..=3 {
        let out = run(&["consistent", "--json", f.path()], &env);
        let v: Option<serde_json::Value> = serde_json::from_slice(&out.stdout).ok();
        if let Some(v) = v
            .as_ref()
            .filter(|v| out.status.code() == Some(0) && v["incomplete"] == false)
        {
            return v["consistent"].as_bool().unwrap();
        }
        eprintln!("consistent not decided on attempt {attempt} (host load?): retrying");
        last = Some(out);
    }
    panic!("consistent never decided in 3 attempts {env:?}: {last:?}");
}

/// `classify --json` with no per-pair deadline, so every answer is decided: at the
/// 5 ms default a loaded host cut pairs (`incomplete: true`) and the positive
/// assertions below missed. The default-flag misses do not depend on timing: they come
/// from the wedge's trusted `Sat` on the weaker clause set.
fn classify(f: &Fixture, env: &[(&str, &str)]) -> serde_json::Value {
    let v = json(
        &["classify", "--json", "--pair-timeout-ms", "0", f.path()],
        env,
    );
    assert_eq!(v["incomplete"], false, "{env:?}: {v}");
    v
}

fn unsat(v: &serde_json::Value, class: &str) -> bool {
    v["unsatisfiable"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == &format!("{NS}{class}"))
}

fn has_row(v: &serde_json::Value, sub: &str, sup: &str) -> bool {
    let want = serde_json::json!([format!("{NS}{sub}"), format!("{NS}{sup}")]);
    v["direct_subsumptions"].as_array().unwrap().contains(&want)
}

/// `disjoint-union-covering` (ABox) and `disjoint-antecedent` (ABox): inconsistent.
#[test]
fn inconsistent_abox_forms_are_found() {
    for (tag, body) in [
        (
            "du",
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))\n\
             ClassAssertion(:C :c) ClassAssertion(ObjectComplementOf(:A) :c)\n\
             ClassAssertion(ObjectAllValuesFrom(:r ObjectComplementOf(:B)) :c)",
        ),
        (
            "dc",
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             ClassAssertion(:A :a) ClassAssertion(ObjectAllValuesFrom(:r :B) :a)",
        ),
    ] {
        let f = Fixture::new(body, &format!("incons-{tag}"));
        assert!(!consistent(&f, &[ON]), "{tag}: consistent");
        assert_eq!(classify(&f, &[ON])["consistent"], false, "{tag}: classify");
        assert!(consistent(&f, &[]), "{tag}: the default shows the old miss");
    }
}

/// The TBox forms, and the shape diversity: each `D` is unsatisfiable (HermiT).
#[test]
fn unsatisfiable_classes_are_found() {
    for (tag, body) in [
        (
            "du-covering",
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))\n\
             SubClassOf(:D :C) SubClassOf(:D ObjectComplementOf(:A))\n\
             SubClassOf(:D ObjectAllValuesFrom(:r ObjectComplementOf(:B)))",
        ),
        (
            "dc-forall",
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             SubClassOf(:D :A) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
        (
            "du-two-hard-members",
            "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B) ObjectMaxCardinality(1 :s :B))\n\
             SubClassOf(:D ObjectAllValuesFrom(:r :B)) SubClassOf(:D ObjectMaxCardinality(1 :s :B))",
        ),
        (
            "dc-soft-trigger",
            "DisjointClasses(:E ObjectIntersectionOf(:A ObjectAllValuesFrom(:r :B)))\n\
             SubClassOf(:D :E) SubClassOf(:D :A) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
        (
            "dc-min",
            "DisjointClasses(:A ObjectMinCardinality(2 :r :B))\n\
             SubClassOf(:D :A) SubClassOf(:D ObjectMinCardinality(2 :r :B))",
        ),
        (
            "dc-or-member",
            "DisjointClasses(:E ObjectUnionOf(:B ObjectAllValuesFrom(:r :B)))\n\
             SubClassOf(:D :E) SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        ),
    ] {
        let f = Fixture::new(body, &format!("unsat-{tag}"));
        assert!(
            unsat(&classify(&f, &[ON]), "D"),
            "{tag}: classify misses D ⊑ ⊥"
        );
        assert!(
            !unsat(&classify(&f, &[]), "D"),
            "{tag}: the default shows the old miss"
        );
        // Control through the main tableau: `sat` decides it either way.
        let out = run(&["sat", f.path(), &format!("{NS}D")], &[]);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "unsat",
            "{tag}"
        );
    }
}

/// `disjoint-union-member`: `D ⊑ ∀r.B` and `∀r.B` a member of `C`'s union, so `D ⊑ C`.
#[test]
fn a_complex_member_is_a_subclass_of_the_union() {
    let f = Fixture::new(
        "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B))\n\
         SubClassOf(:D ObjectAllValuesFrom(:r :B))",
        "member",
    );
    assert!(
        has_row(&classify(&f, &[ON]), "D", "C"),
        "classify misses D ⊑ C"
    );
    assert!(
        !has_row(&classify(&f, &[]), "D", "C"),
        "the default: the old miss"
    );
}

/// Non-entailment controls: the covering must be the whole union (a missing
/// disjunct would make `C ⊑ A`), and disjointness must not reach a class that is in
/// only one member.
#[test]
fn nothing_is_entailed_beyond_the_axioms() {
    let f = Fixture::new("DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))", "fp-du");
    let v = classify(&f, &[ON]);
    assert!(!has_row(&v, "C", "A"), "C ⋢ A: {v}");
    assert!(has_row(&v, "A", "C"), "A ⊑ C: {v}");
    assert!(!unsat(&v, "C"), "C satisfiable: {v}");
    let f = Fixture::new(
        "DisjointClasses(:A ObjectAllValuesFrom(:r :B)) SubClassOf(:D :A)",
        "fp-dc",
    );
    let v = classify(&f, &[ON]);
    assert!(
        !unsat(&v, "A") && !unsat(&v, "D"),
        "A and D satisfiable: {v}"
    );
    assert!(consistent(
        &Fixture::new(
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))\n\
             ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :c)\n\
             ClassAssertion(ObjectComplementOf(:B) :c)",
            "fp-dc-abox",
        ),
        &[ON]
    ));
}

/// The clausification now leaves nothing deferred on `DisjointClasses(A, ∀r.B)`, so
/// the classify wedge's clause shape is Horn (`A → ∃r.Q`), and `analyze_fragment`
/// reads `Horn` instead of `OutOfFragment`. Horn is a clause-shape label, not a
/// completeness claim (#124), but justify's `minimal_guaranteed` admits it: pinned so
/// the shift is visible.
#[test]
fn the_horn_shift_reaches_justify() {
    let f = Fixture::new(
        "DisjointClasses(:A ObjectAllValuesFrom(:r :B)) SubClassOf(:D :A)",
        "horn",
    );
    let q = ["justify", "--json", f.path(), "subclass"];
    let (d, a) = (format!("{NS}D"), format!("{NS}A"));
    let args = [q[0], q[1], q[2], q[3], d.as_str(), a.as_str()];
    assert_eq!(json(&args, &[ON])["minimal"], true);
    assert_eq!(json(&args, &[])["minimal"], false);
    let fragment = |env: &[(&str, &str)]| {
        let out = run(&["classify", f.path()], env);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        text.lines()
            .find(|l| l.starts_with("# fragment:"))
            .map(str::to_owned)
            .unwrap_or_default()
    };
    assert!(fragment(&[ON]).contains("Horn"), "{}", fragment(&[ON]));
    assert!(fragment(&[]).contains("out-of-EL"), "{}", fragment(&[]));
}

/// #600 review (cost canary): a trigger-less fallback (no body-encodable conjunct) is a
/// `⊤`-bodied disjunctive clause, and classify cost grew exponentially in their number
/// (Fable's generator: 5 axioms 1 s, 8 cut, 32 a timeout; main 0.02 s). Past
/// `TRIGGERLESS_FALLBACK_CAP` they are deferred and counted, so K = 32 finishes uncut,
/// with every row the default arm (main's behaviour) finds.
#[test]
fn many_trigger_less_axioms_do_not_blow_up() {
    use std::fmt::Write as _;
    const K: usize = 32;
    let cap = owl_dl_core::clause::TRIGGERLESS_FALLBACK_CAP;
    for (tag, shape) in [
        (
            "hard-pair",
            "DisjointClasses(ObjectAllValuesFrom(:r{i} :B{i}) ObjectAllValuesFrom(:s{i} :C{i}))",
        ),
        (
            "max-pair",
            "DisjointClasses(ObjectMaxCardinality(1 :r{i} :B{i}) ObjectMaxCardinality(1 :s{i} :C{i}))",
        ),
        // `∀r.B ⊑ U` is the trigger-less part; `A ⊑ U` is a told row.
        (
            "du-member",
            "DisjointUnion(:U{i} :A{i} ObjectAllValuesFrom(:r{i} :B{i}))",
        ),
    ] {
        let mut body = String::new();
        for n in 1..20 {
            let _ = writeln!(
                body,
                "SubClassOf(:N{n} ObjectSomeValuesFrom(:t :N{}))",
                n - 1
            );
        }
        for i in 0..K {
            let _ = writeln!(body, "{}", shape.replace("{i}", &i.to_string()));
        }
        let f = Fixture::new(&body, &format!("cost-{tag}"));
        // A deadline that only a blow-up reaches: `incomplete` then reads true.
        let args = [
            "classify",
            "--json",
            "--pair-timeout-ms",
            "5000",
            "--global-timeout-ms",
            "120000",
            f.path(),
        ];
        let on = json(&args, &[ON]);
        assert_eq!(on["incomplete"], false, "{tag}: cut, a blow-up: {on}");
        assert_eq!(on["wedge_clauses_deferred"], K - cap, "{tag}: {on}");
        let off = json(&args, &[]);
        assert_eq!(off["incomplete"], false, "{tag}: default: {off}");
        for key in ["direct_subsumptions", "unsatisfiable", "equivalent_groups"] {
            assert_eq!(on[key], off[key], "{tag}: {key} lost or changed");
        }
        if tag == "du-member" {
            assert!(
                has_row(&on, "A0", "U0") && has_row(&on, "A31", "U31"),
                "{on}"
            );
        }
    }
}

/// The deferral signal at default flags: what #600 leaves deferrable is the antecedent
/// DNF cap (and the reflexive-derived cap). Seven binary unions in an antecedent are
/// 2⁷ = 128 alternatives, over the 64 cap, so classify reports the deferral.
#[test]
fn the_dnf_cap_deferral_is_reported_at_default_flags() {
    use std::fmt::Write as _;
    let mut ors = String::new();
    for i in 0..7 {
        let _ = write!(ors, "ObjectUnionOf(:L{i} :R{i}) ");
    }
    let f = Fixture::new(
        &format!("SubClassOf(ObjectIntersectionOf({ors}) :D)"),
        "dnf-cap",
    );
    let v = classify(&f, &[]);
    assert!(
        v["wedge_clauses_deferred"].as_u64().unwrap() > 0,
        "the DNF-cap deferral is not reported: {v}"
    );
    // Text mode warns, as for refused clauses; the JSON run warns on stderr too.
    for args in [
        vec!["classify", "--pair-timeout-ms", "0", f.path()],
        vec!["classify", "--json", "--pair-timeout-ms", "0", f.path()],
    ] {
        let out = run(&args, &[]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("clausifier skipped"), "{args:?}: {err}");
    }
    let out = run(&["classify", "--pair-timeout-ms", "0", f.path()], &[]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("# wedge clauses deferred: "), "{text}");
    let control = Fixture::new("SubClassOf(ObjectIntersectionOf(:A :B) :D)", "dnf-control");
    let out = run(&["classify", "--pair-timeout-ms", "0", control.path()], &[]);
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("clausifier skipped"),
        "{out:?}"
    );
}

/// #600 review (Fable): **the cost sentinel that gates a default flip.** One
/// trigger-less axiom (`DisjointClasses(∀r.B ∀s.C)`, a `⊤`-bodied clause, admitted under
/// the cap) beside a 200-class `∃t` chain: with the fix on, classify took 24.8–27 s
/// (release) and was cut, losing what main (0.07–0.11 s, complete) finds. A count cap
/// does not bound that cost; the fix stays opt-in until a cost guard makes this pass.
/// `#[ignore]`d because it fails today (with a 120 s global deadline, so it ends).
#[test]
#[ignore = "fails until the opt-in fallback is cost-guarded (#600); gates a default flip"]
fn a_long_chain_with_one_trigger_less_axiom_stays_complete() {
    use std::fmt::Write as _;
    let mut body =
        String::from("DisjointClasses(ObjectAllValuesFrom(:r :B) ObjectAllValuesFrom(:s :C))\n");
    for n in 1..200 {
        let _ = writeln!(
            body,
            "SubClassOf(:N{n} ObjectSomeValuesFrom(:t :N{}))",
            n - 1
        );
    }
    let f = Fixture::new(&body, "cost-sentinel");
    let v = json(
        &[
            "classify",
            "--json",
            "--global-timeout-ms",
            "120000",
            f.path(),
        ],
        &[ON],
    );
    assert_eq!(
        v["incomplete"], false,
        "the fallback's cost cut classify: {v}"
    );
}

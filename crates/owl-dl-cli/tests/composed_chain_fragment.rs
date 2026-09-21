//! #108 routing pin: composed-chain-plus-range ontologies must LEAVE the pure-EL
//! fast path (the saturator is incomplete there), and the admitted chain shapes
//! must STAY on it (de-certifying them costs the fast path on 16% of ORE for
//! nothing — the narrowed predicate hits 2.2%).
//!
//! The answer-level assertions live in `owl-dl-reasoner/tests/composed_chain_range.rs`;
//! this file pins only the ROUTING via the `# fragment:` banner line, because a
//! correct answer via an unintended path would mask a routing regression until a
//! slower fixture surfaced it.
#![allow(clippy::unwrap_used)]

use std::process::Command;

fn fragment_of(body: &str) -> String {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/p>\n\
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u)) Declaration(ObjectProperty(:v))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n{body}\n)\n"
    );
    // std-only temp file: the workspace has no `tempfile` dep and one test does
    // not justify adding it. Uniqueness via a process-wide counter + pid.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "rustdl-frag-{}-{}.ofn",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, ofn).expect("write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .arg("classify")
        .arg(&path)
        .output()
        .expect("run rustdl");
    let _ = std::fs::remove_file(&path);
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find(|l| l.starts_with("# fragment:"))
        .expect("banner has a fragment line")
        .trim_start_matches("# fragment: ")
        .split(" (")
        .next()
        .unwrap()
        .to_owned()
}

const NEST: &str =
    "SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:v :A))))";

#[test]
fn composed_chain_with_range_leaves_the_fast_path() {
    let frag = fragment_of(&format!(
        "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:r :v) :s)\n\
         ObjectPropertyRange(:s :F)\n{NEST}"
    ));
    assert_ne!(
        frag, "pure-EL",
        "the saturator's range fold cannot form the composed key (t,u,v) ⊑ s; \
         certifying it complete here is the #108 D10 bug"
    );
}

#[test]
fn admitted_chain_shapes_stay_on_the_fast_path() {
    // single chain + range (#84's completed case)
    assert_eq!(
        fragment_of(&format!(
            "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\nObjectPropertyRange(:r :F)\n{NEST}"
        )),
        "pure-EL",
        "single-chain range is #84-complete and must keep the fast path"
    );
    // composed chains WITHOUT any range
    assert_eq!(
        fragment_of(&format!(
            "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
             SubObjectPropertyOf(ObjectPropertyChain(:r :v) :s)\n{NEST}"
        )),
        "pure-EL",
        "composition alone is complete (∃-LHS and domains verified) — rejecting it \
         would cost 16% of the ORE pool the fast path, including corpus-verified `ro`"
    );
    // composed chains + DOMAIN
    assert_eq!(
        fragment_of(&format!(
            "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
             SubObjectPropertyOf(ObjectPropertyChain(:r :v) :s)\n\
             ObjectPropertyDomain(:s :F)\n{NEST}"
        )),
        "pure-EL",
        "the domain side of the fold is complete and must keep the fast path"
    );
}

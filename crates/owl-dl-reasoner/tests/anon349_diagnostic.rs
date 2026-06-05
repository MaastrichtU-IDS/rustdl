//! Diagnostic test for the Anonymous-349 closure-realization anomaly
//! (see `docs/phase2e-notgalen-diagnosis.md` Addendum 2026-06-02).
//!
//! Loads full notgalen.ofn, runs `classify_top_down_with_timeout(200ms)`
//! (the same code path the corpus closure-diff uses), and asserts that
//! the resulting `entailed` matrix DOES close `Anonymous-349 ⊑ Anonymous-324`
//! and DOES realize the equivalence `Anonymous-324 ≡ IPBP`.
//!
//! Run 2026-06-02 confirmed all four assertions pass (classify wall ~31 min,
//! standalone — no concurrent load). The T7 closure-diff at commit 34a2b62
//! reported these pairs as MISSED while running concurrent with the SIO
//! flamegraph capture + GALEN classify; this standalone diagnostic agrees
//! with `rustdl explain` + `rustdl classify --pair-timeout-ms 200` CLI output
//! and disagrees with the T7 MISSED report — supporting the addendum's
//! "concurrency / scheduling artifact in the corpus run" interpretation.
//!
//! Ignored in CI; manually re-runnable to verify the anomaly hasn't moved.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const ANON349: &str = "http://galen.org/galen.owl#Anonymous-349";
const ANON324: &str = "http://galen.org/galen.owl#Anonymous-324";
const IPBP: &str = "http://galen.org/galen.owl#IntrinsicallyPathologicalBodyProcess";

#[test]
#[ignore = "diagnostic — loads full notgalen"]
fn anon349_diagnostic() {
    let p = Path::new("../../ontologies/external/notgalen.ofn");
    if !p.exists() {
        eprintln!("SKIP: notgalen not present");
        return;
    }
    let src = std::fs::read_to_string(p).expect("read");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let start = std::time::Instant::now();
    let c = classify_top_down_with_timeout(&onto, Duration::from_millis(200)).expect("classify");
    eprintln!("classify wall = {:.2?}", start.elapsed());

    let s1 = c.is_subclass(ANON349, ANON324);
    let s2 = c.is_subclass(ANON349, IPBP);
    let s3 = c.is_subclass(ANON324, IPBP);
    let s4 = c.is_subclass(IPBP, ANON324);
    let direct = c.direct_subsumers(ANON349);
    let eq324 = c.equivalent_classes(ANON324);
    let eqipbp = c.equivalent_classes(IPBP);
    eprintln!("is_subclass(Anon-349, Anon-324) = {s1}");
    eprintln!("is_subclass(Anon-349, IPBP)     = {s2}");
    eprintln!("is_subclass(Anon-324, IPBP)     = {s3}");
    eprintln!("is_subclass(IPBP, Anon-324)     = {s4}");
    eprintln!("Anon-349 direct supers: {direct:?}");
    eprintln!("Anon-324 equivalents:   {eq324:?}");
    eprintln!("IPBP equivalents:       {eqipbp:?}");

    assert!(
        s1,
        "classify must close Anon-349 ⊑ Anon-324 on full notgalen"
    );
    assert!(s2, "classify must close Anon-349 ⊑ IPBP on full notgalen");
    assert!(
        s3 && s4,
        "classify must realize Anon-324 ≡ IPBP on full notgalen"
    );
    assert!(
        direct.iter().any(|s| *s == ANON324) || direct.iter().any(|s| *s == IPBP),
        "Anon-349 must have Anon-324 or IPBP in direct supers: {direct:?}"
    );
    assert!(
        eq324.iter().any(|c| *c == IPBP),
        "Anon-324's equivalence partners must include IPBP: {eq324:?}"
    );
}

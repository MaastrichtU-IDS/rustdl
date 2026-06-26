//! Negatives-first soundness canary for tautology-skip (`RUSTDL_TAUTOLOGY_SKIP`).
//! The mechanism skips a binary disjunction `a ⊔ ¬a` (complement pair `b ≡ ¬a`)
//! as vacuously satisfied. It is sound ONLY for genuine complement pairs; the
//! MISSED guard verifies a NON-complement binary disjunction is NOT skipped (else
//! a real obligation is dropped → false Sat → missed subsumption/unsat).
//! Process-isolated env via a dedicated test binary.
#![allow(unsafe_code, clippy::unwrap_used, clippy::default_trait_access)]

use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn sat_is_unsat(src: &str, class_iri: &str) -> bool {
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: serialized by ENV_LOCK; dedicated test binary.
    unsafe {
        std::env::set_var("RUSTDL_TAUTOLOGY_SKIP", "1");
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let (ont, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::Cursor::new(src.as_bytes().to_vec()),
        Default::default(),
    )
    .unwrap();
    let out = owl_dl_reasoner::sat_class_probe(
        &ont,
        class_iri,
        64,
        Some(std::time::Duration::from_secs(5)),
    )
    .expect("probe ok")
    .expect("class resolves");
    matches!(out.0, owl_dl_tableau::hyper::HyperResult::Unsat)
}

/// MISSED GUARD (soundness): `X ≡ A ⊔ B` where A,B are NOT a complement pair, and
/// both A and B are unsatisfiable (`Z ⊓ ¬Z`). X must be UNSAT — the `A ⊔ B`
/// disjunction is a REAL obligation that must be explored (both disjuncts clash),
/// NOT skipped. If tautology-skip wrongly skipped it, X would be SAT (missed unsat).
#[test]
fn tautology_skip_does_not_skip_non_complement_disjunction() {
    let src = "Prefix(:=<http://t/>)\n\
Ontology(<http://t/o>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Z))\n\
    SubClassOf(:A :Z) SubClassOf(:A ObjectComplementOf(:Z))\n\
    SubClassOf(:B :Z) SubClassOf(:B ObjectComplementOf(:Z))\n\
    EquivalentClasses(:X ObjectUnionOf(:A :B))\n\
)\n";
    assert!(
        sat_is_unsat(src, "http://t/X"),
        "X ≡ A⊔B with A,B both unsat (non-complement) must be UNSAT — \
         tautology-skip must NOT skip a real disjunction (MISSED guard)"
    );
}

/// POSITIVE (verdict-preserving): `NotA ≡ ¬A`, `C ≡ Foo ⊓ (A ⊔ NotA)`. The
/// `A ⊔ ¬A` disjunction IS a complement pair (skipped), but it is a tautology, so
/// C remains satisfiable (Foo is). Confirms skipping a genuine tautology does not
/// break a satisfiable class.
#[test]
fn tautology_skip_preserves_sat_of_tautology_class() {
    let src = "Prefix(:=<http://t/>)\n\
Ontology(<http://t/o>\n\
    Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:NotA)) Declaration(Class(:Foo))\n\
    EquivalentClasses(:NotA ObjectComplementOf(:A))\n\
    EquivalentClasses(:C ObjectIntersectionOf(:Foo ObjectUnionOf(:A :NotA)))\n\
)\n";
    assert!(
        !sat_is_unsat(src, "http://t/C"),
        "C ≡ Foo ⊓ (A ⊔ ¬A): the tautology is skipped but C stays SAT (Foo satisfiable)"
    );
}

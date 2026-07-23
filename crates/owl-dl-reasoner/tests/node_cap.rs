//! Task 1 of issue-#35 v4: the deterministic live-node cap
//! (`RUSTDL_MAX_NODES`) on the deadline-free tableau path must degrade
//! an unbounded/exploding search to a SOUND `Ok` result (a MISS /
//! "consistent" under-approximation) — never a hang, never
//! `Err(NoVerdict)`, never a panic.
//!
//! The ontology below is the issue-#35 `hang_v3` core: a `{a} ⊓ ¬C`
//! instance probe over `Person ⊑ ∃hasMother.Woman` + the Man/Woman
//! covering disjunction builds an infinite maternal ∃-cycle anchored at
//! a nominal root (the `isMotherOf(a,b)` `ABox` edge). With
//! `RUSTDL_ANYWHERE_BLOCKING=0` (ancestor-only pair-blocking) the
//! completion graph grows essentially without bound near the nominal
//! anchor, so pre-fix (no `RUSTDL_MAX_NODES` support at all — the env
//! var is simply inert) the `realize` call in this test hangs
//! indefinitely (verified directly: `git stash` the source changes,
//! rerun this test, and it does not return inside 2 minutes — matches
//! the ~70 s+ real-KG-scale hang this fixture is modelling).
//!
//! **Cap value (40, not a "round" 300+ number): deliberately small.**
//! Historically (Task 1) `search::branch`'s `NodeCap` handling was
//! *soft* — a cap-tripped disjunct rolled back and the loop still tried
//! its sibling(s), mirroring `DepthLimit`'s soft semantics — and this
//! fixture's covering disjunction (`Person ≡ Man ⊔ Woman`) opened one
//! independent choice point per generated Person-successor, making the
//! number of sibling combinations explored after the cap tripped roughly
//! exponential in how many successors existed before the cap stopped
//! new-node creation (empirically: caps 15–50 resolved in well under a
//! second; 60 already took ~7 s; 80–100+ exceeded an 8 s budget; see
//! task-1-report.md). **Task B made `NodeCap` a HARD early-return**
//! instead (search.rs's doc comment on `SearchVerdict::NodeCap`): the
//! first sibling to trip the cap abandons the rest immediately, so the
//! exponential-in-cap-value blowup no longer applies — this test's cap
//! choice is no longer load-bearing for wall time, but 40 is kept as-is
//! (still exercises real multi-node growth before the trip) since the
//! test's actual contract — `NodeCap` degrades to `Ok`, never a hang or
//! error — is unaffected by which handling is in force. No `ObjectOneOf`
//! / cardinality is involved, so the later nominal-first scheduling fix
//! (Task A) does not mask this safety net.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. This is the only
        // test in this binary (single-threaded), restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _prefixes) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    onto
}

const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/nodecap>
  Declaration(Class(:Person)) Declaration(Class(:Man)) Declaration(Class(:Woman))
  Declaration(Class(:Male)) Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasMother)) Declaration(ObjectProperty(:hasSex))
  Declaration(ObjectProperty(:hasParent)) Declaration(ObjectProperty(:isMotherOf))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))

  SubClassOf(:Person ObjectSomeValuesFrom(:hasMother :Woman))
  EquivalentClasses(:Person ObjectUnionOf(:Man :Woman))
  EquivalentClasses(:Man   ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))
  EquivalentClasses(:Woman ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Female)))
  ObjectPropertyDomain(:hasParent :Person)
  ObjectPropertyAssertion(:isMotherOf :a :b)
)
";

#[test]
fn node_cap_degrades_to_ok_not_error() {
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "40");
    // Ancestor-only pair-blocking: force the nominal-anchored ∃-cycle
    // blowup that anywhere-blocking (the #35 v3 fix) would otherwise tame.
    let _blocking = SetEnvGuard::set("RUSTDL_ANYWHERE_BLOCKING", "0");
    let onto = parse(SRC);
    let r = owl_dl_reasoner::realize(&onto);
    assert!(r.is_ok(), "cap trip must be Ok(sound MISS), got {r:?}");
}

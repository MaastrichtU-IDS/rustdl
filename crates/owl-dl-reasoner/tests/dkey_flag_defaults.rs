//! Default-pinning canaries for the two `DKey` levers flipped ON on 2026-08-03
//! after the volume scan in `docs/2026-08-03-dkey-volume-scan.md`:
//! `RUSTDL_DKEY_EMIT_ORDER` and `RUSTDL_DKEY_ONEOF_SEED`.
//!
//! **Each test pins BOTH halves of the default**, which is the part that is easy
//! to get wrong. The house idiom for a default-ON flag is
//! `is_none_or(|v| v != "0")`, under which:
//!
//! | value        | behaviour |
//! |--------------|-----------|
//! | unset        | **ON**    |
//! | `""` (empty) | **ON**    |
//! | `"0"`        | OFF       |
//! | `"1"`        | ON        |
//!
//! The empty-string row is the one a default-ON flag most easily gets wrong: the
//! opt-in idiom these two flags used to carry (`is_some_and(|v| v == "1")`) makes
//! `""` mean OFF, and a `VAR=` in a shell wrapper is a common accident. A flip
//! that changed only the unset row would leave `""` silently reverting.
//!
//! The fixtures are the ones the levers were built against, so a test here fails
//! if the flag stops being read, if the default is reverted, or if `=0` stops
//! reverting.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Env mutation is process-wide; serialize it and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: `set_var` is unsafe under edition 2024. Serialized via
        // ENV_MUTEX, restored on Drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }

    /// Remove the variable — the only way to exercise a *shipped default*
    /// rather than a value the test itself chose.
    #[allow(unsafe_code)]
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see `set`.
        unsafe { std::env::remove_var(key) };
        Self { key, prior }
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `EnvGuard::set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

// ── RUSTDL_DKEY_EMIT_ORDER ─────────────────────────────────────────────────

/// `Direct ⊑ ∀p.[0,5] ⊓ ∃p.{9}` (unsatisfiable), plus an UNRELATED data property
/// `q` that merely mentions the same two keys in value position. Before the
/// lever, `q`'s component spent the `[0,5] ⟂ 9` pair and declined it, so the
/// axiom was never emitted and `Direct` came back satisfiable — a non-monotonic
/// defect: adding an unrelated axiom removed an entailment.
///
/// Konclude and `HermiT` both report `Direct ≡ owl:Nothing` here (adjudicated
/// 2026-08-03), so the lever's extra disjointness is entailed, not an FP.
const EMIT_ORDER_FIXTURE: &str = r#"Prefix(:=<http://ex.org/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/x>
Declaration(Class(:Direct))
Declaration(Class(:Other))
Declaration(DataProperty(:p))
Declaration(DataProperty(:q))
SubClassOf(:Direct DataAllValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
SubClassOf(:Direct DataHasValue(:p "9"^^xsd:integer))
SubClassOf(:Other DataAllValuesFrom(:q DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "20"^^xsd:integer)))
SubClassOf(:Other DataSomeValuesFrom(:q DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
SubClassOf(:Other DataHasValue(:q "9"^^xsd:integer))
)
"#;

fn direct_is_unsat() -> bool {
    let c = owl_dl_reasoner::classify(&parse(EMIT_ORDER_FIXTURE)).expect("classify");
    c.unsatisfiable_classes()
        .iter()
        .any(|u| u.ends_with("/Direct"))
}

#[test]
fn emit_order_default_is_on_and_zero_reverts() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    {
        let _g = EnvGuard::unset("RUSTDL_DKEY_EMIT_ORDER");
        assert!(
            direct_is_unsat(),
            "UNSET must mean ON: the non-monotonicity fix is default-ON since \
             2026-08-03, so `Direct` must be unsatisfiable with no env override"
        );
    }
    {
        // The row an opt-in-shaped predicate gets wrong.
        let _g = EnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "");
        assert!(
            direct_is_unsat(),
            "EMPTY must mean ON for a default-ON flag (`is_none_or(|v| v != \"0\")`); \
             only an explicit `=0` reverts. If this fires, the predicate is still \
             written in the opt-in shape `is_some_and(|v| v == \"1\")`."
        );
    }
    {
        let _g = EnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "0");
        assert!(
            !direct_is_unsat(),
            "`=0` must revert to the pre-fix behaviour (the documented escape \
             hatch). If this fires, the flag is no longer read at all and the \
             other two arms above are passing vacuously."
        );
    }
}

// ── RUSTDL_DKEY_ONEOF_SEED ─────────────────────────────────────────────────

/// `C ≡ F ≡ ∃h.{1}`, `D ≡ ∃h.{1,2}`, `E ≡ ∃h.{1,2,3}`. The numeric `DataOneOf`
/// buckets were minted but never seeded, so `{1} ⊆ {1,2}` yielded no `F ⊑ D` —
/// the sixth D10-class bug (the gate certified the closure complete while the
/// engine dropped the axiom). Konclude and `HermiT` between them report
/// `C ≡ F`, `F ⊑ D`, `C ⊑ D`, `D ⊑ E` (adjudicated 2026-08-03).
const ONEOF_FIXTURE: &str = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
Declaration(Class(:C))
Declaration(Class(:D))
Declaration(Class(:E))
Declaration(Class(:F))
Declaration(DataProperty(:h))
EquivalentClasses(:C DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer)))
EquivalentClasses(:F DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer)))
EquivalentClasses(:D DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
EquivalentClasses(:E DataSomeValuesFrom(:h DataOneOf("1"^^xsd:integer "2"^^xsd:integer "3"^^xsd:integer)))
)
"#;

fn f_subclass_d() -> bool {
    let c = owl_dl_reasoner::classify(&parse(ONEOF_FIXTURE)).expect("classify");
    c.is_subclass("http://t/F", "http://t/D")
}

#[test]
fn oneof_seed_default_is_on_and_zero_reverts() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    {
        let _g = EnvGuard::unset("RUSTDL_DKEY_ONEOF_SEED");
        assert!(
            f_subclass_d(),
            "UNSET must mean ON: numeric DataOneOf seeding is default-ON since \
             2026-08-03, so `{{1}} ⊆ {{1,2}}` must give F ⊑ D with no override"
        );
    }
    {
        let _g = EnvGuard::set("RUSTDL_DKEY_ONEOF_SEED", "");
        assert!(
            f_subclass_d(),
            "EMPTY must mean ON for a default-ON flag; only an explicit `=0` \
             reverts. If this fires, the predicate is still opt-in-shaped."
        );
    }
    {
        let _g = EnvGuard::set("RUSTDL_DKEY_ONEOF_SEED", "0");
        assert!(
            !f_subclass_d(),
            "`=0` must revert to the unseeded behaviour. If this fires, the flag \
             is no longer read and the other two arms pass vacuously."
        );
    }
}

// ── The volume instrument itself ───────────────────────────────────────────

/// **Guards the `tbox-stats` told-table counters** that the 2026-08-03 `DKey`
/// volume scan was decided on (`docs/2026-08-03-dkey-volume-scan.md`).
///
/// Those two fields were added *for* that scan, and nothing else in the tree
/// reads them — so without this test a refactor could silently make either read
/// 0 and the next scan would report "no volume growth anywhere" for the most
/// boring possible reason. That failure mode is not hypothetical here: the
/// 2026-07-30 population scan was retracted for exactly it (a binary that
/// emitted no `DKey` disjointness at all read the same 113 on `ore_ont_9347` as
/// the correct gate, and `ore_ont_5368` was the only case that could tell them
/// apart).
///
/// The assertion is a DIFFERENTIAL rather than an absolute count, so it pins
/// that the counter tracks the axioms the lever emits — `told_disjoint_pairs`
/// must be 1 with the lever on and 0 with it off, on the fixture whose whole
/// point is that exactly one `DisjointClasses(DKey, DKey)` is at stake.
#[test]
fn tbox_stats_told_counters_track_the_emitted_dkey_disjointness() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(EMIT_ORDER_FIXTURE);

    let off = {
        let _g = EnvGuard::set("RUSTDL_DKEY_EMIT_ORDER", "0");
        owl_dl_reasoner::tbox_stats(&onto).expect("tbox_stats")
    };
    let on = {
        let _g = EnvGuard::unset("RUSTDL_DKEY_EMIT_ORDER");
        owl_dl_reasoner::tbox_stats(&onto).expect("tbox_stats")
    };

    assert_eq!(
        off.told_disjoint_pairs, 0,
        "lever OFF emits no DisjointClasses(DKey, DKey) on this fixture — if this \
         is non-zero the counter is measuring something else"
    );
    assert_eq!(
        on.told_disjoint_pairs, 1,
        "lever ON emits exactly ONE DisjointClasses(DKey, DKey) ([0,5] vs 9). A 0 \
         here means the told_disjoint_pairs counter is dead, which would make a \
         volume scan built on it silently vacuous"
    );
    // told_super_edges must be populated at all (it is the sink `ONEOF_SEED`'s
    // `DKey ⊑ DKey` edges land in, and `told.rs` closes it transitively).
    assert!(
        on.told_super_edges > 0,
        "told_super_edges read 0 on an ontology with named classes — the counter \
         is dead; it is reflexive, so it can never legitimately be 0 here"
    );
}

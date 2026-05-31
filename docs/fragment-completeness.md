# Fragment completeness — what `trust_sat` rides on

`RUSTDL_HYPERTABLEAU_TRUST_SAT` is default-on as of 2026-05-29
(`crates/owl-dl-reasoner/src/lib.rs:649-651`). When on, the hyper
wedge concludes "not subsumed" from its own `Sat` verdict without
consulting the tableau. That is sound **iff** the hyper engine is
*complete* on the workload — see `docs/hypertableau-summary.md` §3.

This doc states precisely what that "iff" is grounded in today:
which fragment the engine is **provably** complete on (the safe
zone), which constructs are **verified by composition** rather than
proven (the empirical zone), and what would have to change for
default-on to be defensible generally (the Phase 4 hook).

## Provably complete fragment

The classify path funnels every query through two engines in sequence:
the consequence-based EL saturator (`crates/owl-dl-saturation`) and,
for the residual, the hyper engine (`crates/owl-dl-tableau/src/hyper.rs`).
Together they are provably complete on the intersection of what each
engine covers.

The EL saturator is complete on the fragment described by Kazakov,
Krötzsch, Simančík (JAR 2014) "The Incredible ELK"
(`crates/owl-dl-saturation/src/lib.rs:1-6`). Within that fragment, one
fixed-point loop computes the subsumer closure over atomic classes using:
told subsumption (`SubClassOf(A, B)`), conjunction distribution and
conjunctive triggers (`SubClassOf(A ⊓ … ⊓ B, C)`), CR5 existential
propagation (`∃r.Y` on either side of a `SubClassOf` with chain
composition), Tseitin introduction for compound existential bodies
(`∃r.(B₁ ⊓ … ⊓ Bₙ)` rewritten to `∃r.F` where `F ≡ B₁ ⊓ … ⊓ Bₙ`),
CR9 role hierarchy (sub-role / equivalent-role closure), length-2 role
chains plus `TransitiveObjectProperty`, domain and range propagation
through the super-role closure, and `DisjointClasses` → Bot detection
(`crates/owl-dl-saturation/src/lib.rs:10-49`). When the saturator
returns `Sat` on an input that falls entirely within this EL fragment,
that verdict reflects a genuine closure: no further consequence can be
derived, and `Sat` is a model by construction.

The hyper engine is complete on Horn DL-clauses — clauses with at most
one head atom. For such clauses, a matching body fires a single,
unchosen consequence: no disjunctive branching is possible, and the
Horn fixpoint runs deterministically to completion
(`crates/owl-dl-tableau/src/hyper.rs:18-26`). When every
clausified constraint on the input has ≤1 head atom, the engine's
`Sat` verdict is a completed, branch-free Horn model, not a
partial search result. The clausifier documents this as approximately
96 % of the corpus being Horn (`crates/owl-dl-tableau/src/hyper.rs:23-26`).

When a workload's expressivity profile falls entirely inside the
supported-EL or Horn fragment — both of which apply simultaneously
for pure EL inputs — `trust_sat` is sound by construction, not by
measurement: a `Sat` verdict from these engines is a genuine model,
and there is nothing the full SROIQ tableau could add.

- **EL++ functional-role witness-merge** (Phase 2a): if `R_i, R_j ⊑ R_f`
  and `R_f` is functional, `X ⊑ ∃R_i.A ⊓ ∃R_j.B` implies
  `X ⊑ ∃R_f.(A ⊓ B)`. Standard EL++ extension (Baader/Brandt/Lutz 2005).
  Atom-set accumulation (per-(sub, R_f) flat set of atomic class IDs)
  ensures termination on dense functional-role hierarchies. See
  `phase2a-results.md` for the corpus-impact measurement (empirically
  doesn't fire on GALEN's MISSED — the handoff's trace was incomplete).

## Verified by composition, NOT proven

Beyond the provably complete fragment, the hyper engine handles a
larger set of SROIQ constructs whose completeness is corpus-validated
but not backed by a formal calculus proof.

Disjunctive branching (H2, `crates/owl-dl-tableau/src/hyper.rs:8-11`)
extends Horn to multi-head clauses via backtracking search with
dependency-directed backjumping (per-label and per-node `birth_deps`
dep-sets, `crates/owl-dl-tableau/src/hyper.rs:164-167`).
Qualified cardinality restrictions involve three sub-features: HF3a
(`≥n` generation) was built and directly tested; HF3b (the `≠`-witness
clash for `≤n` conflicts) and HF3c were **verified by composition** —
the propagation arc and per-node `Label` firing already handled these
cases without dedicated phase work
(`docs/hypertableau-summary.md` §1 wave 2). Nominals are handled by the
NN-rule (HF4a, built): nominals are treated as singletons and merged
when necessary. HF4b (the `≥2 R.{o}` unsat via NN-merge + `≠` clash)
was likewise **verified by composition** rather than built
(`docs/hypertableau-summary.md` §1 wave 2). Inverse roles and double-
blocking use the Motik/Shearer/Horrocks §3.4 pair-blocking condition (HF2,
equal labels + equal parent labels + equal edge role,
`crates/owl-dl-reasoner/src/lib.rs:626-636`).

For every one of these constructs, the claim of completeness rests on
every test in `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`
agreeing FP=0 with the reference oracle, not on a formal calculus proof.
The engineering lessons in `docs/hypertableau-summary.md` §5 are explicit
on this: "Verify-before-build kept paying off. HF3b, HF3c, HF4b each
turned out to be achieved by composition, not built." This is
meaningful empirical evidence, but it is empirical.

## Validated corpus envelope (the empirical "where it's safe")

The hyper engine has been run against a sound-and-complete reference
(Konclude for the pre-Phase-0 set; ROBOT v1.9.6 + HermiT for the Phase 0
additions) on the following ontologies, all returning FP=0
(`docs/hypertableau-summary.md` §2):

- pizza (SHOIN, 499 classes) — 499/499 at 5 s timeout, 0 FP, 0 MISSED,
  100 % complete.
- ro-stripped (SROIFV, 158 classes) — 158/158, 0 FP, 0 MISSED.
- sulo-stripped (SRI, 51 classes) — 51/51, 0 FP, 0 MISSED.
- SIO (SRIQ, 1585 classes) — 8902/8904, 0 FP, 2 MISSED (99.98 %).
- GALEN (SHIF, 2748 classes, ORE 2015) — 27888/27997, 0 FP,
  109 MISSED (99.6 %).
- notgalen (SHIF, 3087 classes, ORE 2015) — 32712/32739, 0 FP,
  27 MISSED (99.9 %).
- ALEHIF+ test (168 classes, ORE 2015) — 247/247, 0 FP, 0 MISSED.

Phase 0 (Task 5) added two further ORE 2015 fixtures against the HermiT
oracle, both returning FP=0 **and** MISSED=0
(`docs/phase0-soundness-results.md`):

- ore-10908-sroiq (SROIQ, 693 classes) — 0 FP, 0 MISSED (100 % complete).
- ore-15672-shoin (SHOIN, 83 classes) — 0 FP, 0 MISSED (100 % complete).

Phase 0 (Task 5) added two further ontologies, each from a distinct expressivity
profile (SROIQ and SHOIN), where the engine was both sound and fully complete
against the HermiT oracle.

Outside this validated set, `trust_sat`-default-on is an empirical bet, not
a proof. The design contract codified in `docs/hypertableau-dead-ends.md`
§11 is explicit: "Validated on the corpus ≠ validated generally."

Phase 1 (selective trust-sat verification) shipped the mechanism but
left it disabled by default — the empirical sweep (`phase1-results.md`)
showed wall-time is not a usable discrimination signal at the
sub-millisecond resolution where wedge NotSubsumed verdicts actually
complete. The validated envelope is unchanged; Phase 1 did not narrow
or widen what counts as sound.

## Soundness implication

> trust_sat is sound iff the engine is complete on the workload.

A workload whose expressivity profile falls entirely inside the Provably
Complete fragment (supported-EL or Horn DL-clauses, as described above) is
safe by construction — `Sat` is a genuine model and no tableau call could
add information. A workload covered by the Validated Corpus Envelope is safe
by measurement — every tested ontology in that set agreed with a sound and
complete oracle at FP=0. A workload that is neither EL/Horn nor a member of
the validated set is the risk surface: the engine may return `Sat` where a
complete reasoner would find `Unsat`, producing a missed subsumption. The
opt-out is `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`
(`crates/owl-dl-reasoner/src/lib.rs:649-651`), which treats any `Sat`
verdict from the wedge as `Unknown` and falls through to the full tableau —
slower (no classification wall reduction), but unconditionally sound on any
ontology.

## What would earn default-on generally

A sound static check that decides, for a given ontology, whether its
expressivity profile falls inside the Provably Complete fragment would turn the
current empirical claim into a per-ontology proof. That check is the Phase 4
auto-gate (see
`docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 4 — Generalization capstone"). Until then, the design contract
codified in `docs/hypertableau-dead-ends.md` §11 — opt-in flags for
sound-where-validated — is the principled position. Phase 0's broadened corpus
(Task 5) widens the validated set and adds two expressivity profiles where
completeness is fully measured; it does not replace the proof. Every new
ontology added to the validation harness in
`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` extends the empirical
envelope; a calculus proof, or a static fragment-membership check at classify
time, would be the step that makes default-on defensible without measurement.

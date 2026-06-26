# SP3 Phase-2 — production ∃-seed (coupled-saturation precompletion) design

**Status:** design. Phase-2 (production) of the precompletion-graph coupling. The mechanism
is probe-validated (`docs/sat-precompletion-probe-results-2026-06-26.md`): the derived-∃-fact
seed collapses CabernetFranc DNF→209 ms (~250×), sound (Sat-preserved). Phase-2 wires it into
the real classify path and gates it at corpus scale.

## Goal

Wire the validated derived-∃-fact seed into the classify path (the label-cache build + the
per-pair tier walk), alongside SP2's named-subsumer seed, so the per-class collapse becomes a
**net wine classify speedup** — and **prove it sound at scale** (FP=0/MISSED=0 byte-identical
corpus-wide). Default OFF; the gate decides ship/flip.

## Mechanism (probe-validated)

The saturation derives the deterministic **value-assignment ∃-facts** (`Zinfandel ⊑
∃hasColor.{Red}`) that resolve wine's value-search — which *named* subsumers (SP2, ~7.5%
ceiling) cannot. Seeding them via `Q → ∃R.target` clauses cascades determinism through the
wedge's `horn_fixpoint`, collapsing the disjunctive model search. Targets are translated to
**wedge-native** ids: named direct; NomKey synthetic → `ClassId::new(num_classes +
ind.index())` (the wedge clausifier's nominal id for `{a}`, bridged by the shared
`IndividualId`); other synthetics (Tseitin/DKey) **dropped** (sound under-approximation).

## Soundness

Derived ∃-facts are **all-model entailed** (the saturator is sound: `c ⊑ ∃R.target` holds in
every model), so seeding them is **monotone** — cannot flip Sat↔Unsat. Same basis as the
named seed; *not* the snapshot reuse-trap. FP-delicate part = the nominal translation, which
Phase-1 verified per-class (verdict-preserved) and the garbage control isolated. **The
classify-scale proof is the full-corpus FP=0/MISSED=0 gate (Task 2), not this argument** —
four "sound by construction" claims were corpus-refuted earlier this project.

## Architecture (3 pieces, mirroring SP2/SP2.1)

1. **Compute the ∃-seed table once** in `HyperCache::build(&internal)` (when
   `hyper_sat_seed_enabled()`): a field `exists_seed: Option<Vec<Vec<(Role, ClassId)>>>`
   indexed by class id. Built from `owl_dl_saturation::saturate_with_exists_facts(&internal)`
   — for each derived `(sub, role, target)`, translate `target` (named direct; NomKey via the
   reverse map → `num_classes + ind.index()`; else drop) and push `(Role::named(role),
   translated)` into `exists_seed[sub.index()]`. Reuses the exact translation in
   `precompletion_probe`. (The same `saturate` is already computed for `sat_seed`; compute both
   from one `saturate_with_exists_facts` call to avoid double saturation.) Flag-off ⇒ `None` ⇒
   zero cost.
2. **Seed it in `classify_labels` AND `decide_with_stats`**, alongside the existing `sat_seed`
   loop: when `exists_seed` is `Some`, push `DlClause { body: [Class(Q,X)], head: [Exists(role,
   target, X)] }` for each `(role, target)` in `exists_seed[class.index()]`. Both sites already
   use `HyperEngine::new` (full index rebuild) when seeded — the SP2.1 lesson; the ∃-clauses
   must be indexed to fire.
3. **Flag.** `RUSTDL_SAT_SEED` now drives **named + ∃ together** (the full coupled-saturation
   seed — what Phase-1 validated). Flag-off byte-identical.

## Selectivity — deferred (measured-only fallback)

The Chardonnay regression (named-only 16 s → named+∃ 42 s) is **invisible at the production
adaptive label-cache deadline** (~3.4 s at 25 ms/pair): named-only already times out at 16 s ≫
3.4 s, so it is a miss either way; the ∃ version (42 s) also exceeds 3.4 s → still a miss.
Meanwhile CabernetFranc goes DNF→0.2 s, so it now **labels within the deadline** (was a miss).
So at the production deadline the ∃-seed converts collapsible hard-tail classes from misses to
labeled while the genuinely-too-slow classes stay misses regardless → expected **net-positive**.
**Selectivity (e.g. named-only-first, ∃-on-timeout) is NOT built up front** (YAGNI); it is the
documented remedy **only if** Task 2 measures a net-negative wine wall.

## Gate (the proof — controller-run)

1. **FP=0 / MISSED=0 byte-identical, full corpus, flag ON.** `konclude_closure_diff` on every
   oracled fixture (bibtex, sulo, galen, notgalen, ro, ore-10908, alehif, sio, ore-15672,
   pizza, wine), each `rustdl_closure = konclude_closure`, FP=0, MISSED=0, unsat counts equal.
   **Wine is critical** (653=653, unsat:rustdl=0) — the classify-scale soundness Phase-1 could
   not test (per-pair `¬sup` × seeded ∃-structure across 137² pairs). Tight per-pair deadline
   (25 ms) reproduces FP signals fast.
2. **Flag-off byte-identical** to the integration branch base.
3. **Net wine classify wall, flag ON vs OFF** (label-cache build + tier walk breakdown). The
   real number: does the hard-tail collapse net-reduce the wall? Compare against SP2.1's
   named-only ~7.5%.

**Verdict:** GO (ship / flip default-ON or opt-in) iff FP=0/MISSED=0 byte-identical corpus-wide
AND a net wine wall improvement beyond SP2.1's ~7.5%. Any FP/MISSED ⇒ NO-GO (the classify-scale
∃-coupling has a hole the probe couldn't see — diagnose, a day not a sub-project). Net-negative
wall ⇒ add selectivity (the fallback above) and re-gate.

## Scope / non-goals

- Production wiring + corpus FP gate + net wall **only**. No selectivity unless measured-needed.
  No new saturation rules (the saturator already derives wine's ∃-facts).
- EL/Horn fast-path untouched (routes to the saturator, never the seeded wedge).
- Default OFF; `main` untouched; branch `feat/sat-precompletion-sp3-prod` off
  `feat/sat-precompletion-probe`. The probe (`precompletion_probe` + gate) stays as the
  per-class diagnostic.

## Global constraints

- FP=0 sacred — the **full-corpus** closure-diff (FP=0 **and** MISSED=0, byte-identical) is the
  gate, before any default flip; wine is the critical fixture.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` (pedantic) clean; `cargo test --workspace` green (flag-off).
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable bin
  on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` +
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

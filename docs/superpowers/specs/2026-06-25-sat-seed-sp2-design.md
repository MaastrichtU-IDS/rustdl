# SP2 — coupled-saturation named-subsumer seed in the classify per-pair path (design)

**Status:** design. SP2 of the coupled-saturation engine
(`docs/superpowers/specs/2026-06-23-coupled-saturation-tableau-design.md`). The
mechanism is probe-validated (`docs/sat-seed-probe-results-2026-06-25.md`); SP2 wires it
into the real classify path and gates it at corpus scale.

## Goal

Wire the validated **named-subsumer seed** into the per-pair classify wedge so the
per-class collapse the probe measured (Zinfandel DNF→2.6 s/Sat, ~21×; SweetWine 4.8×)
applies during real classification — and **prove it sound at scale** (FP=0/MISSED=0
byte-identical corpus-wide). Default OFF; the corpus gate decides ship/flip.

## Mechanism (probe-determined)

Seed the wedge root for each per-pair test `sat(sub ⊓ ¬sup)` with `Q → D` for every
**named** all-model saturated subsumer `D` of `sub` (`owl_dl_saturation::saturate`).
Seeded subsumers propagate through `horn_fixpoint` and cascade determinism into the
downstream disjunction search, collapsing it. **Synthetic IDs (NomKey / ForallKey / DKey
/ Tseitin, index ≥ `vocabulary.num_classes()`) are filtered** — forcing those cross-engine
is a spurious-clash FP (the probe hit Zinfandel→spurious-Unsat before filtering).

## Soundness model

Seeding `sub`'s **entailed named subsumers** is **monotone**: adding facts true of `sub`
(`sub ⊑ D`, with `D` a named class meaning the same in saturator and wedge) cannot turn a
satisfiable `sub ⊓ ¬sup` unsatisfiable, nor an unsatisfiable one satisfiable. So FP=0 and
MISSED=0 hold by construction. **But this is exactly the "sound by construction" sentence
shape that preceded four FP-graveyard NO-GOs this session — the corpus oracle is the proof,
not the argument.** The classify-scale surface the probe did NOT test: per-pair `¬sup`
injection interacting with seeded labels across all 137² pairs. The gate runs the full
corpus, FP=0 **and** MISSED=0, byte-identical.

## Architecture (3 pieces)

1. **Compute saturation once per ontology.** `HyperCache::build(&internal)` (already built
   once, reused across the O(n²) pairs) gains, **when the flag is on**, a field
   `sat_seed: Option<Vec<Vec<ClassId>>>` indexed by class id: `sat_seed[c]` = the named
   saturated subsumers of `c` (`saturate(&internal).subsumers_of(c)`, filtered to
   `d != c && d.index() < num_classes`). Flag-off ⇒ `None` ⇒ zero cost (no `saturate`
   call). The table is the one-time saturation amortized across every pair.
2. **Seed each per-pair call.** `decide_with_stats(sub, sup, …)`: if `self.sat_seed` is
   `Some(tbl)`, push `DlClause { body: [Q], head: [Class(d, X)] }` for each
   `d ∈ tbl[sub.index()]`, exactly as the probe does. No change when `None`.
3. **Flag.** `hyper_sat_seed_enabled()` (default OFF:
   `std::env::var_os("RUSTDL_SAT_SEED").is_some_and(|v| v != "0" && !v.is_empty())`),
   threaded at `HyperCache::build`. Flag-off path byte-identical to the integration branch.

## The gate (the proof — controller-run)

1. **FP=0 / MISSED=0 byte-identical, full corpus, flag ON.** `konclude_closure_diff` on
   every oracled fixture (bibtex, ro, ore-15672, pizza, galen, notgalen, ore-10908, sio,
   wine) — each `rustdl_closure = konclude_closure`, FP=0, MISSED=0, **unsat counts equal**.
   Wine is the critical one (653=653, unsat:rustdl=0): it is where seeding fires hardest
   and where a classify-scale FP would surface. Tight per-pair deadline (25 ms) reproduces
   FP signals fast, as in prior gates.
2. **Flag-off byte-identical** to the integration branch base (same fixtures).
3. **Wine classify wall-time, flag ON vs OFF.** Does the per-class collapse compound into a
   classify speedup? (Honest: the probe showed 2.6 s *per hard class* with the current
   saturator; full classify wall is the real number. Magnitude is informative, not the
   gate — the gate is FP=0/MISSED=0.)

**Verdict:** GO (flip default-ON, or at least ship opt-in + greenlight the SP1 increments
with a measured payoff target) iff FP=0/MISSED=0 byte-identical corpus-wide AND a wine wall
improvement. Any FP or MISSED ⇒ the classify-scale coupling has the hole the probe couldn't
see ⇒ NO-GO, diagnose (cost: a day, not a sub-project).

## Scope / non-goals

- SP2 is **wiring + the corpus FP gate only.** It does NOT add saturation rules (the SP1
  increments — `∀`/`≤n`/nominal — are gated on SP2 clearing, with this as their payoff
  harness). It does NOT touch the EL/Horn fast path (those route to the saturator and never
  hit the seeded wedge).
- Default OFF; `main` untouched; branch `feat/sat-seed-sp2` off `feat/nominal-rearch-sp0`.
- The probe code (`seed_probe` + `tests/seed_probe_gate.rs`) stays as the per-class
  diagnostic; SP2 is the classify-path production wiring.

## Global constraints

- FP=0 sacred — the **full-corpus** closure-diff (FP=0 **and** MISSED=0, byte-identical) is
  the gate, run before any default flip; wine is the critical fixture.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` (pedantic) clean; `cargo test --workspace` green (flag-off).
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable
  bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  + `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

# SP3 Phase-1 — precompletion-graph coupling viability probe (design)

**Status:** design. Phase-1 (gating probe) of the precompletion-graph coupling — the
amplifier beyond the SP2 named-subsumer seed (`docs/sat-seed-sp2-gate-results-2026-06-25.md`).
Throwaway-allowed; the verdict doc is the durable deliverable. Phase-2 (production) is a
separate spec, gated on this probe.

## Why

SP2's named-subsumer seed soundly collapses ~5 of wine's ~25 hard classes but hit a
**~7.5% ceiling**: the saturator's *named* closure is already complete on wine (653=653),
so seeding more named subsumers adds nothing, and named subsumers alone don't constrain the
**value-assignment** model search for the other ~20 hard classes. The saturator *also*
derives the deterministic **∃/value facts** that resolve exactly those choices —
`Zinfandel ⊑ ∃hasColor.{Red}` (forced by `∀hasColor.OneOf{…} ⊓ ≤1 hasColor`). Seeding
*those* is the precompletion coupling. The probe decides, cheaply, whether it works before
the FP-delicate production build.

## The load-bearing question the probe answers

Does seeding the saturation's derived ∃-facts into the wedge **collapse a hard
non-collapsing wine class** (one the named-seed leaves slow), with the **verdict preserved
(stays Sat)** — and can the **nominal-target translation** (NomKey → wedge-native nominal)
be done soundly? GO iff yes; otherwise the named-seed ~7.5% is the coupled-saturation wine
ceiling.

## Mechanism (probe)

1. **Expose** from `owl_dl_saturation`: the derived ∃-facts (`seen_facts` / `facts`:
   `(sub, role, target)`) as a queryable accessor (e.g. `Subsumers`-adjacent
   `exists_facts_of(c) -> Vec<(RoleId, ClassId)>`), plus the **NomKey → individual reverse
   map** (the saturator builds NomKey per individual at lowering; expose the inverse).
2. **Translate** each derived `(R, target)` of the probed class to a wedge-native head
   atom:
   - `target` is a **named** class (`index < num_classes`) → `∃R.target` directly.
   - `target` is a **NomKey** synthetic → map back to its individual `a`, then to the
     wedge's clausal nominal class for `{a}` (the same id the clausifier uses for
     `ObjectOneOf({a})`); seed `∃R.{a}`.
   - `target` is any other synthetic (Tseitin/DKey) → **drop** (untranslatable; sound
     under-approximation — fewer seeds, never a wrong one).
3. **Seed** `Q → ∃R.target` (an `Atom::Exists` head clause) into the wedge for the probed
   class, IN ADDITION to the named-subsumer seed, then run `sat(class)` with the full index
   rebuild (as the seed probe does).
4. **Measure** on a **hard non-collapsing** wine class (pick the worst: measure named-seed
   branch counts across the ~25 hard classes, choose one still in the hundreds-of-k):
   - branches with ∃-seed vs named-seed-only vs none;
   - **verdict preserved == Sat** (the soundness check — wine classes are satisfiable);
   - **control:** a garbage ∃-seed (random R/target) must NOT produce a correct fast Sat
     (mirrors the named-seed garbage control that caught the synthetic-ID FP).

## Soundness

Derived ∃-facts are **all-model entailed** (the saturator is sound: `c ⊑ ∃R.target` holds in
every model), so seeding them is **monotone** — adding an entailed witness cannot flip
Sat↔Unsat. This is the same soundness basis as the named seed, and distinct from the
snapshot-cache reuse-trap (which replayed *one model's* labels). **The FP-delicate part is
the nominal-target translation:** the seeded `∃R.{a}` must mean the same `{a}` in the wedge
as the saturator's NomKey. The probe's verdict-preservation check (and Phase-2's full-corpus
FP=0/MISSED=0) is the proof, not this argument — four "sound by construction" claims were
corpus-refuted earlier this project.

## Decisive outcomes

- **GO** — ∃-seed collapses the hard class (order-of-magnitude below named-seed-only) AND
  verdict stays Sat AND the garbage control does not collapse-to-correct-Sat. → spec Phase-2
  (wire ∃-seeding into `classify_labels` + full-corpus FP=0/MISSED=0 gate + wine wall).
- **NO-GO** — ∃-seed doesn't collapse, or verdict flips (translation unsound and not
  cheaply fixable). → the named-seed ~7.5% is the coupled-saturation wine ceiling; record it.

## Scope / non-goals

- Phase-1 **probe only** (a `precompletion_probe` fn + an `#[ignore]` gate test, mirroring
  `seed_probe`/`seed_probe_gate`). Throwaway code; the verdict doc
  `docs/sat-precompletion-probe-results-2026-06-26.md` is durable. The saturator accessor
  (derived ∃-facts + NomKey reverse map) is the one keep-on-GO piece.
- Phase-2 (production wiring into `classify_labels` + the corpus FP gate) is a separate spec,
  gated on this probe.
- Branch `feat/sat-precompletion-probe` off `feat/sat-seed-sp2`. Default OFF / probe-only;
  `main` untouched.

## Global constraints

- Soundness: verdict-preserved on the probed class is the Phase-1 check; full-corpus
  FP=0/MISSED=0 is Phase-2's gate (not claimed here).
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` (pedantic) clean; `cargo test --workspace` green.
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable
  bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  + `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

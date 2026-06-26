# Stage-4 — hard-class diagnostic (capturable vs genuine) design

**Status:** design. The gating first sub-project of Stage-4 (the engine frontier to bring
wine's ~891 wedge-hard classes from 3.2 s toward Konclude's ~114 ms). This is a
**measurement**, not an engine build — it decides *which* Stage-4 to build. Throwaway-allowed;
the verdict doc is the durable deliverable.

## Why a diagnostic first

Wine is at a sound near-optimum of 3.2 s (∃-seed + tight cap, FP=0/MISSED=0). The residual is
~891 classes the wedge can't label even at a 30 s budget (post-∃-seed). The ∃-seed proves
that **saturation-captured determinism collapses hard classes soundly** (CabernetFranc
DNF→209 ms). The open question — which defines what Stage-4 *is*, and has the 8-NO-GO frontier
behind it — is whether the 891's hardness is:

- **(1) UNCAPTURED determinism** — the saturator could derive the deterministic ∃-facts that
  collapse them but doesn't yet (richer `∀`/`≤n`/nominal rules; or ∃-targets the current
  translation *drops*). → Stage-4 = a **bounded, sound saturation extension** (extends the
  proven ∃-seed; achievable; may reach ~sub-second).
- **(2) GENUINE nondeterminism** — real disjunctive choice no all-model fact captures. →
  Stage-4 = the **deep search-engine rearchitecture** (integrated nominal handling / per-test
  tree-shrinking — Konclude has it, the wedge lacks it; large, FP-critical, but Konclude
  proves achievable).

This is **not** a re-run of SP-0's shadow-dep probe: SP-0 measured the *pre-∃-seed* world and
found the dep chains genuine *for backjumping*. The ∃-seed changed the landscape; "how much of
the 891 is *still* capturable by all-model saturation" is unmeasured.

## The diagnostic — two probes on the 891 hard classes

**Probe A — richer saturation.** The saturation increments 1 (`∀R.C` propagation) + 2
(qualified `≤1 R.C` witness merge) exist on branch `feat/saturator-forall-propagation`
(commits `0b1a702`, `1f69b43`) and derive *more* deterministic ∃-facts. Fold them into the
`saturate_with_exists_facts` pipeline (cherry-pick onto the Stage-4 branch), rebuild the
∃-seed table, and re-measure the wine label-cache misses under identical conditions
(`--pair-timeout-ms 1`, ∃-seed on, adaptive + a generous-deadline sweep).
- **Substantial miss drop (esp. in the 891)** ⇒ **capturable**: richer saturation derives the
  missing determinism → Stage-4 is a sound saturation extension.
- **Flat** ⇒ the increments don't capture it (consistent with their "corpus-invisible on wine"
  note for *subsumers* — but ∃-facts are the new test) → leans **genuine**.

**Probe B — dropped ∃-targets.** Instrument `saturate_with_exists_facts` (or the seed
translation) to count, per class, how many derived ∃-facts are **dropped** as untranslatable
(Tseitin/DKey targets, not named, not NomKey) vs kept. For the 891 hard classes:
- **High dropped-count** ⇒ **capturable-in-principle**: the determinism *is* derived but
  thrown away by the translation; recovering it (a richer target translation — e.g. decode
  compound Tseitin filler structure into wedge clauses) would seed it.
- **Low dropped-count** (the 891 simply have few derived ∃-facts) ⇒ the saturator isn't
  deriving their determinism at all → leans **genuine** (or needs Probe-A's richer rules).

## Verdict (pre-committed) → Stage-4's true scope

- **CAPTURABLE** (Probe A drops misses, and/or Probe B shows recoverable dropped ∃-facts) →
  Stage-4 = **bounded sound saturation/translation extension** (next spec: fold the increments
  + richer ∃-target translation into the production ∃-seed, FP=0/MISSED=0 gate + net wall).
  Achievable; extends the proven win; may reach ~sub-second on wine.
- **GENUINE** (both probes flat) → the 891 need the **deep engine rearchitecture**. That is a
  fresh, **∃-seed-era** go/no-go on the frontier (the stale pre-∃-seed NO-GOs don't bind) —
  scoped as its own large, FP-critical sub-project (integrated nominal + completion-graph
  reuse), committed to with eyes open, not as a quick continuation.
- **MIXED** (some capturable, some genuine) → do the bounded extension first (banks the
  capturable fraction soundly), then decide the deep engine for the residual with the
  sharpened number.

## Scope / non-goals

- **Diagnostic only** — Probe A is a cherry-pick + re-measure; Probe B is read-only
  instrumentation + a count. No production wiring, no engine change, no default change. The
  durable deliverable is `docs/stage4-hardclass-diagnostic-results-2026-06-26.md`.
- Probe A's cherry-picked increments are **not merged** by this sub-project (only measured); if
  CAPTURABLE, the production fold is the next spec.
- Branch `feat/stage4-hardclass-diagnostic` off `feat/sat-precompletion-sp3-prod` (has the
  ∃-seed). `main` untouched.

## Global constraints

- This is a measurement; any code (Probe-B instrumentation, the cherry-pick) is throwaway and
  must keep `cargo build`/existing tests green. Soundness of the *measured* configs is checked
  by the wine closure-diff (653=653) where a config is run end-to-end.
- `cargo fmt --all -- --check`; `cargo clippy` clean on touched crates.
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable
  bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` +
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

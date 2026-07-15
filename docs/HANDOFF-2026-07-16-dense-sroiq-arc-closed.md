# HANDOFF — dense-SROIQ deep-R&D arc CLOSED (2026-07-16)

Supersedes `docs/HANDOFF-2026-07-15-dense-sroiq-fix2.md` for resume purposes. Self-contained.

## TL;DR

The dense-SROIQ completeness/wall tail (exemplar `ore_ont_10019`: 47 classes, ~33
wedge-stalled; Konclude 90 ms / HermiT 360 ms) is **fully explored and measured out**.
No sound, corpus-visible win was found; every lever is closed with evidence. All work
is **sound, default-OFF, committed** on `feat/wedge-semantic-branching` (nothing pushed
— shared org repo, user's call). The branch is a banked, merge-ready capability +
diagnostics; **do not flip any of these flags default-ON** (all are corpus-invisible or
a completeness trade).

## What was built this session (all on `feat/wedge-semantic-branching`)

1. **Fix #2 Layer A** — in-search disjointness pruning + unit-forcing at the wedge `⊔`
   decision (`RUSTDL_SEMANTIC_BRANCHING`, default OFF). **Verdict-preserving**; found+fixed
   2 latent FP hazards (survivors-remain backjump dep-subset — pizza 2→14 spurious unsat;
   landing-node merge resolution). Commits `f13550e..5f65be4`, `9fe0fa9`. Findings
   `docs/2026-07-15-semantic-branching-findings.md`.
2. **Fix #2 Layer B** — per-node exclusion set (exclude a clean-`Unsat` sibling's class →
   sound case-split). Commit `dd77605`. Sound (curated + non-Horn `ore_ont_13723` oracle
   FP=0; pizza 31 376 exclusions byte-identical). **GO/NO-GO = NO-GO**: `ore_ont_10019`
   byte-identical OFF-vs-ON despite 77 842 exclusions — collapses local disjunctions but
   not the whole-graph H2 thrash. Findings `docs/2026-07-15-semantic-branching-layerB-findings.md`.
3. **Bound-the-tail** — sound divergence-keyed skip of the wedge-stall→tableau fallthrough
   (`RUSTDL_BOUND_DIVERGED_TAIL`, default OFF) + `# fallthrough:` diagnostic counters.
   Commits `5e039eb`, `1f93c70`. **Phase 0 rescue-rate measurement CLOSED it**: the
   fallthrough rescues **11 real subsumptions** on `ore_ont_10019` (all deadline-stalls, 0
   divergence-stalls) — it is NOT redundant. The −44 % wall "reclaim" (skip all
   fallthroughs) silently drops those 11 → a completeness trade, not free. Findings
   `docs/2026-07-15-bound-the-tail-findings.md` + `docs/2026-07-16-fallthrough-rescue-rate.md`.

## The arc, fully measured out

- **Fix #1** backjump-precision — ruled out (bit-identical `bjgap`, prior session).
- **Fix #2** semantic branching (Layer A+B) — sound, **NO-GO** (decides 0 extra).
- **Bound-the-tail** — sound form inert (divergence fires on only 2 of 1265 stalls, after
  the deadline); the wall reclaim costs +11 MISSED.

**Only remaining closer for the dense-SROIQ disjunctive tail:** Konclude-class
whole-model caching / CDCL clause-learning. **Deferred** — reuse-trap FP surface
(`snapshot-cache-fp-soundness-fix`, `reuse-trap-A1`), multi-month, and a prior NO-GO
without explicit cost acceptance. Do not start it unprompted. The aggregate deadline
(`RUSTDL_AGGREGATE_DEADLINE_MS` / `--global-timeout-s`) is the sound wall-bound.

## Verification (this machine)

- Full workspace `build` + `clippy -D warnings` + `fmt --check` = **clean**.
- Full workspace `cargo test` = green **except** `incremental_matches_baseline_on_fixtures`
  (pre-existing; fail-louds because the gitignored `ontologies/regression/funcmerge-cyclic.ofn`
  is not fetched here — red on the base commit too, unrelated to this branch).
- Soundness gate (Fix#2 A+B): curated byte-identical OFF-vs-ON (galen/notgalen/sio/wine/
  ore-15672/ore-10908/alehif/pizza incl. `INVERSE_FUNC_MERGE=0`) + non-Horn `ore_ont_13723`
  Konclude oracle FP=0/MISSED=0.

## Env / resume (machine-local, NOT in git)

- **Toolchain:** cargo via `$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`
  (prefix PATH each shell; this satisfies the `RUSTUP_TOOLCHAIN=stable` requirement).
  Rebuild `-p owl-dl-cli -p owl-dl-bench` before any CLI/matrix run (stale-binary trap).
- **ORE data:** `/data/dumontier/ore-run/work/sym/ore_ont_{10019,13723,10080}.ofn`;
  Konclude oracle `/data/dumontier/ore-run/pilot/ore_ont_13723.owl/kon.owx`.
- Curated corpus: `./scripts/fetch-real-ontologies.sh` (gitignored; note
  `ontologies/regression/` is NOT fetched by it — hence the one failing test above).

## Open / carry-forward (NOT dense-SROIQ; need user direction)

- **Integration decision for `feat/wedge-semantic-branching`** still pending: keep-as-is
  (banked), push+PR, or merge-to-main-local (all flags default-OFF → no behaviour change
  on main). Nothing pushed yet.
- Pre-existing (from the 2026-07-15 handoff): correct the paper (`~/code/rustdl-paper`)
  framing to "sound; near-complete (hard dense-SROIQ tail Konclude clears)".

## Process notes (kept working this session)

- **Measure-first repeatedly paid off:** the shipped divergence flag *looked* right and
  was inert; the bound-tail −44 % *looked* free and cost 11 MISSED — both caught by a
  cheap measurement before shipping a risky knob. SB_VERIFY-style ground-truth probes +
  Config A–E bisection found the Layer-A survivors-remain FP that reasoning missed.
- **The `advisor` tool was the reliable independent soundness reviewer** — the opus
  reviewer/implementer subagents hit stream-idle timeouts repeatedly (a session-env
  issue), so the controller implemented directly and the advisor caught 2 latent FP
  hazards + scoped the bound-tail exit condition correctly.

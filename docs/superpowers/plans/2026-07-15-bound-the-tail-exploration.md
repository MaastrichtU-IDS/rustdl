# Bound-the-tail — exploration plan (dense-SROIQ wall reclamation)

**Motivation:** `docs/2026-07-15-bound-the-tail-findings.md` — ~half the dense-SROIQ
classify wall is the main-tableau (`search.rs`) fallthrough re-thrashing pairs the
wedge already stalled on (`ore_ont_10019`: 77.7 s → 43.4 s if all fallthroughs are
skipped). The sound divergence-keyed skip (`RUSTDL_BOUND_DIVERGED_TAIL`, shipped
5e039eb, default-OFF) is INERT there (`is_diverging` fires only at *saturated* depth,
after the per-pair deadline). Reclaiming the −44 % needs either (A) skipping the
fallthrough on deadline-`Stalled` too — completeness-risky — or (B) making divergence
detectable pre-deadline so the sound skip fires. This plan explores both, **decided by
one measurement**.

**Branch:** `feat/wedge-semantic-branching` (or a fresh branch off it). All flags
default-OFF; flip only after the stated gate is green in a separate commit.
**Toolchain:** `RUSTUP_TOOLCHAIN=stable cargo …` via `$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`; rebuild `-p owl-dl-cli -p owl-dl-bench` before any matrix/measure (stale-binary trap).
**ORE data:** `/data/dumontier/ore-run/work/sym/ore_ont_{10019,13723,10080}.ofn`; oracle `/data/dumontier/ore-run/pilot/ore_ont_13723.owl/kon.owx`.

---

## Phase 0 — the pivotal measurement: fallthrough rescue rate (decides the whole plan)

**Question:** how often does the main-tableau fallthrough, AFTER a wedge `Stalled`,
actually find a subsumption the wedge missed (`Subsumed`) vs. also stalling/not-
subsuming? If ~0 corpus-wide, skipping the fallthrough is **MISSED=0-safe** and the
cheap −44 % path (Phase 1) is viable. If >0 on some ont, skipping regresses those →
Phase 2 (earlier divergence) is the only sound route.

- [ ] **Step 0.1 (instrument, diagnostic-only):** in `subsumes_via_tableau`, when the
  wedge verdict is `Unknown`/`UnknownDiverged` (i.e. the fallthrough runs), record a
  counter split: `fallthrough_ran`, `fallthrough_subsumed` (tableau returned Subsumed),
  `fallthrough_notsubsumed`, `fallthrough_noverdict`. Also split by divergence-vs-
  deadline stall. Surface in the CLI banner (`# fallthrough:`). No behaviour change.
- [ ] **Step 0.2 (measure curated + ORE):** run classify on every hybrid ont
  (sio, wine, ore-10908, ore-15672, alehif, shoiq-knowledge) + `ore_ont_10019`,
  `ore_ont_13723`, `ore_ont_10080`. Record the rescue rate
  (`fallthrough_subsumed / fallthrough_ran`) per ont and whether the rescues are on
  deadline-`Stalled` or divergence-`Stalled` pairs.
- [ ] **Step 0.3 (decision):**
  - **Rescue rate ≈ 0 corpus-wide** → the fallthrough almost never rescues a wedge
    stall → **Phase 1** (skip/share-budget is MISSED=0-safe on the corpus; still
    off-corpus-risky, so keep default-OFF + a full-corpus gate).
  - **Rescue rate > 0 on some ont** → identify which onts/patterns (likely defined-sup
    / functional + ≥n). Those pairs MUST keep the fallthrough → **Phase 2** (only cut
    the provably-diverging pairs), or a pattern-scoped skip that spares the rescuing
    patterns.
  - Either way, write `docs/2026-07-16-fallthrough-rescue-rate.md`.

---

## Phase 1 — cheap path: honest per-pair budget (only if Phase 0 rescue ≈ 0)

The current `effective_deadline` gives the fallthrough a FRESH `now + per_pair`, so a
pair can burn 2× its budget (wedge + tableau). Two variants, measure both:

- [ ] **Step 1.1 (budget-sharing, the principled framing):** thread the wedge's
  already-computed deadline into the fallthrough so a pair gets ONE `per_pair` budget
  total. On a hard pair the wedge consumes it → the tableau gets ~0 → immediate
  NoVerdict → not-subsumed fast. Behind default-OFF `RUSTDL_SHARE_PAIR_BUDGET`.
  Framing: this is arguably a **correctness fix** (`--pair-timeout-ms` should bound the
  PAIR, not each engine call), which is easier to justify than a blanket skip.
- [ ] **Step 1.2 (deadline-keyed skip, the aggressive variant):** extend the
  `RUSTDL_BOUND_DIVERGED_TAIL` arm to also skip on plain deadline-`Stalled`
  (`HyperVerdict::Unknown`), gated by a distinct value (e.g. `=2` = "diverged+deadline").
- [ ] **Step 1.3 (GATE — full-corpus MISSED=0, non-negotiable):** run the full curated
  `konclude_closure_diff` suite + the ORE oracle onts, flag ON, at the standard budget.
  **FP=0 is trivial; the gate is MISSED=0 vs OFF** (the cut must not drop any curated
  subsumption). Any new MISS → the variant is not corpus-safe → do NOT ship; fall to
  Phase 2. Record the `ore_ont_10019` / `ore_ont_10080` wall win.
- [ ] **Step 1.4 (decision):** if MISSED=0 corpus-wide AND a material wall win → ship
  the winning variant default-OFF (opt-in; document the off-corpus completeness risk).
  Flip default-ON only with explicit user sign-off (it is a completeness trade). Write
  findings.

---

## Phase 2 — sound path: earlier divergence detection (if Phase 0 rescue > 0, or Phase 1 fails MISSED=0)

Make the *sound* divergence-keyed skip (already shipped) actually fire on
`ore_ont_10019` by detecting the thrash before the per-pair deadline. The stall there
is "thrashing through a tiny state set at STABLE node count / UNSATURATED depth"
(memory: `e-interaction` revisits ~14 states ~40 k×), which the current
`is_diverging` (requires `depth_saturated`) misses.

- [ ] **Step 2.1 (characterize):** instrument the wedge on `ore_ont_10019`'s stalled
  pairs — branches, restores, max_branch_depth vs cap, node-count stability, and any
  state-revisit signal. Confirm the thrash is unsaturated-depth + high revisit (the
  memory's hypothesis) rather than genuine progress.
- [ ] **Step 2.2 (candidate signals, measure each):**
  - (a) Lower `DIV_WINDOW` (500 → 200/100) — cheapest; but memory flags it MISSED-risky.
  - (b) Add an unsaturated-depth thrash clause to `is_diverging`: fire when
    `restores≈branches` over the window at STABLE node count even below the depth cap
    (the node-growth clause was previously dropped precisely because growth is stable —
    so "stable node count + all-failing" is the real divergence signature here).
  - (c) A bounded state-revisit counter (approx, cheap) that trips when the same
    label-signature recurs beyond a threshold.
- [ ] **Step 2.3 (GATE — corpus MISSED=0, per adaptive-budget precedent):** each signal
  is verdict-preserving ONLY if it never cuts a real proof. Gate every candidate on
  curated MISSED=0 byte-identical (the discriminator is: a real subsumption proof
  terminates within the window; only genuine thrash is cut). Keep the tightest signal
  that stays MISSED=0 AND makes the divergence-keyed skip fire on `ore_ont_10019`.
- [ ] **Step 2.4 (decision):** if a MISSED=0 signal fires the sound skip on
  `ore_ont_10019` with a material wall win → ship (default-OFF → ON after corpus gate,
  this one is SOUND so default-ON is defensible). Else → the tail is genuinely deferred;
  document that whole-model caching / CDCL remains the only closer.

---

## Phase 3 — synthesis + honest close

- [ ] Consolidate Phase 0-2 into `docs/2026-07-16-bound-the-tail-results.md`: the
  rescue-rate data, which variant (if any) shipped, its gate result, and the residual.
- [ ] Update `docs/2026-07-15-bound-the-tail-findings.md` decision section + the
  `fix2-semantic-branching-nogo` memory.
- [ ] If nothing ships MISSED=0-with-a-win: state plainly that the dense-SROIQ wall tail
  is bounded only by the opt-in aggregate deadline, and the redundant-fallthrough
  reclaim needs the risky path the user must explicitly accept. A legitimate close.

## Notes / guardrails

- **Measure-first every phase** (the shipped divergence-keyed flag looked right and was
  inert — the pattern this session repeatedly taught). Phase 0 is the gate for the rest.
- **The gate is COMPLETENESS (MISSED=0), not soundness** — everything here only removes
  subsumptions, so FP=0 is trivial; don't spend the FP oracle on it.
- **Advisor before shipping any variant** — the completeness trade (Phase 1) and the
  divergence-signal tuning (Phase 2) are both the kind of subtle call the advisor caught
  twice this session.
- Prior-author constraint (do not silently violate): the bare per-pair path is
  deliberately unbounded to not cut completable-slow onts (ore-15672). Any default-ON
  flip that curtails the fallthrough overrides that choice — user sign-off required.

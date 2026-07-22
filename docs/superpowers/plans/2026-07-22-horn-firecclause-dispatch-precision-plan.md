# Plan: Horn `fire_clause` dispatch precision (label-cache-build match_body waste)

**Status:** proposed, for advisor review then delegation to Fable.
**Supersedes:** the non-Horn-trigger-index plan (2026-07-22), P0-refuted — wrong
call site. This re-aims at the site P0 found.
**Branch (to create):** `perf/horn-dispatch-precision` (off current `main`, v0.3.34).

## 1. Target (found by the prior P0 gate — measured, not inferred)

Fable's P0 (branch `perf/nonhorn-trigger-index`, NO-GO report) attributed the
`ore_ont_3215` wall by call site and phase:
- **The label-cache build is 81% of the wall** (102 s / 125 s; the tier-walk never
  runs — the global deadline expires during the label build).
- **`match_body` call site 3517 — the Horn `fire_clause` in `horn_fixpoint` —
  is 1.76 BILLION calls, 99.92% returning empty, ~40% of all user CPU**, ~700 ns
  each. The non-Horn loop (the prior plan's target, site 2759) was inert (0.0013%).

**Root cause (code-grounded, hyper.rs:1998 `process_event` + 1179 index build):**
each Horn clause is indexed under **every** body atom
(`x_trigger[c]`/`succ_trigger[c]`/`role_trigger[r]`). When a node gains ONE trigger
atom, `fire_clause(ci, n)` → `match_body` runs. On shared-conjunct ontologies
(3215's ~18k `C ≡ (∃R.D ⊔ D) ⊓ E` defs share conjuncts) a common atom triggers
thousands of clauses whose *other* body atoms are absent → the match fast-rejects.

**Key precision (verified this session):** `match_body` ALREADY fast-rejects on a
missing X-class (hyper.rs:3583, `for &c in plan.x_classes { if !node.has(c) return
empty }`). So the ~700 ns waste is **dispatch + call + plan-fetch + x_class-loop
overhead for a clause that was always going to reject** — NOT the join. The lever
is therefore **"don't dispatch a clause whose other body atoms are absent,"** not
"add a reject" (one exists). Whether the 700 ns is mostly reclaimable (call/dispatch
overhead) or mostly the unavoidable x_class check is the P0 question below.

## 2. Goal & non-goals

**Goal:** cut the dispatch+call overhead of empty `fire_clause`/`match_body`
attempts at the Horn site by making dispatch more selective — a clause is
"activated" only when the node plausibly carries ALL its X-class body atoms
(watched-atom / Rete-beta style), instead of firing on any one. **Verdict-preserving
by construction:** changes only WHEN `match_body` is attempted, never which clauses
fire or what they derive (`match_body` still verifies). Target: materially cut the
label-build 40%-CPU chunk on shared-conjunct onts; constant-factor, soundness-safe.

**Non-goals:**
- NOT the non-Horn loop (P0-refuted), CDCL, CB-SROIQ, or build-once/#37 (all NO-GO).
- NOT changing verdicts, disjunct selection, or the label cache's EXISTENCE (it is
  net-positive — disabling it is measured-negative; the lever is making its Horn
  fixpoint cheaper, not skipping it).
- NOT the prep/saturation phase (a separate unbounded cost; out of scope here).

## 3. Phase 0 — measurement GATE (do FIRST; STOP-and-report) — DECIDES THE BUILD

The prior P0 already established the target; this P0 validates that the *specific
fix* (all-X-class-present dispatch gating) reclaims the waste. Do NOT build until it
clears.

**OUTCOME METRIC FIRST (advisor — the load-bearing fix vs the prior draft):** 3215's
wall is PINNED at the deadline (125 s = 120 s budget + ~5 s; the label build never
completes and consumes the whole budget). So **wall-at-a-fixed-deadline CANNOT show
this win** — a 2× faster `fire_clause` just does 2× more work in the same 120 s, wall
stays ~125 s. The win metric MUST be **wall-to-completion** and/or **coverage
(classes/edges decided) at a fixed budget** — the same lens the label-cache A/B used
correctly this session (112 k edges @120 s vs 88 k @171 s). Every P0 run below and the
§5.6 gate use wall-to-completion / coverage, NEVER wall-at-deadline.

Instrument `fire_clause` / `process_event` (env-gated `RUSTDL_HORN_DISPATCH_PROBE`,
default off, byte-identical when off), tier-walk AND label-build phases tagged.
**Include an UNBOUNDED (or 1800 s) 3215 run** (single-thread, RSS watchdog) so the
run reaches completion — needed for both the wall-to-completion metric AND the
phase split at completion (Q5).
1. **Body-atom-count distribution** of clauses dispatched at 3517: how many have
   ≥2 body atoms (only those can be gated tighter)? Report the gate-able fraction.
2. **Missing-atom-TYPE + per-trigger-path split (advisor — sharpened).**
   `match_body` ALREADY X-class-rejects, so an X-class pre-check only reclaims
   empties whose MISSING atom is an X-class. For the 99.92% empties report: (i) the
   missing-atom type (X-class vs successor-class vs role), and (ii) the per-trigger-
   path counts — `x_trigger` (2028) vs `succ_trigger` fire-at-all-predecessors
   (2036, prime suspect: 3215's `∃R.D` gives real pred×succ-class fan-out) vs
   `role_trigger` (2062) vs `role_back_trigger` (2075). This decides WHICH Phase-1
   mechanism is needed (X-class gate helps only X-class-missing empties on the
   x_trigger path; succ/role-dominated empties need the analogous check on the
   successor/pred node, or mechanism (a) is only a partial fix).
3. **Overhead split of the ~700 ns:** how much is dispatch+call+plan-fetch
   (reclaimable by not-dispatching) vs the `x_classes`/verify loop itself (paid
   either way). If most is the unavoidable check, the ceiling is low.
4. **Projected win (in coverage/completion terms):** (gate-able empty fraction) ×
   (reclaimable-overhead fraction of the 700 ns) × (fire-clause self-time share at
   COMPLETION, Q5) → estimated wall-to-completion reduction. GO threshold ≥ ~15%
   wall-to-completion (or ≥ ~15% more coverage at a fixed budget).
5. **Phase split AT COMPLETION (advisor):** on the unbounded run, the label-build
   vs tier-walk vs unsat-probe wall split. `fire_clause`/`horn_fixpoint` runs in
   ALL of them — so the end-to-end ceiling is the SUM of their fire-clause shares,
   not just label-build's 40%. This makes the honest win larger than the bounded
   snapshot suggested — and is only measurable at completion (why Q's use unbounded).

Run on `ore_ont_3215` (flagship, unbounded) + `ore_ont_10908` (completes in 0.23 s —
a completion baseline; P0 showed weak-anchor waste there) + one more shared-conjunct
volume-tail ont. **NO-GO if** the empties aren't reclaimable by an all-present gate
(most are single-atom, or the missing atom / trigger path isn't gate-able), or the
700 ns is mostly the unavoidable check, or projected wall-to-completion / coverage
win < ~15%.

## 4. Phase 1 — watched-atom dispatch gating (only if P0 = GO)

Make a Horn clause "active" at a node only when it plausibly carries all its body
atoms. **The mechanism is CONDITIONAL on P0-Q2's missing-atom-type / per-trigger-path
result** — do not assume the X-class variant:
- **(a) Pre-dispatch all-atom check on the RIGHT node (surgical):** before calling
  `fire_clause` in the dominant trigger loop, cheaply check the clause's OTHER body
  atoms via precomputed per-clause metadata + `node.has`. If P0-Q2 says the empties
  are X-class-missing on the `x_trigger` path → check secondary X-classes at `n`. If
  they are succ-class/role-missing on the `succ_trigger` fire-at-all-preds path
  (2036, the likely bulk) → the check must be on the SUCCESSOR/pred node, not `n`
  (an X-class-at-`n` gate would not touch these — this is the advisor's key caveat).
  Skips the call+plan-fetch for doomed clauses. Smallest change; the right v1 for
  whichever path dominates.
- **(b) Watched-atom count-down (Rete-beta):** per (node, clause) track how many
  body atoms are still missing; dispatch `match_body` only when the count hits 0.
  Stronger (O(1) activation) but needs per-node×clause state — heavier, invasive to
  the trail/backtracking (state MUST be save/restored). Only if (a)'s ceiling is
  insufficient AND P0 justifies it.

Amortization: precompute per-clause body-atom metadata once at `ClauseIndexes`
build (alongside `x_trigger`/`match_plans`), shared via the existing `Arc`.

## 5. Correctness gates (mandatory)

1. **Byte-identical closures corpus-wide** (curated + ORE volume-tail sample);
   0 diffs — the gating is verdict-preserving by construction, so ANY diff is a bug
   (watch the trail/backtracking interaction if mechanism (b) is used — the
   per-node×clause state MUST be correctly save/restored, else a stale "active"
   count causes a missed fire = MISSED, or a spurious skip = MISSED; neither is an
   FP but both violate byte-identity).
2. Non-Horn oracle FP gate (`ore_ont_13723`) = 0.
3. Full workspace suite green; the wedge canaries (shadow_dep, backjump_precision,
   incremental_fixpoint_identity, etc.) green.
4. fmt + clippy `-D warnings` clean.
5. **EL non-regression** (galen) — the Horn fixpoint is the EL hot loop too; the
   gating must not tax it (galen has few shared-conjunct clauses, so the pre-check
   should be near-free — verify).
6. **The win — measured as wall-to-completion / coverage, NOT wall-at-deadline**
   (3215's wall pins at the budget; a real speedup shows only as faster completion
   or more classes/edges decided per fixed budget — see §3). Report: 3215
   wall-to-completion (unbounded) before-vs-after, and coverage (edges decided) at a
   fixed budget before-vs-after.

## 6. Delegation notes (Fable)

- Branch `perf/horn-dispatch-precision` off `main`. Do NOT push/merge.
- Toolchain: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; export RUSTUP_TOOLCHAIN=stable`. Confirm fresh `target/release/rustdl` before benchmarking.
- Reuse the prior P0 instrumentation on branch `perf/nonhorn-trigger-index`
  (`RUSTDL_NONHORN_PROBE` counters + phase markers) as a starting point — extend it
  to the fire_clause/body-atom-count attribution.
- **RUN IN A MEMORY-SAFE WAY:** cap concurrency and set `RAYON_NUM_THREADS=1` for
  prep-heavy onts — the prior session's 245 GB "11270 OOM" was multi-thread prep
  contention from concurrent probes (11270 completes in ~1.4 GB single-stream;
  verified 2026-07-22). One ont at a time under an RSS watchdog.
- TDD: the byte-identical gate on a small shared-conjunct fixture (gated-on vs
  gated-off → identical closure) before wiring the hot loop.
- Commit trailers (exact):
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7`
- **STOP-and-report at the Phase 0 gate.** A NO-GO is valid.

## 7. Risks

- **P0 = NO-GO:** the 700 ns may be mostly the unavoidable x_class check (not
  reclaimable by not-dispatching), or most empties may be single-X-class clauses
  (no tighter gate possible), or role/succ-triggered empties dominate (different
  fix). P0 catches this before the build.
- **Backtracking-state bug (mechanism b):** per-node×clause watched counts that
  aren't trail-save/restored correctly → byte-identity violation (MISSED-risk). The
  §5.1 gate catches it; prefer mechanism (a) unless (b) is justified.
- **Constant-factor only:** cuts the label-build wall on shared-conjunct onts; will
  not close the Konclude gap or help non-shared-conjunct tails. Frame honestly.

# SP2 Phase A, Task A2: `ore_ont_10019` depth-binned clash smoke test

**Date:** 2026-07-14
**Task:** SP2 Phase A / Task A2 of `.superpowers/sdd/task-A2-brief.md` — a
read-only measurement harness + findings note. **No engine/behavior change.**
Harness: `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs`.

## Necessary-not-sufficient smoke test — read this before the numbers

This task answers exactly one narrow question, per the brief's "empty-tail
kill gate": do `ore_ont_10019`'s stalled classes even have a non-trivial
*deep* clash tail (`branch_depth >= split_depth`)? If `n_deep ≈ 0` across the
stalled classes, there is nothing for a Phase B deep-tail subset-core prune
to act on, and Phase A should stop and pivot. If `n_deep` is substantial, that
is a necessary precondition for Phase B to be worth building — **it is not
sufficient**: a high `deep.reusable_nogood_frac` / `deep.revisit_frac` here is
corroborating but does not confirm SP2's subset-core no-good mechanism is
viable, and a low value of either would NOT by itself be a kill (these
`analyze()` metrics summarize the *label-set-hash* revisit/reuse pattern
recorded by the existing shadow-dep probe; they do not lower-bound whether a
node-local, per-solve-scoped core-keyed prune — Phase B's actual mechanism —
would fire, since Phase B scopes and keys nogoods differently than this
coarse aggregate). Likewise a large `bjgap_shadow` (backjumping already jumps
far) and a low `revisit_context_shared_frac` are *warnings* about the
reuse-trap that Phase B's per-solve scope + node-local oracle are specifically
designed to neutralize — not kills. The real go/no-go verdict on the
mechanism itself is Phase B, not this harness.

## Step 1: identify the stalled classes

```sh
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
./target/release/rustdl hyper-sat ~/data/ore-run/input/ore_ont_10019.ofn --per-class-timeout-ms 300
```

Re-run today (2026-07-14) on a freshly built binary — post SP1's
default-ON incremental `horn_fixpoint` (see
`docs/2026-07-13-ore_ont_10019-stall-findings.md`), so the branch counts and
depths are higher than the original SP0 measurement (deeper/cheaper search
within the same 300ms budget), but the class set and verdict counts (14 sat /
0 unsat / **33 stalled**) are unchanged:

```
# features: inverse=false nominal=false card=true
# classes:          47
# sat:              14
# unsat:            0
# stalled:          33
# max_depth_reached:142
# --- top classes by branching ---
#   Stalled wall=302.15ms branches=16622 (disj=16622 merge=0) restores=16622 depth=138 blk=967921/0  http://ontology.dumontierlab.com/SecondaryAmineGroup
#   Stalled wall=301.38ms branches=16601 (disj=5027 merge=11574) restores=16601 depth=97 blk=880246/0  http://ontology.dumontierlab.com/PrimaryAmineGroup
#   Stalled wall=302.25ms branches=16568 (disj=16568 merge=0) restores=16568 depth=138 blk=963835/0  http://ontology.dumontierlab.com/MethylGroup
#   Stalled wall=302.23ms branches=16389 (disj=16389 merge=0) restores=16389 depth=137 blk=951803/0  http://ontology.dumontierlab.com/CarbonAtom
#   Stalled wall=302.06ms branches=16323 (disj=16323 merge=0) restores=16323 depth=137 blk=946836/0  http://ontology.dumontierlab.com/SulfinicAcidGeneralGroup
#   Stalled wall=302.19ms branches=16308 (disj=16308 merge=0) restores=16308 depth=137 blk=946536/0  http://ontology.dumontierlab.com/SulfonicAcidGroup
#   Stalled wall=302.22ms branches=16299 (disj=16299 merge=0) restores=16299 depth=137 blk=945136/0  http://ontology.dumontierlab.com/SulfoxideGroup
#   Stalled wall=302.30ms branches=16279 (disj=16279 merge=0) restores=16279 depth=138 blk=958138/0  http://ontology.dumontierlab.com/KetoneGroup
#   Stalled wall=302.17ms branches=16275 (disj=16275 merge=0) restores=16275 depth=137 blk=959176/0  http://ontology.dumontierlab.com/OxygenAtom
#   Stalled wall=302.23ms branches=16224 (disj=16224 merge=0) restores=16224 depth=137 blk=938160/0  http://ontology.dumontierlab.com/Alkyl
#   Stalled wall=302.33ms branches=16212 (disj=16212 merge=0) restores=16212 depth=136 blk=954463/0  http://ontology.dumontierlab.com/EtherGroup
#   Stalled wall=302.19ms branches=16172 (disj=16172 merge=0) restores=16172 depth=138 blk=919452/0  http://ontology.dumontierlab.com/SulfonylHalideGroup
#   Stalled wall=302.31ms branches=16167 (disj=16167 merge=0) restores=16167 depth=137 blk=934670/0  http://ontology.dumontierlab.com/SulfonicAcidDerivativeGroup
#   Stalled wall=302.55ms branches=16090 (disj=16090 merge=0) restores=16090 depth=136 blk=930329/0  http://ontology.dumontierlab.com/AldehydeGroup
#   Stalled wall=302.25ms branches=16070 (disj=16070 merge=0) restores=16070 depth=136 blk=929038/0  http://ontology.dumontierlab.com/AcylBromideGroup
```

**Namespace:** `http://ontology.dumontierlab.com/` (grep-confirmed against the
`.ofn`'s class IRIs). **Stalled classes used for the harness:** the 15 top
classes-by-branching above — a representative sample of the 33 `Stalled`
classes, not exhaustive.

**`split_depth` choice:** these classes branch from depth 0 up to an observed
max of ~137-142. Midpoint between "shallow" (near the root, depth ~0) and
that observed cap: `(0 + 137) / 2 ≈ 68`, rounded to **`split_depth = 70`**.

## Step 2/3: harness + both runs

Harness: `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs` — for each of the
15 stalled classes, calls `owl_dl_reasoner::sat_class_probe(&ont, iri, 256,
Some(Duration::from_secs(30)))`, then over `stats.clash_records` prints
`shadow_measures::analyze` (aggregate) and
`shadow_measures::analyze_by_depth(records, 70)` (`n_shallow`/`n_deep`,
`deep.reusable_nogood_frac`, `deep.revisit_frac`,
`deep.revisit_context_shared_frac`, `deep.bjgap_shadow` histogram).

### Run A — asymptotic (`RUSTDL_ADAPTIVE_BUDGET=0`, per-class deadline runs to the full 30s)

```sh
RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable \
  cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture
```

Full test wall: 450.25s (15 × ~30s, as expected with the adaptive early-cut
disabled). Per-class results (verdict is `Stalled` for all 15 — none resolved
within 30s even with the adaptive early-cut off):

| class | branches | max_depth | clashes | n_shallow | n_deep | deep.reusable_nogood_frac | deep.revisit_frac | deep.revisit_ctx_shared_frac | deep.bjgap_shadow (min/median/p90/max/mean) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| SecondaryAmineGroup | 251160 | 256 | 23381 | 6 | 23375 | 0.9997 | 1.0000 | 1.0000 | 1/130/130/154/129.14 |
| PrimaryAmineGroup | 244348 | 256 | 30393 | 6 | 30387 | 0.9997 | 1.0000 | 0.9991 | 1/130/130/137/129.18 |
| MethylGroup | 254391 | 256 | 23488 | 6 | 23482 | 0.9997 | 1.0000 | 1.0000 | 1/130/130/154/129.14 |
| CarbonAtom | 264346 | 256 | 52410 | 6 | 52404 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/147/130.42 |
| SulfinicAcidGeneralGroup | 260443 | 256 | 35598 | 6 | 35592 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.50 |
| SulfonicAcidGroup | 257540 | 256 | 35206 | 6 | 35200 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.50 |
| SulfoxideGroup | 260718 | 256 | 35636 | 6 | 35630 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.50 |
| KetoneGroup | 297046 | 256 | 56715 | 6 | 56709 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/138/129.64 |
| OxygenAtom | 258188 | 256 | 25457 | 6 | 25451 | 0.9997 | 1.0000 | 1.0000 | 1/130/130/154/129.34 |
| Alkyl | 264085 | 256 | 36085 | 6 | 36079 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.51 |
| EtherGroup | 251378 | 256 | 23390 | 6 | 23384 | 0.9997 | 1.0000 | 1.0000 | 1/130/130/154/129.14 |
| SulfonylHalideGroup | 267533 | 256 | 36584 | 6 | 36578 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.51 |
| SulfonicAcidDerivativeGroup | 262922 | 256 | 35930 | 6 | 35924 | 0.9998 | 1.0000 | 1.0000 | 1/130/130/154/129.51 |
| AldehydeGroup | 282730 | 256 | 58027 | 6 | 58021 | 0.9999 | 1.0000 | 1.0000 | 1/130/130/154/129.64 |
| AcylBromideGroup | 256915 | 256 | 34116 | 19 | 34097 | 0.9999 | 0.9999 | 0.9993 | 1/130/130/130/129.41 |
| **Total** | | | | **103** | **542313** | | | | |

Full raw output: (excerpt — the run's `--nocapture` stdout in full is
reproduced in the harness's own doc comment invocation; the table above is a
complete transcription of every field for all 15 classes.)

### Run B — in-budget (adaptive budget default-ON, the shipping behavior)

```sh
RUSTDL_SHADOW_DEP_PROBE=1 RUSTUP_TOOLCHAIN=stable \
  cargo test -p owl-dl-reasoner --release --test sp2_nogood_gate -- --ignored --nocapture
```

Full test wall: 2.29s (adaptive budget cuts each class off after roughly
100-200ms — far short of the 30s deadline). All 15 classes still report
`Stalled`:

| class | branches | wall_ms | clashes | n_shallow | n_deep | deep.reusable_nogood_frac | deep.revisit_frac | deep.revisit_ctx_shared_frac | deep.bjgap_shadow (min/median/p90/max/mean) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| SecondaryAmineGroup | 1508 | 108 | 286 | 6 | 280 | 0.9714 | 1.0000 | 1.0000 | 1/95/154/154/90.42 |
| PrimaryAmineGroup | 2028 | 199 | 553 | 6 | 547 | 0.9835 | 1.0000 | 0.9488 | 1/97/137/137/89.71 |
| MethylGroup | 1508 | 108 | 286 | 6 | 280 | 0.9714 | 1.0000 | 1.0000 | 1/95/154/154/90.42 |
| CarbonAtom | 2032 | 144 | 413 | 6 | 407 | 0.9779 | 1.0000 | 1.0000 | 1/90/130/144/82.69 |
| SulfinicAcidGeneralGroup | 2018 | 164 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| SulfonicAcidGroup | 2018 | 164 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| SulfoxideGroup | 2018 | 165 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| KetoneGroup | 2034 | 140 | 217 | 6 | 211 | 0.9526 | 1.0000 | 1.0000 | 1/77/118/138/71.67 |
| OxygenAtom | 2014 | 168 | 428 | 6 | 422 | 0.9810 | 1.0000 | 1.0000 | 1/129/134/154/103.82 |
| Alkyl | 2018 | 168 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| EtherGroup | 1508 | 109 | 286 | 6 | 280 | 0.9714 | 1.0000 | 1.0000 | 1/95/154/154/90.42 |
| SulfonylHalideGroup | 2018 | 167 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| SulfonicAcidDerivativeGroup | 2018 | 165 | 346 | 6 | 340 | 0.9765 | 1.0000 | 1.0000 | 1/114/134/154/97.18 |
| AldehydeGroup | 2012 | 165 | 345 | 6 | 339 | 0.9764 | 1.0000 | 1.0000 | 1/114/134/154/96.86 |
| AcylBromideGroup | 2022 | 140 | 267 | 19 | 248 | 0.9879 | 0.9879 | 0.9061 | 1/57/130/130/63.47 |
| **Total** | | | | **103** | **5054** | | | | |

## Step 4: applying the (weak) gate

**`n_deep` is not empty in either run.** Every single one of the 15 stalled
classes has a large, non-trivial deep clash tail:

- Asymptotic (adaptive budget off, 30s/class): **542,313 deep clashes** across
  15 classes vs **103 shallow** — deep clashes are **>99.98%** of all recorded
  clashes for every class.
- In-budget (shipping default, ~100-200ms/class): **5,054 deep clashes**
  across 15 classes vs **103 shallow** — still **>97%** deep for every class,
  even under the tight adaptive-budget cutoff.

**Verdict: PROCEED to Phase B.** The empty-tail kill condition (`n_deep ≈ 0`)
does not hold — there is a large, robust deep clash tail for a Phase B
deep-tail subset-core prune to act on, present under both the asymptotic and
the shipping-default budget regime. Per Step 5 of the SP2 plan
(".superpowers/sdd/task-A2-brief.md"), Phase A does not empty-tail-kill; work
should continue to Phase B (the sound, default-OFF, node-local core-keyed
prune behind `RUSTDL_WEDGE_NOGOOD`).

### Notes on the other metrics (read as corroboration + warnings, not verdicts)

- **`deep.reusable_nogood_frac` is high** (0.95–0.9999 in both runs) and
  **`deep.revisit_frac` is ~1.0** almost everywhere. This is *corroborating*
  — consistent with a small number of distinct clashing label-sets being hit
  over and over at depth — but per the framing above, **it is not
  confirmation** that Phase B's actual node-local core-keyed mechanism will
  fire the way this aggregate suggests; had these numbers instead come back
  low, that would *still not have been a kill* (see the header section). The
  actual mechanism validation is Phase B's job.
- **`bjgap_shadow` is large** (median ~90-130, p90 up to ~154 depending on
  class/run) — backjumping (in the shadow, non-taint-collapsed dependency
  layer) is already reaching a long way back from the clash point. Per the
  brief, this is a **warning**, not a kill: it means Phase B's per-solve scope
  + node-local oracle need to be robust to a reuse pattern where the "same"
  clash keeps recurring at very different points in the backjump chain, which
  is exactly the reuse-trap concern Phase B's design is meant to neutralize.
- **`deep.revisit_context_shared_frac` is mostly 1.0 but dips slightly** on
  two classes: `PrimaryAmineGroup` (0.9488–0.9991 across the two runs) and
  `AcylBromideGroup` (0.9061–0.9993). Both are still high (>90%), so this is
  a minor **warning** to carry into Phase B's design — these two classes are
  worth re-checking first once the node-local oracle is built, since they are
  the ones where the revisit context is *least* uniformly shared.

## Files

- `crates/owl-dl-reasoner/tests/sp2_nogood_gate.rs` — the `#[ignore]`d harness.
- This note.

## Verification

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings`: clean.
- Both required invocations (asymptotic + in-budget) ran to completion,
  producing depth-binned output for all 15 stalled classes (tables above).

---

## Task B4 — direct measurement (2026-07-14, `feat/sp2-nogood`)

Fresh build of `owl-dl-cli` + `owl-dl-bench` (`RUSTUP_TOOLCHAIN=stable`, confirmed
real recompile). No source changed, no defaults flipped, no commit. Full report:
`.superpowers/sdd/task-B4-report.md`.

### Step 1 — curated matrix (`--pair-timeout-ms 1000 --global-timeout-s 120`)

All 8 ontologies (family, galen, pizza, ro, sio, sulo, trivial, wine) are
**FP=0 / MISSED=0 with the flag ON**, closures byte-identical OFF vs ON. GATE PASS
(no soundness regression, no over-prune). Flag-ON adds some wall (family 810→1520,
pizza 120→320, wine 60→470 ms); correctness unchanged.

### Step 2 — non-Horn FP oracle (ore_ont_13723)

`konclude_closure_diff::ore_one_closure_matches_oracle`: closure 10166=10166,
**FP=0 → 0** OFF vs ON, test ok both ways.

### Step 3 — ore_ont_10019 classify (`--pair-timeout-ms 250`, aggregate deadline 60 s)

| nogood | adaptive_budget | incomplete | direct lines | wall |
|--------|-----------------|-----------|--------------|------|
| OFF | 0 | 1579 | 59 | 60.03 s |
| ON  | 0 | 1579 | 59 | 60.02 s |
| OFF | 1 | 1459 | 63 | 60.01 s |
| ON  | 1 | 1451 | 61 | 60.01 s |

- **All configs pin the 60 s aggregate deadline** ⇒ deadline-bound, non-deterministic
  (rayon scheduling decides which pairs finish first).
- adaptive_budget=0: hierarchy **byte-identical** OFF vs ON (59=59).
- adaptive_budget=1: ON has *fewer* lines than OFF and the OFF-vs-ON diff set **changes
  run-to-run** (run 1: Sulfinic/Sulfoxide; repeat: AcylGroup) — scheduling noise, not a
  new decision. **ON never gains a subsumption over OFF; no class stably newly decided.**
- Incomplete count flat within noise; wall unmoved.
- **`nogood_prunes` / `nogood_prunes_netnew` are NOT surfaced** in classify output (only
  label-heuristic counters + `# timed-out pairs`). A separate SearchStats probe is needed
  to confirm net-new prune activity.

**Factual bottom line (no verdict — controller's call):** soundness gates clean
(curated FP=0/MISSED=0 ON, 13723 FP=0→0); on ore_ont_10019 the flag did not stably change
the hierarchy, did not lower the stalled count beyond noise, and did not move wall; prune
counters were not observable from the CLI.

## VERDICT (controller, 2026-07-14): 2b DEAD — sound, zero classify benefit

**By the decision criterion** (VIABLE iff the flag flips ≥1 currently-stalled class to
*decided* within budget, driven by net-new deep-tail prunes): **NOT met → 2b DEAD.**

- On `ore_ont_10019` the flag decides **no new class** (hierarchy byte-identical at
  adaptive_budget=0; the ±1–2 line diffs at adaptive_budget=1 are deadline-scheduling
  noise that changes run-to-run, and ON never *gains* a subsumption over OFF), does **not**
  lower the stalled/incomplete count beyond noise, and does **not** move wall (every config
  pins the 60 s aggregate deadline).
- Worse, flag-ON **adds** wall overhead on several curated ontologies (family 810→1520 ms,
  pizza 120→320, wine 60→470) — the per-clash core-extraction cost (repeated node-local
  closures per clash), exactly the cost concern the advisor raised — for no benefit.

**Soundness is not in question:** curated FP=0/MISSED=0 with byte-identical closures OFF
vs ON across all 8 ontologies, and the non-Horn adversarial oracle (`ore_ont_13723`) holds
FP=0→0 byte-identical. The prune is verdict-preserving (a superset of a B0-oracle-validated
node-local UNSAT core is UNSAT; the taint→`DepSet::ALL` widening keeps the backjump sound).
The mechanism is *correct* — it is simply *ineffective* on this workload.

**Why (as far as measured):** this is the CDBL 0%-wall outcome the roadmap warned about,
now confirmed for the wedge regime. Two compounding causes, consistent with the advisor's
pre-build analysis and SP1's result:
1. **SP1 already ate the upside.** A node-local prune front-runs at most one incremental
   `horn_fixpoint`, which SP1 made ~56× cheaper — so even prunes that fire save little.
2. **The residual stall is depth-bound, not re-derivation-bound** (SP1 finding; the classes
   reach the depth cap and the run is deadline-bound). No-goods prune redundant
   re-derivation; they do not reduce the search *depth* needed to refute — so they cannot
   convert the deadline-bound stall into decided classes.

**Observability caveat:** `nogood_prunes`/`nogood_prunes_netnew` are not surfaced by the
CLI, so we did not confirm *whether* the prune fires on `ore_ont_10019` (thin admissible-core
population — many clashes merge-tainted and excluded) versus *fires-but-backjump-redundant*.
Either way the classify outcome is zero benefit; the distinction would only refine the
post-mortem, not the verdict. A one-line SearchStats stderr dump under the flag would settle
it cheaply if ever revisited.

**Pivot (per roadmap):** the tractability lever for `ore_ont_10019`'s depth-bound stall is
NOT node-local UNSAT memoization. Remaining candidates: (2a) stronger/label-normalized
blocking to cap the disjunctive-search depth, or bounding `search.rs` as an honest tail
backstop (roadmap "honest tail gate"). SP2 as scoped (2b) is closed DEAD.

**Disposition of the code:** `RUSTDL_WEDGE_NOGOOD` stays **default-OFF**. It is sound and a
clean, tested reference implementation, but adds wall cost with no benefit when ON — so it
should either be left dormant (opt-in, for a future workload where re-derivation *is* the
bottleneck) or reverted. Controller/user's call at branch finish.

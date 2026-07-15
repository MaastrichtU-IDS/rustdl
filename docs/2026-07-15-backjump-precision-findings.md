# H3b backjump-precision probe: `ore_ont_10019` disjunctive-DFS stall

2026-07-15. Phase 1 of the wedge backjump-precision R&D
(`docs/superpowers/plans/... H3b`, brief `.superpowers/sdd/task-1-brief.md`).
Read-only measurement — no engine/behaviour change. Reuses the shipped shadow
precise-dependency probe (`RUSTDL_SHADOW_DEP_PROBE`) via a new gate,
`crates/owl-dl-reasoner/tests/backjump_precision_gate.rs`.

## Question

`ore_ont_10019` (namespace `http://ontology.dumontierlab.com/`) has ≥33
classes that stall the wedge under a 300 ms per-class budget
(`rustdl hyper-sat ... --per-class-timeout-ms 300`). Is the stall driven by
**backjump degradation** — the real `clash_deps` collapsing to `DepSet::ALL`
at a clash where a precise ("shadow") dep-set exists and would let
dependency-directed backjumping skip most of the disjunctive search — or is
the disjunctive breadth **intrinsic** (the true, precise dependency is itself
deep, so no backjump repair would help)?

## Setup

- Branch `feat/wedge-backjump-precision`; binary freshly built
  (`RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`).
- Stalled-class discovery: `rustdl hyper-sat ore_ont_10019.ofn
  --per-class-timeout-ms 300 [--depth 512] | grep -i stalled` → 33 stalled
  classes across two runs (branch counts vary run-to-run because the cutoff
  is wall-clock, not branch-count). `HydroxylGroup` confirmed present, at
  branch depth 137 (of the 512 cap used for discovery).
- Probed 5 of the deepest confirmed-stalled classes (depth 137-138 in the
  discovery run, `merge=0` in the hyper-sat branch split — pure `⊔`
  disjunctive branching, no `≤n` merge branches — deliberately chosen so a
  high `real_ALL%` on these classes **cannot** be attributed to
  functional/merge-taint; there is no merge to taint):
  `HydroxylGroup`, `MethylGroup`, `OxygenAtom`, `KetoneGroup`,
  `SecondaryAmineGroup`.
- Probe run: `sat_class_probe(&ont, &iri(class), depth=256, timeout=30s)`,
  env `RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0` (probe on; no
  early divergence cut, so the search runs the full 30 s per class rather
  than being adaptively truncated).
- Command: `RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable
  cargo test -p owl-dl-reasoner --release --test backjump_precision_gate --
  --ignored --nocapture`.

## Results

| class | result | branches (disj/merge) | max_depth | clashes | real_ALL | real_ALL% | crippled | bjgap_real (med/p90/max) | bjgap_shadow (med/p90/max) |
|---|---|---|---|---|---|---|---|---|---|
| HydroxylGroup | Stalled | 266408 (266264/144) | 256 | 25971 | 20323 | 78.3% | 20290 | 130/130/154 | 130/130/154 |
| MethylGroup | Stalled | 264537 (264393/144) | 256 | 24124 | 23062 | 95.6% | 23029 | 130/130/154 | 130/130/154 |
| OxygenAtom | Stalled | 266390 (266246/144) | 256 | 25971 | 20323 | 78.3% | 20290 | 130/130/154 | 130/130/154 |
| KetoneGroup | Stalled | 310989 (310945/44) | 256 | 60503 | 57551 | 95.1% | 57519 | 130/130/138 | 130/130/138 |
| SecondaryAmineGroup | Stalled | 259037 (258893/144) | 256 | 23770 | 23062 | 97.0% | 23029 | 130/130/154 | 130/130/154 |

All five classes still `Stalled` at the full 30 s / depth-256 budget (the
300 ms discovery timeout was not the bottleneck — these are genuinely hard,
not merely rushed). All five hit `max_branch_depth == 256`, i.e. the search
runs into the depth cap itself, not just the wall-clock budget.

**Aggregate reading** (mean across the 5 classes, unweighted): `real_ALL` ≈
**88.9%** of recorded clashes; `crippled_backjumps` tracks `real_ALL` almost
exactly (within 0.1-0.2% of it in every row).

## The key observation: `bjgap_real` and `bjgap_shadow` are numerically
## identical (not just close) on every one of the 5 classes

For all five stalled classes, `bjgap_real.median == bjgap_shadow.median`,
`.p90 == .p90`, and `.max == .max` — bit-for-bit identical histograms. This
holds despite `real_ALL%` being high (78-97%) and despite `crippled_backjumps`
being reported as nearly equal to `real_ALL` under the brief's Step-2
definition (`real_ALL AND shadow_bjgap > 1`).

The reconciliation is in the sentinel encoding: `DepSet::ALL`/overflow is
represented as `highest = Some(127)` (not "no info" / infinite gap), so
`bjgap_real` for an ALL-collapsed clash is `branch_depth − 127 + 1`, not
`branch_depth + 1`. With `max_branch_depth == 256` for every probed class,
most recorded clashes fire near the depth cap, so `branch_depth − 127 + 1 ≈
130` for the bulk of them — matching the observed real-median of 130 almost
exactly. The **shadow** median lands at the *same* 130, which means the
shadow's own genuinely-precise `highest` is *also* clustering close to level
~127 on the median clash — not near the clash's own branch depth (which
would give a shadow bjgap near 1, "no gap"), and not near the root (which
would give a shadow bjgap near 256, "huge gap"). In other words: on the
median clash, the *true* minimal dependency set is itself roughly as deep as
the `ALL` sentinel already assumes.

Two consequences:

1. The brief's `crippled_backjumps` metric (`real_ALL && shadow_bjgap > 1`)
   is nearly vacuous on this workload: `shadow_bjgap > 1` is satisfied by
   almost any dependency shallower than the clash's own decision level, which
   is true here for essentially every clash regardless of whether the shadow
   dependency is "meaningfully more precise" than the ALL sentinel. It does
   not discriminate a genuine backjump opportunity from one where the
   precise dependency is itself still deep.
2. The more load-bearing comparison — `bjgap_shadow.median` vs
   `bjgap_real.median` — shows **no material gap** on any of the 5 classes:
   both sit at 130 (of a 256-deep search), not `bjgap_real ≈ 1` with
   `bjgap_shadow` far larger. The pattern the brief's Fix #1 criterion asks
   for (`bjgap_real.median ≈ 1`, `bjgap_shadow.median` far greater) is **not
   present** here; the pattern matching Fix #2 (`bjgap_real ≈ bjgap_shadow`,
   both already ~precise/deep) **is** present, on the aggregate histograms.

## Caveats

- `merge=0`-ish branch split (44-144 merge branches out of ~260-311k total;
  effectively all-disjunctive) on all five classes rules out
  functional/`≤1`-merge taint as the source of the high `real_ALL%` — there is
  almost no merge branching to taint. Whatever collapses ~80-97% of clashes to
  `ALL` here is a **disjunctive-branch** widening site (the `⊔`-rule clash
  path), not the cardinality/merge-taint path the existing
  `RUSTDL_PRECISE_CARD_DEPS` lever targets.
- The `bjgap_real == bjgap_shadow` equality is an aggregate (median/p90/max)
  finding, not a per-record proof that `real.highest == shadow.highest` on
  every clash — it is consistent with (but does not by itself establish)
  that the shadow's precise highest is *itself* clustering near level ~127
  on the bulk of clashes. A per-record breakdown (e.g. a scatter of
  `shadow.highest` restricted to the `real_ALL` subset) was not computed in
  this Phase-1 pass; it would sharpen (1) below if pursued.
- All 5 classes stall at the search's `max_branch_depth == 256` cap; the
  disjunctive breadth is at minimum partly a **scale** phenomenon (depth-256
  search over a dense chemistry-ontology disjunction set), independent of
  dependency precision.

## Read (factual lean, not a ruling)

The data leans towards **Fix #2** (absorption/BCP or bound-the-tail) over
**Fix #1** (backjump-precision repair):

- The Fix #1 signature the brief specifies — `bjgap_real.median ≈ 1` with
  `bjgap_shadow.median` far greater — is absent. Instead `bjgap_real ≈
  bjgap_shadow` on every probed class, at the histogram level (median, p90,
  *and* max all match).
- This suggests that even where the real dep-set is reported as `ALL`
  (78-97% of clashes), the clash's true minimal dependency is *itself*
  roughly as deep as the `ALL` sentinel's fixed level (127) already implies
  for a clash at this search depth — i.e., recovering the precise dep-set
  would not, on the median/p90/max, buy a materially larger backjump than
  what the sentinel already yields at this depth. The disjunctive breadth
  looks intrinsic to these 5 classes rather than an artifact of an
  over-eager `DepSet::ALL` fallback.
- Caveat pulling the other way: `crippled_backjumps` (the brief's literal
  Step-2 metric) is large in absolute terms (20k-58k per class) — if that
  metric, rather than the aggregate histogram comparison, is the intended
  decision signal, it would read as supporting Fix #1. The write-up above
  explains why this reviewer believes that metric is too weak a filter on
  this workload (`shadow_bjgap > 1` is satisfied almost unconditionally at
  these branch depths) and that the identical-histogram result is the
  stronger signal — but the controller should weigh both readings.

**No fix has been selected or implemented in this pass** — this is Phase 1
measurement only, per the task brief's instruction that the controller makes
the Fix #1 vs Fix #2 verdict.

## Reproduce

```sh
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
RUSTDL_SHADOW_DEP_PROBE=1 RUSTDL_ADAPTIVE_BUDGET=0 RUSTUP_TOOLCHAIN=stable \
  cargo test -p owl-dl-reasoner --release --test backjump_precision_gate -- \
  --ignored --nocapture
```

## VERDICT (controller, 2026-07-15): Fix #1 ruled out → wedge quick-levers exhausted

**Fix #1 (backjump-precision repair) is NOT the lever.** `bjgap_real` and
`bjgap_shadow` are *bit-identical* on all 5 classes across median, p90, AND max
(130/130/154). Had precise deps offered headroom, real-`ALL` clashes with a
shallow true `highest` would give a large shadow `bjgap`, pushing shadow's
upper percentiles/max above real's — that divergence is absent everywhere in the
distribution. So recovering the precise dep-set would not enable a materially
bigger backjump than the `DepSet::ALL` sentinel already yields at these depths;
the clashes' true minimal dependencies are themselves deep. The high real-`ALL`%
(~89%) is real but does not translate to lost backjump distance.
(Residual caveat: a per-record `shadow.highest` histogram restricted to the
real-`ALL` subset was not computed and would nail this definitively; but the
max-equality already makes a material-headroom Fix #1 unlikely. Cheap to sharpen
if a reviewer wants certainty before fully closing Fix #1.)

**This exhausts the wedge's tractable quick levers for the dense-SROIQ tail:**
throughput (SP1) — dead; node-local no-goods (SP2) — dead; MRV ordering — on,
inert; `sat_lookahead` — inert; backjump-precision (this) — ruled out. The
remaining directions are both large:
- **Fix #2 — absorption / unit propagation (BCP):** make disjunctions not become
  branch points (Konclude's 90 ms comes largely from this). A genuine new engine
  capability — multi-session, uncertain, and untried. The only remaining path
  that could actually *decide* the 33 stalled classes.
- **Bound-the-tail (honest floor):** make the `Stalled → NoVerdict → search.rs`
  fallthrough bounded so classify returns sound-incomplete fast instead of
  burning the deadline (some ORE onts hang; SP2 sweep showed 4 timeout onts).
  Does NOT close completeness, but a sure robustness win for the whole tail.

Konclude/HermiT decide the ontology in ms, so it is *not* intractable — the gap
is rustdl's disjunctive-engine sophistication (absorption/propagation), a large
build. The honest position: the wedge's incremental levers are spent; closing
`ore_ont_10019` now requires either the big absorption investment (Fix #2) or
accepting the characterized tail and bounding it.

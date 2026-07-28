# Model-derived realization read-off — results (NO-GO for default-ON)

**Date:** 2026-07-28
**Branch:** `feat/model-derived-realize` (parked, commits `746da29..ca7bebb`; NOT merged)
**Gate:** `RUSTDL_MODEL_DERIVED_TYPES` (default OFF — and staying that way)
**Spec:** `docs/superpowers/specs/2026-07-28-model-derived-realize-types-design.md`
**Plan:** `docs/superpowers/plans/2026-07-28-model-derived-realize.md`

## What was built

A HermiT-style deterministic type read-off for `realize`: from the one ABox
witness model the #57 pseudo-model already builds, read the labels whose hyper
per-label `DepSet` is **empty** (derived with no ⊔-branch decision ⟹ entailed in
every model ⟹ sound to emit as a type without a `{a} ⊓ ¬C` probe). Branch-dependent
labels still get probed. Fully implemented, per-task-reviewed (TDD), and
final-whole-branch-reviewed. Default-OFF; the flag-off path is byte-identical to
pre-branch (confirmed).

- Task 1 (`hyper.rs`): `DepSet::is_empty`, `absorbed_merge` merge-touch flag,
  `seeded_individual_deterministic_labels` accessor (excludes merge-touched
  individuals).
- Task 2 (`lib.rs`): `WitnessModel { complete, deterministic }`, built once.
- Task 3 (`realize.rs`): gate flag + four-way decision (told → deterministic
  read-off → #57 prune → probe).
- Task 4 (`tests/model_derived_realize.rs`): read-off / must-probe / merge-guard
  fixtures.

## Verdict: NO-GO on default-ON — two independent reasons

### 1. Corpus-invisible value (measured, isolated)

Ship-gate measurement, single-threaded (isolated — the ORE benchmark's "slow"
realize walls turned out to be `-P 4` contention-inflated; isolated, those onts
finish in <4 s):

- **Hard DNF tail: 0/8 model-builders rescued.** Of the 58 onts rustdl DNFs at
  realize, only 15 build a witness model at all (8 s budget). Of 8 sampled
  model-builders, read-off ON converts **zero** to completing — they DNF at the
  130 s cap with the flag on exactly as with it off (0 type rows either way).
- **Completing onts: zero speedup.** On 8 onts that complete, ON wall ≈ OFF wall
  (e.g. 1344↔1349 ms, 2558↔2541 ms, 3611↔3569 ms) and ON output is byte-identical
  to OFF (md5 match on all 8). ON is if anything marginally slower (extra
  deterministic-label computation in the witness build).

**Why (architectural):** rustdl already has (a) the told-closure fast path for
told positives and (b) the #57 pseudo-model prune for negatives (default-ON). The
read-off only adds *deterministic-but-not-told positives* — a negligible increment
on completing onts. And on the DNF tail it removes only the **cheap** (deterministic)
probes, while the wall is dominated by the **expensive branch-dependent** (hard-SROIQ)
probes it structurally cannot touch. It falls in a gap: already covered on one side,
unable to reach the bottleneck on the other.

### 2. Structural FP hole for default-ON (crown-jewel risk)

The final whole-branch review found what four per-task reviews missed. An
EMPTY-cause `≤n`-**partition** merge (`partition_rec` → `merge`, `hyper.rs:3691`)
skips the `birth_deps` causation fold (`hyper.rs:3841`, `if cause_deps != EMPTY`).
The survivor is flagged `absorbed_merge` (so it is excluded from read-off) but its
`birth_deps` stays empty. The back-prop edge-copy (`hyper.rs:3916`, gated
`inverse_func_merge || incremental_fixpoint`, both default-ON) then fires a clause
`R(rp,y) ∧ L(y) → M(rp)` onto a **different, unmerged, named individual `rp``;
`clause_body_deps` (`hyper.rs:4018`, body-only, no ambient decision term) unions the
survivor's *empty* `birth_deps`, so `M(rp)` lands empty-dep. The node-scoped merge
guard does not exclude `rp` → `M(rp)` would be read off as entailed despite depending
on the `≤n` branch choice → **false-positive subsumption**.

Empirically unreproduced (neither the reviewer nor the Task-4 fixtures could force
the co-firing), but structurally live and consistent with the engine's known
EMPTY-cause merge behavior. **Default-ON must not flip on empirical-clean alone.**
Closing it would need hot-path surgery (taint-fold a sentinel into the survivor's
`birth_deps` on the EMPTY-cause path, or a coarse whole-ont bailout that disables
read-off if any EMPTY-cause merge fires). **Not worth building given reason 1.**

## The realize gap is real — this was the wrong lever

The motivating gap stands: on the 58 onts rustdl DNFs at realize, **Konclude
completes 54/58** (avg 6.8 s) and **HermiT 21/58**; rustdl completes 0. Konclude
realizes all individuals from one saturated model; rustdl runs O(individuals ×
classes) independent probes.

But the read-off is not the fix. The measured DNF-tail bottleneck is **mixed**:

| ont | `classify` alone (isolated) | realize |
|---|---|---|
| `ore_ont_13545` | 7 ms | DNF (130 s) |
| `ore_ont_14379` | 2.0 s | DNF |
| `ore_ont_10197` | 28 s | DNF |
| `ore_ont_9053` | 57 s | DNF |

`realize_tableau_internal` runs `classify_top_down_internal` **first** (for the
Hasse-leaf filter). On `9053`/`10197` that alone burns 28–57 s before any
per-individual work. On `13545`/`14379` classify is fast and the per-individual
**probe loop** — dominated by expensive branch-dependent hard-SROIQ pairs — is the
wall. Read-off addresses neither.

## Recommended next investigation (separate, needs its own Phase-0)

Two real leads, each requiring an instance-level Phase-0 diagnosis before scoping
(per the `dense-sroiq-root-cause` lesson — read the failing instance, don't scope
off an aggregate):

1. **Realize's up-front `classify_top_down` cost.** On classify-heavy ABox onts it
   dominates the realize budget. Can realize use a cheaper/partial hierarchy for the
   Hasse-leaf filter, or interleave, instead of a full classify first?
2. **The expensive branch-dependent probe residual.** This is the same
   hard-SROIQ-pair frontier as the wine-wall / disjunctive-search work
   (`wine-wall-bjgap1-genuine`) — the one-model-realize (Konclude-style, "increment-2")
   direction, which is genuinely hard.

## Disposition

Branch parked unmerged (protects the commits; nothing builds on this, so it is not
merged as dormant scaffolding). Do **not** build the FP fix. This document + the
spec + the plan are the durable record of the measure-out.

## Addendum (2026-07-28): Phase-0 refutes lead #1 (classify cost)

Decomposed realize into classify-alone vs probe-loop on 4 model-builders (300 s cap,
isolated). Classify is **not** the binding constraint:

| ont | classify alone | realize (300 s cap) | ⇒ probe-loop |
|---|---|---|---|
| `ore_ont_13545` | 6 ms | ok 7 ms (rows=0, empty realize) | ~0 |
| `ore_ont_14379` | 2.0 s | DNF (300 s) | ~298 s |
| `ore_ont_10197` | 28 s | DNF (300 s) | ~272 s |
| `ore_ont_9053` | 56 s | DNF (300 s) | ~244 s |

The per-individual **probe loop** runs 244–298 s and still produces zero output.
Removing/cheapening `classify_top_down` saves 2–56 s where the probe loop needs
cutting by 200 s+ — a red herring. (The earlier "classify-dominant on some" framing
was misleading: classify is a *visible* cost but never the binding one; and `13545`'s
earlier 130 s DNF was a contention anomaly — its realize is empty and instant.)

**Both leads collapse to one root:** the O(individuals × classes) probe loop dominated
by expensive branch-dependent hard-SROIQ probes = the one-model-realize / wine-wall
frontier (`wine-wall-bjgap1-genuine`). Lead #1 is closed. Separately notable: realize
is **all-or-nothing** — it prints types only after the full parallel loop completes, so
a DNF yields *nothing* even for individuals already typed. A global-deadline
partial-output path (analogous to classify's `--global-timeout-ms`) is a cheap
*robustness* win (DNF→partial), distinct from the hard completeness frontier.

## Addendum 2 (2026-07-28): Phase-0 on lead #2 finds a real classify-bug (modest lever) + confirms the hard frontier

Instrumented `realize_tableau_internal` (heartbeat + stage timers + probe-loop
counters, temp, reverted). The per-individual probe loop was NOT the first
blocker on the classify-heavy onts — **`classify_top_down_internal` is called
UNBOUNDED** (`realize.rs:940`, `(internal, None, None)`) while the CLI `classify`
passes default budgets. Verified: `classify --pair-timeout-ms 0` (unbounded, what
realize does) DNFs at 90 s; default-budget classify is 2 s. So on classify-heavy
onts, one hard class-subsumption pair hangs *inside realize's classify step*,
before the individual loop starts. **This is a genuine correctness/robustness bug**
(realize can hang unboundedly in a step the CLI bounds), sound to fix (bounded
classify = sound under-approx: at worst a less-*minimal* `most_specific_types`,
never a false type).

**Value of bounding it (1000 ms/pair, shippable defaults, over the 58): 2/58
rescued** (`14379`: DNF→instant with told+#57-prune handling all 5490 pairs,
**probe=0**; `8250`). The other **56 DNF** are the hard frontier: on `10197`/`9053`
bounded classify unblocks the classify step (26 s/56 s) but the per-individual loop
is then dominated by **expensive in-witness probes** (~830 ms each, `probe_true=0` —
hard negatives hitting the 750 ms deadline), confirmed by heartbeat (probe 48→144,
probe_total 40→120 s). That is the one-model-realize / hard-SROIQ-probe frontier
(`wine-wall-bjgap1-genuine`), unchanged.

**Net:** lead #2 decomposes into (a) a small standalone robustness fix — bound
realize's internal classify — worth shipping on its own merit (closes the
unbounded-hang, rescues the classify-bug-only tail), and (b) the hard in-witness
probe frontier for the remaining ~56/58 (measured-out territory). The
"iterate witness labels not all-classes" idea does NOT help (b): the expensive
probes are in-witness, not pruned iterations.

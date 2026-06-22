# SP0 saturation spike — verdict: NO-GO (value-partition hypothesis refuted) — 2026-06-23

Plan: `docs/superpowers/plans/2026-06-23-sp0-saturation-spike.md`. Spec:
`docs/superpowers/specs/2026-06-23-coupled-saturation-tableau-design.md`.

## Verdict

**NO-GO for SP0 as scoped (value-partition saturation).** The pre-committed gate required
wine's per-pair branching to *collapse* by pre-resolving value partitions. The cheapest
possible probe refuted the underlying premise: **value partitions are NOT wine's dominant
per-pair cost.** A saturation targeting them (SP0's scope) cannot collapse wine.

The broader coupled-saturation project is *not* refuted, but the gate revealed it has **no
cheap value-partition on-ramp** — the minimum saturation that would help wine must cover the
**joint ∀ × ≤n × nominal interaction**, which is the full, hardest part of Konclude's
saturation. So there is no de-risked entry smaller than the whole multi-month build.

## Method — cheapest-probe-first (no seed code needed)

The plan's Task 1 (value-partition detection) was built and unit-tested (validated the IR
access), but the verdict came from **text-level ontology rewrites + the matched test**,
which is far cheaper than building the Task-2 seed. For each rewrite, the matched hard test
`sat(AlsatianWine ⊓ ¬AmericanWine)` (Konclude 1ms-after-39ms-precompute; rustdl DNF) was
re-timed (rustdl, 45–60s cap):

| wine rewrite | matched test |
|---|---|
| **remove value-partitions** (`ObjectOneOf(...)` → `owl:Thing`, all 33) | **DNF 60s** |
| remove cardinality (`Max`/`Min`/`Exact`) | DNF 45s |
| remove max-cardinality only | DNF 45s |
| remove `∀` (`ObjectAllValuesFrom`) | DNF 45s |
| **remove all three jointly** | **COLLAPSED — 0.0s** (EL-instant) |

## Interpretation

- Removing value-partitions alone leaves the test DNF → **value partitions are not the
  bottleneck** (this corrects `docs/konclude-vs-rustdl-wine-2026-06-23.md` §6, which
  attributed wine's cost to nominal value-partitions).
- No single non-EL construct is the bottleneck (each alone → still DNF).
- Removing **all three** (∀ + ≤n + nominal) drops wine to EL-instant → the cost is their
  **joint interaction** (consistent with the wine-wall finding: combinatorial width from
  nominals × ∀ × cardinality over the varietals).

## Consequence for the project

- **SP0 (value-partition spike) is dead** — saved the multi-day Task-2 seed build via a
  ~10-minute probe. This is the gate working as designed.
- The coupled-saturation project's "cheap de-risked on-ramp" does not exist: a saturation
  that helps wine must soundly approximate the **joint ∀ + ≤n + nominal** structure. The
  ≤n-cardinality + ∀ interaction is the algebraically hardest part of Konclude's saturation
  to reproduce soundly. So re-scoping a smaller spike isn't possible — the next gate would
  *be* a large chunk of the real project.
- Recommendation: **do not start the saturation build now.** The honest state is: the
  technique works (Konclude), the prize is wine + obscure DL tail (capability investment,
  not working-corpus speedup), and there is no cheap way to further de-risk it — it is a
  full multi-month, soundness-critical build with the hard part (joint ≤n+∀+nominal sound
  saturation) unavoidable from the first useful increment. Bank the scope/spec/plan as a
  ready, honestly-costed package; revisit only with explicit appetite for that full build.

## Code disposition

Throwaway per the spike plan: `crates/owl-dl-core/src/value_partition_spike.rs` and its
`lib.rs` mod line are reverted (not merged). Only this verdict doc + the scope spec + the
plan land. The rewrite probes used `/tmp/w-*.ofn` (scratch, not committed).

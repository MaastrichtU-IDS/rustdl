# Three-arm sweep: NEITHER opt-in flips, and the reorder cost `invpair` its only corpus gain

**Date:** 2026-08-07 · **Pin:** `bin/rustdl-rel-20a7e73`, sha `74180aca60d1bd8f`
**Predictions pre-registered before the run** (`/tmp/sweep3-pred.txt`, reproduced below).
Three arms sharing one baseline — `off` / `RUSTDL_INVERSE_PAIR_FUNC=1` /
`RUSTDL_HYPER_MATCH_DEADLINE=1` — 1,920 ontologies each, 60 s cap, threads=1, sequential,
`--digest-strip-comments`. Per-arm propagation proven on each flag's own discriminating input
before launch (`consistent`→`inconsistent`; `label_cache_build` 76,077→107 ms).

## Raw result

| | off | invpair | matchdl |
|---|---:|---:|---:|
| ok | 1750 | 1748 | 1748 |
| dnf | 169 | 171 | 171 |
| `ok → dnf` | — | 4 | 3 |
| `dnf → ok` | — | 2 | 1 |
| answer changes | — | **0** | **0** |

## Most of the regression signal is cap-boundary noise — and the arms prove it

Three ontologies regress in **both** arms. Re-run serially at a 120 s cap they produce
**identical output in all three arms**:

| ontology | off | invpair | matchdl |
|---|---|---|---|
| `ore_ont_14351` | ok/526/68 s | ok/526/59 s | ok/526/59 s |
| `ore_ont_15491` | ok/8179/57 s | ok/8179/66 s | ok/8179/**80 s** |
| `ore_ont_5617` | ok/8844/58 s | ok/8844/70 s | ok/8844/**85 s** |

All three complete at 57–85 s against a **60 s** sweep cap, so the `ok → dnf` is a boundary
artifact. **Independent confirmation:** the `off` arm itself drifted from yesterday's
`ok=1753 dnf=166` to `ok=1750 dnf=169` at *identical* default settings, which is only possible
if ~3 ontologies straddle the cap.

**But this is not a clean acquittal for `matchdl`.** It adds **+40%** (57→80 s) and **+47%**
(58→85 s) wall on two of them. A flag predicted to be inert at default budgets is not inert; it
is materially slower on some completers, and at a 60 s production cap that pushes them over.

## `RUSTDL_INVERSE_PAIR_FUNC`: one real regression, and the corpus gain is GONE

- **Real regression: `ore_ont_16372`, 8.28 s → dnf.** Not boundary noise — independently
  reproduced serially on 2026-08-06 and unrepaired by the reorder.
- **The +17-pair gain has been lost.** Yesterday `ore_ont_13859` normalised to **6,264 = Konclude
  = HermiT** with the flag on, against 6,247 off. Today: **6,247 both arms.**

**Cause: my own reorder fix on 2026-08-06.** Moving `derive_inverse_pair_functionality` to run
*after* `derive_functional_max_cardinality` — which repaired 3 of 4 sweep regressions — also
stopped derived-functional roles receiving the `∃R.⊤ ⊑ ≤1 R` enforcement GCI. **Those GCIs were
the mechanism producing the 17 sound subsumptions.** I made that trade without measuring the
benefit side, and reported the fix as "3 of 4 regressions gone" without noting what it cost.

So the flag's present value is narrower than I stated: it still decides the 5-axiom reproducer
and the 7-axiom `ore_ont_4141` core (Part 2's edge materialisation, untouched by the reorder,
and the canaries still pass), but it has **no corpus-level completeness gain and one corpus
regression**.

## Verdict against the pre-registered rules: NEITHER FLIPS

- **`invpair`** — rule was *"flip only if `ok → dnf` = 0"*. It is 1 (real). **Stays OFF.**
- **`matchdl`** — rule was *"corpus-inert ⇒ keep OFF, because a default flip that changes
  nothing is risk without reward"*. It is worse than inert: 0 recoveries, 0 answer changes, and
  +40–47% wall on two completers. **Stays OFF.**

Both remain sound, tested opt-ins for v0.4.15 (FP=0 net 13 VERIFIED, 1,586 tests, flag-OFF
byte-identical). Neither is a default change.

## Follow-ups this created

1. **Recover `invpair`'s completeness gain without the regressions.** The enforcement GCI is
   valuable *and* costly; emitting it selectively — rather than for every derived-functional
   role on ontologies carrying 76–118 inverse pairs — is the obvious shape, and is untried.
2. **`matchdl`'s wall cost needs explaining** before any flip. Truncating match enumeration
   removes pruning, so downstream work grows; that is a plausible mechanism but unmeasured.
3. The two "recoveries" (`ore_ont_2574`, `ore_ont_7204`) were **not** serially verified and, on
   the evidence above, should be assumed boundary noise until they are.

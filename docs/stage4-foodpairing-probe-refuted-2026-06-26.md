# Stage-4 — food-pairing probe: REFUTED; residual = wine-type taxonomy value-determinism

**Status:** recon (durable, throwaway on feat/stage4-engine-characterization). Isolates what the
post-lazy-skip 200k Gamay branches actually are, via body-trigger dump.

## Result

With `RUSTDL_SKIP_NAMED_BINARY=137` (Consumable partition skipped), the residual compound ⊔ points
are **almost all on node=0 (Gamay's home node)**, with clause-body triggers = wine-type taxonomy
classes: **Wine, SemillonOrSauvignonBlanc, WhiteWine, WhiteNonSweetWine, Meritage** (NOT
food/`Course`/`hasDrink` — food-pairing REFUTED). The arity-5 survivor `[193,268,192,257,288]` is
triggered by **Meritage** (a multi-way grape-blend definition); the `[308,318]` by
**SemillonOrSauvignonBlanc** (≡ Semillon ⊔ SauvignonBlanc).

**Smoking gun:** Gamay's home-node label accumulates **both `RedWine` AND `DryWhiteWine`/
`WhiteTableWine`** during the search — i.e. the wedge branches the wine-type definitions and
*explores white-wine branches for a red wine*, only clashing on color (`≤1 hasColor` + Red≠White)
deep, then backtracking. The 200k is this wine-type-definition product search.

## What this sharpens (and why told-disjointness/SP-B failed)

The residual is the **wine-type taxonomy disjunctions** (grape-varietal unions + color/body/sugar
type partitions). They ARE constrained — Gamay's red color *should* exclude WhiteWine and its
subclasses — but the exclusion is **value-partition-semantic** (`∀hasColor.{Red}` vs
`∀hasColor.{White}` under `≤1 hasColor` + `Red≠White`), **NOT told-disjointness**. That is exactly
why SP-B (told-disjoint ⊔-guidance) got `pruned=0`: the incompatibility relation it used is too
weak to see color-based type-disjointness. The needed relation is **value-aware**: two types are
incompatible when their forced `∀R.{v}` values differ on a functional/`≤1` role (nominal
distinctness).

## Concrete component (the sharpened target)

A **value-aware incompatibility relation** — `∀R.{v1}` (or derived) vs `∀R.{v2}`, `v1≠v2`, `R`
functional/`≤1` ⟹ incompatible — fed into the wedge's ⊔ live-filter to prune dead wine-type
branches (the white-wine branches for a red wine) BEFORE exploring them. This is SP-B's machinery
(saturation-guided ⊔ pruning) with the richer value-aware relation SP-B lacked. It is a concrete,
buildable, measurable, FP-safe-direction (pruning a dead disjunct is the precise-merge-FP danger
direction ONLY if the incompatibility is mis-derived — so it must be gated wine-FP-first) first
component of the integrated ∀+≤1+nominal expansion.

## Verdict

Food-pairing refuted; the residual is wine-type taxonomy disjunctions resolvable by value-partition
determinism. The lever is precisely a **value-aware incompatibility relation** (richer than
told-disjoint, the SP-B gap), feeding ⊔-pruning. This is the first concrete component of the
Konclude integrated expansion — measurable as a throwaway (does value-aware ⊔-pruning collapse
Gamay?) before productionizing. main = origin/main 1be126d (pushed); this recon read-only.

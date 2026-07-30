# Reduced-input `abox_check`: stop building a full `PreparedOntology` to read one verdict

**Date:** 2026-07-30
**Status:** Design — ready for implementation planning
**Origin:** salvaged finding 2 of `2026-07-30-tbox-only-prepare-design.md` (that lever retired at
zero payoff; this one replaced it and is better evidenced)
**Flag:** none needed for correctness — see § Rollout

## The waste

**The waste is on the FAST PATH ONLY** (corrected 2026-07-30 — the first draft of this spec
claimed both entry points):

- **fast path (`classify.rs:785-787`) — pure waste.** A **full** `PreparedOntology` is built
  (EL saturation, told tables, `HyperCache::build`, NNF + absorb, complements) *solely* to read
  `abox_verdict()`, and then `return classify_pure_el(...)` **discards it** — `prepared` is never
  used again.
- **top-down path (`classify.rs:1626`, verdict at `:1631`) — no waste.** The same object is
  needed for classification anyway, and `abox_verdict()` is a **lazily-initialised** field
  (`lib.rs:4692`, `get_or_init`), so the check adds no construction there.

This is confirmed by mode, 8 of 8: every ontology that saved takes the fast path, and both that
saved nothing take the hybrid path (see § Measured breadth). It is a mechanism, not a correlation.

But `abox_check::check` reads only eight fields: `abox`, `axioms`, `told`, `pool`,
`inverse_pairs`, `hierarchy`, `disjoint_role_pairs`, `closure`. It **never touches `hyper` or
`tbox`** — and those are the expensive ones. `classify.rs:588-594`'s own comment already names
this cost, but acts on it only for ABox-*free* inputs.

Measured cost of the discarded work, `ore_ont_1043`, single-threaded:

| config | wall | RSS |
|---|---|---|
| baseline | 2.35 s | 526 MB |
| `RUSTDL_HYPERTABLEAU=0` | 2.10 s | 434 MB |
| `RUSTDL_ABOX_CHECK=0` | 1.73 s | 341 MB |
| both off | 1.65 s | 341 MB |

So the check's build costs **0.62 s / 185 MB**, of which `HyperCache` is 0.25 s / 92 MB and
absorb-plus-the-rest ~0.37 s / 93 MB.

`RUSTDL_ABOX_CHECK=0` is an **upper bound**, not this lever's value: a reduced input still builds
the eight fields. The recoverable part is the `hyper` + `tbox` share, which the table puts at a
majority of the 185 MB and at least 40% of the wall.

## Measured breadth

Eight ABox-bearing ontologies that currently complete, `check ON` vs `check OFF`:

| ontology | assertions | ON | OFF | saving |
|---|---|---|---|---|
| `ore_ont_10073` | 457,670 | 9.70 s | 5.82 s | **40%** |
| `ore_ont_10068` | 180,242 | 3.62 s | 2.23 s | **38%** |
| `ore_ont_1012` | 237,119 | 11.01 s | 8.78 s | **20%** |
| `ore_ont_10127` | 105,548 | 19.06 s | 18.98 s | 0% |
| `ore_ont_10123` | 15,133 | 1.38 s | 1.36 s | 1% |
| `ore_ont_10058` | 1,425 | 3.91 s | 3.91 s | 0% |
| `ore_ont_10173` | 2,004 | 3.91 s | 3.91 s | 0% |
| `ore_ont_10230` | 586 | 0.12 s | 0.10 s | 17% (0.02 s absolute — noise) |

**Second sample — the 50k–180k band (spec gate 6, now run):**

| ontology | assertions | ON | OFF | saving |
|---|---|---|---|---|
| `ore_ont_1043` | 137,569 | 2.36 s | 1.63 s | **31%** |
| `ore_ont_1115` | 74,124 | 0.83 s | 0.64 s | **23%** |
| `ore_ont_10965` | 108,314 | 1.24 s | 0.97 s | **22%** |
| `ore_ont_11110` | 81,052 | 4.45 s | 3.54 s | **20%** |
| `ore_ont_10127` | 105,548 | 19.06 s | 19.04 s | 0% |
| `ore_ont_10838` | 135,472 | 4.64 s | 4.67 s | −1% |

**Combined, 14 ontologies sampled:**

| band | wins | savings |
|---|---|---|
| ≥180k assertions | **3 / 3** | 40%, 38%, 20% |
| 50k–180k | **4 / 6** | 31%, 23%, 22%, 20% |
| <50k | **0 / 4** | inert |

**7 of 9 above 50k assertions save 20–40%; everything below 50k is inert.**

**Population — measured on the correct predictor, not the proxy (2026-07-30).** The "192 with
≥50k assertions" figure below was an assertion-count proxy written before the fast-path mechanism
was found. Counted directly over 120 sampled completing ORE ontologies:

| set | count | share |
|---|---|---|
| completing (sampled) | 120 | — |
| take the fast path | 68 | 57% |
| fast path **and** any ABox | **25** | **21%** |
| fast path **and** ≥50k ABox assertions | **8** | **6.7%** |

So the addressable set is ~21% of completing ontologies for *some* saving and **~6.7% for the
20–40% band** — extrapolating to ORE's 1,920, roughly **400** and **107** respectively. That is
materially smaller than the retired 192-with-≥50k-assertions estimate, and it is the number to
quote: it selects on the predictor that was shown to be *binding* (the path) rather than merely
present (assertion count). The recurring error this repo has made six times this month is exactly
that substitution.

Unlike the three estimates that collapsed this month, this is a **measured in-band hit rate
(78%)**, not a feature-presence count — the distinction that killed the others. It is still a
sample: 9 of 192 measured above the threshold.

**The predictor is the PATH, not the size — and this fully explains the two non-winners.**
Modes measured:

| ontology | mode | saving |
|---|---|---|
| `1043`, `1115`, `10965`, `11110`, `10068`, `10073` | **pure EL (fast path)** | 20–40% |
| `10127`, `10838` | **hybrid** | 0% |

All six winners are fast-path; both non-winners are hybrid. So the saving requires **fast path +
an ABox large enough that the discarded build is expensive**. ABox size only sets the magnitude;
the path decides whether there is anything to save at all. Nothing is left unexplained.

Consequence for the population: it is not "192 ontologies with ≥50k assertions" but "ABox-bearing
ontologies that reach the fast path" — i.e. `is_pure_el` / `saturator_complete_fragment` /
`tbox_only_saturator_eligible` **and** ABox-bearing. Count that set before quoting a number.

## Why this is a better lever than the one it replaced

| | retired ABox-filter lever | this |
|---|---|---|
| soundness argument needed | yes — nominal-freedom + consistency, with an inconsistency-path obligation | **none** |
| verdict change | possible if the contract is broken | **verdict-identical by construction** |
| premise | ABox irrelevant to class subsumption | none — same inputs, same answer |
| applies to nominal-bearing ontologies | no | **yes** |
| measured payoff | 0 | 20–40% on 7/9 sampled >50k assertions |

`abox_check` receives exactly the same eight values, so it computes exactly the same verdict. The
change is which *other* fields get built alongside — a pure waste removal.

## Design

**Extract the check's dependencies into their own input type**, rather than adding a second
`PreparedOntology` constructor:

```rust
pub(crate) struct AboxCheckInput<'a> {
    abox: &'a Abox,
    axioms: &'a [Axiom],
    told: &'a ToldTables,
    pool: &'a ConceptPool,
    inverse_pairs: &'a [(Role, Role)],
    hierarchy: &'a RoleHierarchy,
    disjoint_role_pairs: &'a [(Role, Role)],
    closure: &'a SaturationResult,
}
```

`abox_check::check` takes that instead of `&PreparedOntology`. **Only the fast-path site
(`classify.rs:785-787`) changes**: it builds the eight values directly and calls
`abox_check::check` on them, skipping `HyperCache::build`, `ConsistencyCache::build`,
`snapshot_cache`, and absorb entirely. The top-down site keeps `prepared.abox_verdict()`
unchanged — it needs the full object regardless, so there is nothing to save and no reason to
touch it.

**Why a struct rather than a reduced constructor.** A second `PreparedOntology` constructor would
leave two objects of the same type with different completeness, and the id-space hazard the
`ConsistencyCache` doc already records (`lib.rs:3159`: "a mismatched hierarchy would let an
unrelated edge satisfy a super-role atom = false clash") becomes reachable by accident. An
explicit input type makes the dependency set checked by the compiler and cannot drift — if someone
later makes `abox_check` read `hyper`, it will not compile.

**Fold in the duplicated saturation.** On the fast path the caller already holds a closure (it
is computed before the gate and passed to `classify_pure_el`), yet `from_internal` calls
`owl_dl_saturation::saturate(&internal)` again at `lib.rs:4567`. `AboxCheckInput` borrows the
caller's closure, removing a second full EL saturation. Separate waste from the `hyper`/`tbox`
one, measured inside the same 0.62 s.

**`ConsistencyCache` disappears from the fast path for free** under this change (it is only ever
built inside `from_internal`, which the fast path stops calling). Recorded because it is a second
independent waste and confirms the direction: `prepared.consistency` is read only at
`lib.rs:4002/4046/4668/4684` — 4002 inside `is_consistent_internal_full`, the rest on the same
consistency path. **`classify.rs` never reads the field** (its 11 textual hits on "consistency"
are comments about the *ABox pre-check*, a different mechanism — verified, not assumed). Yet it is
built whenever `wedge_consistency_enabled() && internal_has_abox`. On a large-ABox ontology that
re-clausifies the whole axiom set a second time. Dead-field removal on this path; no soundness
surface.

## Scope

**In scope.** `AboxCheckInput`, the **fast-path** call site only, closure reuse there, and the
gates below.

**Out of scope.** Any change to what `abox_check` *decides* (P1–P9 are untouched). `realize`,
`materialize_*`, `is_consistent`, `disjointness`, `individuals`, `property_values` — they keep
using `PreparedOntology` unchanged. The DKey-disjointness blowup at conversion (the other
salvaged finding, and where `ore_ont_9347`'s 42 GB actually lives).

## Gates

1. **Verdict identity — the load-bearing gate.** For every ABox-bearing fixture and every
   synthetic in `abox_check`'s existing 16 unit tests, the verdict before and after must be
   identical, including the `reason`. This is the whole correctness claim; it should be checkable
   by construction and also tested.
2. **FP=0 / MISSED=0.** `./scripts/run-soundness-diff.sh` — reference closures galen 27997,
   notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158,
   ore-15672 142, sulo 51, bibtex 16. **Mandatory locally**: the CI job is a `workflow_dispatch`
   stub with unprovisioned fixtures, so this is the only FP=0 evidence the change will get.
3. **Full-output identity on ABox fixtures.** `classify` output byte-identical before/after on
   `alehif-test`, `wine`, `family`, `pizza`, `ro`, `ore_ont_15167`. Unlike the retired lever, the
   curated corpus *can* gate this one — no nominal-free precondition means every ABox fixture
   exercises it.
4. **`realize` / `is_consistent` / `materialize_*` unchanged** on the same fixtures — proves the
   extraction did not leak into the ABox-dependent consumers.
5. **Recovery, per ontology.** `ore_ont_10073`, `10068`, `1012`, `1043` — report wall and RSS
   before/after individually. Expect less than the `ABOX_CHECK=0` upper bound; state the fraction
   achieved rather than quoting the bound.
6. **The 50k–180k band.** Sample ≥5 ontologies there and report. If they are inert, say so — the
   addressable population is then ~84, not ~192.

## Rollout

**No flag needed for correctness**, because the change is verdict-identical by construction rather
than by argument — there is nothing to A/B for soundness. Land it directly, gated on 1–4.

If a flag is wanted for perf A/B, gate only the *skipping* (`RUSTDL_ABOX_CHECK_REDUCED`,
default ON, `=0` restores the full build) so a regression can be bisected without reverting.
Do not gate the `AboxCheckInput` extraction itself — a compile-time dependency narrowing has no
runtime variant.

## What this does not claim

- It does not fix `ore_ont_9347` (42 GB in DKey disjointness at conversion — separate finding).
- It does not recover any DNF ontology. Every ontology measured here already completes; `11311`
  DNFs both with and without the check. **This is a wall/RSS lever, not a completeness lever.**
- It does not change any verdict, by design. If it does, that is a bug, not a trade-off.

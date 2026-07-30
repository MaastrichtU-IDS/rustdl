# Reduced-input `abox_check`: stop building a full `PreparedOntology` to read one verdict

**Date:** 2026-07-30
**Status:** Design — ready for implementation planning
**Origin:** salvaged finding 2 of `2026-07-30-tbox-only-prepare-design.md` (that lever retired at
zero payoff; this one replaced it and is better evidenced)
**Flag:** none needed for correctness — see § Rollout

## The waste

Both classify entry points construct a **full** `PreparedOntology` — EL saturation, told tables,
`HyperCache::build`, NNF + absorb, complements — solely to read `abox_verdict()`, then proceed:

- fast path: `classify.rs:785-787`, then `return classify_pure_el(...)`
- top-down path: `classify.rs:1626`, verdict read at `:1631`

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

**3 of 8 show 20–40%, all above 180k assertions.** Small-ABox ontologies are inert.

Population, for scale only: of 1,920 pool ontologies, 1,146 have an ABox, 654 have ≥1k
assertions, 192 have ≥50k, **84 have ≥180k**.

**Do not read 84 as 84 wins.** That is a feature-presence count, and `ore_ont_10127` already
falsifies a naive threshold — 105k assertions, 0% saving, because its 19 s wall is dominated by
something that makes a 0.6 s prepare invisible. Three prior estimates this month died exactly
this way (83 ABox-bearing "addressable" → 0/4 rescued; a 5-ontology ABox-filter payoff → 0;
a multiplicative mechanism → refuted). The honest claim is: **3 of 3 sampled above 180k
assertions saved 20–40%; the 50k–180k band is untested; validate the population before quoting
it.**

## Why this is a better lever than the one it replaced

| | retired ABox-filter lever | this |
|---|---|---|
| soundness argument needed | yes — nominal-freedom + consistency, with an inconsistency-path obligation | **none** |
| verdict change | possible if the contract is broken | **verdict-identical by construction** |
| premise | ABox irrelevant to class subsumption | none — same inputs, same answer |
| applies to nominal-bearing ontologies | no | **yes** |
| measured payoff | 0 | 20–40% on 3/3 sampled >180k |

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

`abox_check::check` takes that instead of `&PreparedOntology`. Both classify sites build the eight
values directly and pass them, skipping `HyperCache::build`, `ConsistencyCache::build`,
`snapshot_cache`, and absorb entirely for the check.

**Why a struct rather than a reduced constructor.** A second `PreparedOntology` constructor would
leave two objects of the same type with different completeness, and the id-space hazard the
`ConsistencyCache` doc already records (`lib.rs:3159`: "a mismatched hierarchy would let an
unrelated edge satisfy a super-role atom = false clash") becomes reachable by accident. An
explicit input type makes the dependency set checked by the compiler and cannot drift — if someone
later makes `abox_check` read `hyper`, it will not compile.

**Fold in the duplicated saturation.** `from_internal` calls
`owl_dl_saturation::saturate(&internal)` at `lib.rs:4567`, but both callers already hold a
closure — `classify_pure_el` is passed one, and `classify_top_down_internal` computes one before
the prepare. `AboxCheckInput` should borrow the caller's closure, removing a second full EL
saturation per classify. This is a separate waste from the `hyper`/`tbox` one and is measured
inside the same 0.62 s.

**Also remove `ConsistencyCache` from the classify path.** `prepared.consistency` is read only at
`lib.rs:4002/4046/4668/4684` — 4002 inside `is_consistent_internal_full`, the rest on the same
consistency path. **`classify.rs` never reads the field** (its 11 textual hits on "consistency"
are comments about the *ABox pre-check*, a different mechanism — verified, not assumed). Yet it is
built whenever `wedge_consistency_enabled() && internal_has_abox`. On a large-ABox ontology that
re-clausifies the whole axiom set a second time. Dead-field removal on this path; no soundness
surface.

## Scope

**In scope.** `AboxCheckInput`, both classify call sites, closure reuse, skipping
`ConsistencyCache` on the classify path, and the gates below.

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

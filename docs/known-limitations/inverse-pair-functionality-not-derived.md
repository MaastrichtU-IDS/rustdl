# Functionality is not propagated across a declared `InverseObjectProperties` pair

**Found:** 2026-08-05 · **Status:** **MECHANISM FIXED** behind `RUSTDL_INVERSE_PAIR_FUNC`
(default OFF); the full ontologies remain a **performance** DNF · **Severity:** wrong
`consistent` verdict
**Oracle:** Konclude 0.7.0 **and** HermiT 1.4.3, independently
**Fixtures:** `crates/owl-dl-reasoner/tests/fixtures/inverse_functional_derivation/`

## The defect

rustdl honours **declared** role characteristics but never **derives** one from the other across a
declared inverse pair. Both directions are missing:

- `InverseObjectProperties(R, S)` + `FunctionalObjectProperty(R)` ⟹ `S` is inverse-functional — **missed**
- `InverseObjectProperties(R, S)` + `InverseFunctionalObjectProperty(R)` ⟹ `S` is functional — **missed**

The derivation is elementary: if `S = R⁻` and `R` is functional, then `S⁻ = R` is functional, which is
exactly what "`S` is inverse-functional" means.

## Minimal reproducer — 5 axioms

`a-derived-inverse-functional.ofn`:

```
InverseObjectProperties(:hasPart :partOf)
FunctionalObjectProperty(:hasPart)
ObjectPropertyAssertion(:partOf :a :c)
ObjectPropertyAssertion(:partOf :b :c)
DifferentIndividuals(:a :b)
```

Both assertions invert to `hasPart(:c, :a)` and `hasPart(:c, :b)`; `hasPart` is functional, so
`:a = :b`, contradicting `DifferentIndividuals`.

| | verdict |
|---|---|
| Konclude | inconsistent |
| HermiT | inconsistent (`InconsistentOntologyException`) |
| **rustdl 0.4.14** | **`consistent`** ✗ |

Unchanged by `RUSTDL_ABOX_SATURATION=0`, `RUSTDL_ABOX_CHECK=0`, `RUSTDL_DKEY_MERGING_GATE=0`.

## Two controls prove the scope is exactly the inverse step

Both are caught, so neither the functional merge nor the clash machinery is at fault:

| fixture | difference from the reproducer | rustdl |
|---|---|---|
| `d-direct-functional-CONTROL.ofn` | asserts `hasPart` **directly**, no inverse | inconsistent ✓ |
| `e-declared-inverse-functional-CONTROL.ofn` | **declares** `InverseFunctionalObjectProperty(:partOf)` | inconsistent ✓ |
| `f-derived-functional.ofn` | the **reverse** derivation | **`consistent`** ✗ |

`e` versus `a` is the sharpest pair: semantically equivalent inputs, one declared and one requiring
one inference step, and only the declared one is decided.

## Code site

`crates/owl-dl-reasoner/src/abox_check.rs:286-296` builds both sets **purely from declared axioms**:

```rust
Axiom::FunctionalRole(r)        => { functional_roles.insert(r.role_id()); }
Axiom::InverseFunctionalRole(r) => { inverse_functional_roles.insert(r.role_id()); }
```

A repo-wide grep finds **no** derivation of `InverseFunctionalRole` from
`InverseObjectProperties` + `FunctionalRole` anywhere in `crates/`. `abox_saturation.rs` has the
matching gap: its comment at `:254` reads *"Only Named roles (not inverse) are stored"*, and its
`functional` set is keyed `(RoleId, bool)` but populated only from declared axioms.

Note `expand_role_characteristics` (`reasoner/src/lib.rs:5949`) is **not** the missing piece — its
documented job is appending `SubClassOf` / `InverseObjectProperties`, and `⊤ ⊑ ≤1 r.⊤` for functional
roles; it does not cross-propagate characteristics between inverse partners.

## How this was found, and why the real ontologies were unusable directly

`ore_ont_4141` (67,143 axioms) and `ore_ont_8445` (138,737) are both oracle-confirmed inconsistent
while rustdl times out at 200 s on `consistent` *and* `classify`. Delta-debugging with **Konclude as
the oracle** (1.4 s per test, verdict read from log content, never from its exit code) reduced `4141`
from 67,143 axioms to a **7-axiom core** — retained as `ore_ont_4141-7axiom-core.ofn`:

```
InverseObjectProperties(hasPart, partOf)   FunctionalObjectProperty(hasPart)
FunctionalDataProperty(label)
partOf(SubCountry_3146, Country_181)   label(SubCountry_3146, "Évora")
partOf(SubCountry_3147, Country_181)   label(SubCountry_3147, "Faro")
```

Same merge, but the clash lands on a **functional data property** with two distinct string literals
rather than on `DifferentIndividuals`. rustdl reports `consistent` on this core too. The reduction
controls both passed (full set inconsistent, empty set consistent), and a subset Konclude could not
process was counted as *not* inconsistent — the conservative direction, so the core is a genuine
superset of a true minimal core.

`ore_ont_8445`'s reduction was still running when this was written; it is the same ontology family
(BioCaster, 16 `FunctionalObjectProperty` + 32 `FunctionalDataProperty`) and very likely the same
mechanism, but that is **not** yet verified.

## What was built, and what it does NOT fix

`derive_inverse_pair_functionality` in `owl-dl-core/src/convert.rs`, behind
`RUSTDL_INVERSE_PAIR_FUNC` (**default OFF**), runs at conversion time — **not** in
`expand_role_characteristics`, because `build_abox_check_inputs` clones `axioms` at
`lib.rs:5948` *before* calling that pass at `:5949`, so a derivation placed there would
be invisible to `abox_check`, the one consumer that needs it. It runs before
`derive_functional_max_cardinality` so a newly-functional role also gets that pass's
`∃R.⊤ ⊑ ≤1 R` enforcement GCI.

| fixture | flag OFF | flag ON |
|---|---|---|
| `a-derived-inverse-functional` | consistent ✗ | **inconsistent ✓** |
| `f-derived-functional` (reverse direction) | consistent ✗ | **inconsistent ✓** |
| `g-chained-needs-fixpoint` (two-link chain) | consistent ✗ | **inconsistent ✓** |
| `d-direct-functional-CONTROL` | inconsistent ✓ | inconsistent ✓ |
| `e-declared-inverse-functional-CONTROL` | inconsistent ✓ | inconsistent ✓ |
| **`ore_ont_4141-7axiom-core`** | consistent ✗ | **inconsistent ✓** |

**Part 1 alone was NOT enough**, and finding out why produced the actual fix. Deriving
`InverseFunctionalRole(S)` closes only the `DifferentIndividuals` route, because the
engine does not merge **predecessors** — `derive_functional_max_cardinality` is
forward-only by design, `∃R⁻.⊤ ⊑ ≤1 R⁻` being a measured no-op. Ablation showed the
*direct* analogue is decided even with **both** `ABox` pre-checks disabled, i.e. on the
tableau via the forward `≤1` rule.

**Part 2 therefore reuses that proven forward path rather than building predecessor
merge:** for a declared pair `(R, S)` with `R` functional, every asserted `S(a, b)`
entails `R(b, a)`, so materialising that edge lets `∃R.⊤ ⊑ ≤1 R` + `apply_max` fire at
`b`. Confirmed by hand-adding the edges before implementing anything: `consistent` →
`inconsistent`. Bounded to functional partners on purpose, so edge growth tracks
assertions on the partner of a functional role rather than the whole `ABox`.

## SCOPE: the core is decided, the full ontologies are NOT

| | flag OFF | flag ON |
|---|---|---|
| 7-axiom core | consistent ✗ | **inconsistent ✓** |
| full `ore_ont_4141` (67,143 axioms) | TIMEOUT @300 s | **TIMEOUT @300 s** |
| full `ore_ont_8445` (138,737 axioms) | TIMEOUT @300 s | **TIMEOUT @300 s** |

**So this closes the mechanism gap, not the two ontologies.** The clash is reachable only
on the tableau path, and that path does not scale to a 67k-axiom `ABox`. Deciding the full
ontologies needs the clash in a **pre-check**: `abox_check`'s P5 re-tests only
`different_pairs` after a merge, never functional-**data**-property value conflicts, and
`abox_saturation`'s merge populates its `functional` set from declared axioms only (its own
comment at `:254`: *"Only Named roles (not inverse) are stored"*). That remains open and is
now the binding item for these two ontologies.

## Gates run

- **Canaries:** `crates/owl-dl-reasoner/tests/inverse_pair_functionality.rs`, 6 tests.
- **Sabotage: 6 mutations run, 4 caught, 2 survived — both survivors named rather than
  hidden.** Caught: one-directional derivation; flag ignored; materialisation removed;
  materialised edge direction not swapped. **Survived: (a) replacing the fixpoint loop
  with a single pass** — tried twice, including with adverse source ordering, so the
  fixpoint is UNCOVERED and may be redundant; **(b) removing the `functional` bound on
  materialisation**, which is expected, since the bound is a *cost* property and no
  correctness canary can see it. The bound's justification is the DKey discriminators
  below, not a unit test. A future simplifier should not read these canaries as protecting
  either property.
- **FP=0 net with the flag ON:** all present fixtures VERIFIED, every closure exact and
  unchanged from the reference values (galen 27997, notgalen 32739, sio 8904,
  ore-10908 6001, pizza 499, alehif 247, ro 158, ore-15672 142, bibtex 16). That is
  **inertness** on the curated corpus, not evidence of correctness under load — the shape
  needs an inverse pair *and* a functional role together, which no curated fixture has.
- **DKey gate discriminators unchanged**, which was the specific perf risk since these
  axioms feed `merge_inducing`/`collapse` at `convert.rs:3066`/`:3169`:
  `ore_ont_9347` = 113 and `ore_ont_5368` = 18,620,251 concept rules at **both** flag
  settings — re-verified after Part 2, which matters more there because materialised
  object assertions feed the same component analysis.
- `cargo fmt` clean; `clippy -D warnings` clean; **1,586 tests pass, 0 fail**.

**Not run, so not claimed:** a full 1,920-ontology two-arm sweep. That is required before
any default flip, because the change adds role characteristics on every ontology carrying
an inverse pair beside a functional role, and no MISSED-net arm was run either.

## Original suggested fix, and the gate it needs

A preprocessing derivation over `InverseObjectProperties(R, S)` — symmetric, so apply in both
directions:

```
FunctionalRole(R)        ⟹ InverseFunctionalRole(S)
InverseFunctionalRole(R) ⟹ FunctionalRole(S)
```

**Direction of risk is inverted from most work here.** This *adds* role characteristics, so the KB
gets stronger and more clashes become derivable — the failure mode is a **false positive**, not a
miss. It therefore needs the full FP=0 net rather than a completeness check, plus a corpus sweep,
because new functional roles mean new merges on every ontology declaring an inverse pair alongside a
functional role. That is not a rare shape, so **do not assume it is cheap.** The four fixtures above
are the correctness canaries: `a` and `f` must flip to inconsistent, and `d` and `e` must stay
inconsistent.

## 2026-08-08 re-measurement: the blocker is bigger than recorded, and it is not the enforcement GCI

Re-measured on 0.4.16 (`9d44680`) while scoping the "recover the +17" follow-up.

**`ore_ont_16372` flag-ON is NOT a DNF.** Every prior arm capped it at 120–150 s and
recorded `dnf`; with a 200 s cap it **completes in 194.57 s**. That matters because the
real shape is worse than a DNF is, not better:

| | wall | rows |
|---|---:|---:|
| flag OFF | **3.66 s** | **2,236** |
| flag ON | 194.57 s | **749** |

A **53× slowdown** *and* a **two-thirds answer loss** — the missing rows are pairs that
time out and default to `not-subsumed` (sound, but a large completeness loss).

**The +17 is currently unrealised.** `ore_ont_13859` reports **971 rows at both flag
settings** on this binary: the reorder that fixed 3 of 4 regressions did cost the gain, as
suspected. So today the flag is **strictly harmful** — no benefit anywhere, one severe
victim.

### Where the flag-ON cost actually is

`--global-timeout-ms 60000` forces a banner out of the slow arm:

| | flag OFF | flag ON |
|---|---|---|
| label heuristic | pruned=98,697 pass_through=1,609 **misses=0** | pruned=10,079 pass_through=244 **misses=54** |
| `label_cache_build` | 747 ms | 749 ms (**unchanged**) |
| `tier_walk` | **1,669 ms** | **58,156 ms** |

The cache builds in the *same* time and yields **54 `NoVerdict` classes**. This is the
same all-or-nothing property documented in
`docs/2026-08-08-label-cache-aggregate-bound.md`: 54 unprunable classes × 744 ≈ 40k
unprunable pairs, and pruning collapses ~10×. **The flag's cost is not in preprocessing,
absorption, or the merge — it is 54 poisoned label classes.**

Part 2 (materialisation) is **inert here**: `ore_ont_16372` has **zero**
`ObjectPropertyAssertion`. So the cost comes purely from Part 1's derived
`InverseFunctionalRole` axioms. Ablations that do **not** fix it:
`RUSTDL_INVERSE_FUNC_MERGE=0`, `RUSTDL_CLASSIFY_BACKFOLD=0`, `RUSTDL_HYPERTABLEAU=0`,
`RUSTDL_DOMAIN_ABSORPTION=0`, `RUSTDL_ITERATIVE_DEEPENING=0`, and
`RUSTDL_LABEL_CACHE_TIMEOUT_MS` at 5,000 or 30,000 (misses stay at 54 — they are **not**
budget-starved).

### Hypothesis raised and REFUTED: the adaptive-budget early-cut

`RUSTDL_ADAPTIVE_BUDGET=0` takes misses **54 → 14** and `tier_walk` 58,156 → 28,606 ms,
which implicates the divergence early-cut in *creating* the `NoVerdict`s. It also suggests
a plausible asymmetry: a `Stalled` costs **O(1)** in the pair oracle (one pair defaults to
not-subsumed) but **O(n)** in the label cache (it poisons every pair involving that class),
so exempting the label path looks principled.

**Measurement refutes it.** With no global cap:

| | flag ON | flag OFF |
|---|---|---|
| `ADAPTIVE_BUDGET=1` | 194.57 s ✓ | **3.66 s** |
| `ADAPTIVE_BUDGET=0` | **dnf @200 s** | 37.26 s |

Disabling the early-cut makes the flag-ON arm **worse**, and costs the flag-OFF baseline
**10×**. The early-cut is load-bearing on this ontology, not the culprit; it converts some
classes to `NoVerdict` but prevents far more work than it causes.

### Consequence for the "recover the +17" plan

The plan was selective enforcement-GCI emission. Two findings retire it as the next step:

1. **It targets the wrong problem.** `16372` regresses *without* any enforcement GCI (that
   is what the reorder removed), so re-adding GCIs selectively cannot fix it and can only
   add risk back.
2. **Any count-based gate would be a lookup table.** The beneficiary has **14** inverse
   pairs; the three GCI-caused regressors have 76–118; but `16372` has **20**. A threshold
   admitting 14 while excluding 20 is a constant fitted to one ontology, not a criterion.

**The binding item is the 54 poisoned label classes** — specifically, why derived
`InverseFunctionalRole` axioms make those classes' wedge satisfiability searches diverge
when the same ontology's declared characteristics do not. That is a wedge-search question,
not a preprocessing one, and it is where any further work on this flag should start.

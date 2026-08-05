# Functionality is not propagated across a declared `InverseObjectProperties` pair

**Found:** 2026-08-05 · **Status:** **PARTIALLY FIXED** behind `RUSTDL_INVERSE_PAIR_FUNC`
(default OFF) · **Severity:** wrong `consistent` verdict
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
| **`ore_ont_4141-7axiom-core`** | consistent ✗ | **consistent ✗ — STILL MISSED** |

**The motivating ontology is NOT fixed, and that is the honest headline.** Probes isolate
why: the real core's clash arrives through a **functional data property** on the merged
individuals, and `abox_check`'s P5 re-tests only `different_pairs` after a merge — never
data-value conflicts. Meanwhile the *directly* asserted analogue **is** caught, so the
merge-plus-data-clash route works when no inverse is involved. Deriving the
characteristic is therefore necessary but **not sufficient**.

**The second, separable defect** (still open): after a functional/inverse-functional
merge, the post-merge re-check must revisit functional-**data**-property value conflicts,
not just `DifferentIndividuals`. Either `abox_check` P5 gains that re-check, or
`abox_saturation`'s merge learns to honour inverse-functionality (it keys its
`functional` set `(RoleId, bool)` but populates it only from declared axioms, and its
own comment at `:254` says *"Only Named roles (not inverse) are stored"*).

## Gates run

- **Canaries:** `crates/owl-dl-reasoner/tests/inverse_pair_functionality.rs`, 6 tests.
- **Sabotage: 2 of 3 caught, and the survivor is recorded.** Making the derivation
  one-directional fails `derived_functional_needs_the_flag`; ignoring the flag fails both
  flag-OFF assertions. **Replacing the fixpoint loop with a single pass leaves everything
  green** — tried twice, including with the inverse-pair axioms in deliberately adverse
  source order. So **the fixpoint loop is UNCOVERED and may be redundant**; a future
  simplifier should not treat these canaries as protection.
- **FP=0 net with the flag ON:** all present fixtures VERIFIED, every closure exact and
  unchanged from the reference values (galen 27997, notgalen 32739, sio 8904,
  ore-10908 6001, pizza 499, alehif 247, ro 158, ore-15672 142, bibtex 16). That is
  **inertness** on the curated corpus, not evidence of correctness under load — the shape
  needs an inverse pair *and* a functional role together, which no curated fixture has.
- **DKey gate discriminators unchanged**, which was the specific perf risk since these
  axioms feed `merge_inducing`/`collapse` at `convert.rs:3066`/`:3169`:
  `ore_ont_9347` = 113 and `ore_ont_5368` = 18,620,251 concept rules at **both** flag
  settings.
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

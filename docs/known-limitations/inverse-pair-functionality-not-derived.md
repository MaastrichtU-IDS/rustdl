# Functionality is not propagated across a declared `InverseObjectProperties` pair

**Found:** 2026-08-05 · **Status:** open, unfixed · **Severity:** wrong `consistent` verdict
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

## Suggested fix, and the gate it needs

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

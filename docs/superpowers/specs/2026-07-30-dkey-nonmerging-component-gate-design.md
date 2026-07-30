# Skip DKey disjointness for non-merging role components

**Date:** 2026-07-30
**Status:** Design — prototyped and measured; ready for implementation planning
**Flag:** `RUSTDL_DKEY_MERGING_GATE`, default **ON**, `=0` reverts
**Prototype:** branch `perf/dkey-nonmerging-component-gate`, commit `3aae033` (WIP: no flag, no
dedicated tests)

## Summary

`convert_ontology` materialises `DisjointClasses(DKey(a), DKey(b))` for O(k²) pairs of data values
even when **no axiom in the ontology can ever put two DKeys in one node label**. On
`ore_ont_9347` that is 49,571,087 axioms, and classify DNFs at 600 s using **70.7 GB**. Gating the
seeding on whether the DKeys' role component contains a merge-inducing role reduces it to **113
axioms** and classify to **11.0 s / 0.226 GB** — a 313× RSS reduction and DNF → completes.

## The defect

`DataPropertyAssertion(p, a, v)` lowers to `ClassAssertion(a, ∃p.DKey(v))`, so an ABox with k
distinct data values mints k DKey classes, all appearing under role `p`.

The bounded seeding shipped in v0.3.29 (`RUSTDL_BOUNDED_DKEY_DISJOINT`) already reasons correctly
about *when* a disjointness axiom is consumable — its own comment:

> A disjointness axiom is only ever CONSUMED when both DKeys land in ONE node label, which requires
> their data roles to be connected through a merge-inducing super-role (functional /
> inverse-functional / in a `≤n` / carrying a `∀role.DKey` or a DKey-range).

It computes exactly that predicate as `m_star` (`convert.rs`, step (a) + downward closure through
sub-roles) and uses it to gate the **union** of role components in step (d):

```rust
for &(sub, sup) in &edges {
    if m_star[sup] { uf.union(sub, sup); }      // union gated on m_star -- correct
}
```

But step (e) then anchors a DKey to `uf.find(role)` for **every** `Some` / `All` / `Min` / `Max`
occurrence, with no `m_star` condition:

```rust
| ConceptExpr::Max(_, r, f) => anchor(&mut uf, &mut components, *r, *f),   // every role
```

So all k DKeys on one data property land in one component and are seeded all-pairs — **C(k,2)** —
regardless of whether anything can merge them. The union is gated; seeding a component *with
itself* is not.

**Why those pairs are dead weight.** `∃p.DKey_a ⊓ ∃p.DKey_b` is satisfiable with two **distinct**
`p`-successors. Nothing forces both keys onto one node unless `p` is functional /
inverse-functional / occurs in a `≤n` / carries `∀p.DKey` or a DKey range — i.e. unless `m_star[p]`.
A component containing no `m_star` role therefore can never co-label, and every pair seeded within
it is unusable.

`ore_ont_9347` is the pure case: 19,160 `DataPropertyAssertion` and **zero** of
`DataSomeValuesFrom`, `DataAllValuesFrom`, `DataHasValue`, `FunctionalDataProperty`,
`DataPropertyRange`. Nothing in the file can consume a single one of its 49.6M axioms.

## Design

Compute, once, the set of components that contain at least one merge-inducing role, and treat a DKey
as anchored only to those:

```rust
// after step (d)'s union loop
let merging_comps: HashSet<usize> =
    (0..num_roles).filter(|&r| m_star[r]).map(|r| uf.find(r)).collect();

// inside the step-(e) `anchor` closure
let comp = uf.find(role.role_id().index() as usize);
if !merging_comps.contains(&comp) { return; }
```

**This reuses an existing safe path rather than adding one.** A DKey left with no component falls
into the branch `seed_disjoint_bucket` already has:

> Neither anchored nor unanchored: the DKey appears under no role restriction at all — it can never
> reach a node label, so its disjointness is dead weight; skip it entirely.

The gate extends that skip from *cannot be labelled* to *cannot be **co**-labelled*. The
`unanchored` set (direct, non-role-mediated label placement) is untouched and still pairs with
everything, so the conservative fallback for label placements the lowering cannot produce remains
in force.

**Flag.** `RUSTDL_DKEY_MERGING_GATE`, default ON, read once per conversion alongside
`bounded_dkey_disjoint_enabled`. `=0` restores today's behaviour. The gate is meaningful only when
bounded seeding is on; with `RUSTDL_BOUNDED_DKEY_DISJOINT=0` there is no component map, and that
path stays exactly as it is.

## Soundness

The change **removes** axioms, so it cannot introduce a subsumption: fewer disjointness axioms ⇒
fewer clashes ⇒ fewer derived `⊥` ⇒ **never a false positive**. FP=0 is preserved structurally.

The risk is entirely **completeness**: dropping a pair that *would* have been consumed is a MISS.
That is bounded by the argument above — the pair is consumable only via a merge, every merge source
is in `m_star`, and `m_star` is closed downward through sub-roles (so a sub-role of a functional
super is included). Two conservative properties of the existing code are preserved: `role_id()`
ignores inverse polarity (an inverse `≤n` anchors on the same named role) and `m_star` is
over-approximate by construction ("deliberately COARSE in the safe direction (over-union /
over-anchor only)").

## Evidence

**The curated corpus cannot validate this**, and that is pre-existing: `datatype_value_membership.rs`
says outright that "the corpus has NO such clash, so these canaries are the ENTIRE safety net for
`definitely_disjoint`". So the FP=0 net demonstrates **inertness**, not correctness, and the
canaries are the real gate.

| evidence | result |
|---|---|
| `ore_ont_9347` concept_rules | 49,571,087 → **113** (= the `RUSTDL_DATA_PROPERTIES=0` value) |
| `ore_ont_9347` classify | **DNF @600 s / 70.7 GB** → **11.0 s / 0.226 GB**, real subsumptions |
| `ore_ont_5368` concept_rules | 18,620,251 → **18,620,251 unchanged** (15 `FunctionalDataProperty` ⇒ genuinely merging) |
| FP=0 net | 22 passed / 0 failed, every closure exact (galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16) |
| datatype suites | 178 pass across `datatype_value_membership`, `classify_concrete_domain`, `data_properties`, `datatype_inconsistency` |

**Safety net verified NON-VACUOUS by sabotage.** Gating unconditionally (zero DKey pairs ever)
**fails exactly three** canaries — `forall_value_outside_range_clashes`,
`forall_float_value_outside_clashes`, `forall_string_value_outside_enum_clashes` — and they pass
with the real gate. So the gate demonstrably preserves the pairs that are actually consumed, rather
than passing because nothing was testing them.

Correction to the consumer survey: `forall_complement_value_in_range_clashes` passes even with zero
pairs, so it clashes by another mechanism. **Three** canaries guard DKey-vs-DKey disjointness, not
four.

## Scope

**In scope.** The `merging_comps` gate, its flag, dedicated tests (a non-merging fixture asserting
zero seeded pairs; a merging fixture asserting pairs survive), and the gates below.

**Out of scope — Lever 2, the on-demand disjointness oracle.** The originally-chosen design was to
stop materialising DKey disjointness entirely and answer it on demand from a predicate over the two
decoded IRIs. Measurement redirected: this gate solves the `9347` class in ~8 lines with **no engine
plumbing**, whereas the oracle needs new side-table hooks in **four** consumers, three of which have
none today —

- `owl-dl-saturation/src/lib.rs:3145` (`SatRules::disjoint_pairs`) — no hook
- `owl-dl-tableau/src/hyper.rs:1194` (`build_disjoint_pairs` → `HyperEngine::disjoint_pairs`) — no hook
- `owl-dl-reasoner/src/abox_saturation.rs:418` — no hook
- classic tableau — **has** the precedent (`set_dkey_ranges`, `lib.rs:344`, read by
  `concrete_domain_clash`, `lib.rs:1617`)

The oracle remains the only lever for genuinely-merging large components (`5368`: 6,101 values on
functional data properties, unchanged by this gate). Defer it until the population count says how
much residual it addresses. Encouraging structural finding for whenever it is built: **no consumer
iterates the full pair set** — every one is a point query or iterates node labels — so an oracle is
architecturally feasible.

**Also out of scope.** Any change to `definitely_disjoint` / the range algebra, and the `unanchored`
fallback.

## Gates

1. **Flag-OFF byte-identity.** With `RUSTDL_DKEY_MERGING_GATE=0`, conversion output and `classify`
   must be byte-identical to pre-change `main` on every ABox/data fixture. Proves the gate is the
   only behavioural delta.
2. **The three canaries, ON and OFF.** All must pass with the gate ON. Re-run the sabotage
   (unconditional gate) and confirm those same three FAIL — if they do not, the net is not
   protecting the pairs and the evidence for this change is void.
3. **FP=0 net.** `./scripts/run-soundness-diff.sh` — 22/0 with the reference closures above.
   Understood to show inertness, not correctness.
4. **Recovery, per ontology.** `ore_ont_9347` and `ore_ont_5368`: report concept_rules, wall, and
   RSS before/after individually, from **pinned** binaries. (A rebuild under a running measurement
   already invalidated one scan in this session — pin the binaries, do not rebuild mid-run.)
5. **Population, before/after on pinned binaries.** How many ORE ontologies does the gate reduce,
   and how many remain ≥1M after (the residual the oracle would address)? Report the count actually
   *reduced*, not the count that merely *has* data-property assertions — the present-vs-binding
   distinction this repo has repeatedly got wrong.
6. **New dedicated tests.** A conversion-level test asserting **zero** `DisjointClasses` seeded for
   a non-merging fixture, and a companion asserting pairs ARE seeded once the same fixture's
   property is made `Functional`. That pair pins the gate's boundary directly rather than through a
   reasoning outcome.

## Retracted measurement — a sabotaged binary was mistaken for the gate (2026-07-30)

**A population scan reporting "443 of 1,893 ontologies reduced, 0 residual ≥1M" is INVALID and must
not be cited.** Its "after" binary was the **sabotage** build, not the gate.

How it happened: to prove the canaries non-vacuous, the `anchor` gate was temporarily forced to
`if true { … return; }` (skip *every* component ⇒ zero DKey pairs ever), rebuilt, the 3 expected
canary failures observed, and then **the source was reverted without rebuilding**. A later
`cp target/release/rustdl …/rustdl-after` copied that stale sabotaged binary.

Why the sanity check missed it: the check used `ore_ont_9347`, which reads **113 under both** the
real gate and the sabotage — all its pairs are dead weight either way. `ore_ont_5368` is the case
that discriminates, and it read **12,201** (the zero-pairs value) instead of 18,620,251.

**The shipped code is unaffected and correct.** Verified on a freshly built binary from branch HEAD:
`ore_ont_9347` → 113, `ore_ont_5368` → **18,620,251** — i.e. exactly the claim in § Evidence, that
`5368` is untouched because it is genuinely merge-inducing. Only the *measurement* was wrong.

**Discriminating check for any future measurement of this gate** — `9347` alone cannot tell the gate
from a no-pairs build:

| binary | `9347` | `5368` |
|---|---|---|
| pre-change `main` | 49,571,087 | 18,620,251 |
| **gate ON (correct)** | **113** | **18,620,251** |
| sabotage / zero-pairs | 113 | 12,201 |

**Process rule, learned twice in one session.** Both measurement failures here had the same cause:
reusing `target/release/rustdl` across configurations. Pin a binary to a uniquely named path
*immediately after the build that produced it*, name the path after the configuration, and verify
the pin against a case that discriminates the configurations before trusting a scan built on it.
(The first instance was rebuilding `target/release` while a scan was reading it.)

## What this does not claim

- It does not help `ore_ont_5368` or any ontology whose data properties are genuinely
  merge-inducing. That is Lever 2.
- It is not validated by the curated corpus — see § Evidence. The canaries are the net.
- It does not change `definitely_disjoint`, so the FP surface CLAUDE.md flags for D11b is untouched.

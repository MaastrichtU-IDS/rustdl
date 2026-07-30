# DKey collapse-vs-broadcast fixtures

Produced by two independent adversarial reviews (2026-07-30) of the proposed
collapse/broadcast split in DKey-disjointness seeding. See
`docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md`.

**Why these exist.** The naive form of that lever — a per-component early return in
`dkey_components`' `anchor` closure — would silently destroy working concrete-domain
clashes, including the flagship D11b one. These fixtures are the evidence for that,
and they are the gate any implementation must pass. They were verified against
`main` at `ef41128` on 2026-07-30; every "expected" column below is a **measured**
value on that commit, not an intention.

Two different questions are being asked, and confusing them wastes time (it did):

- **"class unsat"** — the KB stays *consistent*; a named class becomes
  unsatisfiable. Test with `rustdl classify --json` and read `unsatisfiable`.
- **"inconsistent"** — the whole KB is inconsistent. Test with `rustdl consistent`.

| fixture | asks | expected on `ef41128` | guards |
|---|---|---|---|
| `two-disjoint-ranges.ofn` | class unsat | `C` unsat | broadcast×broadcast: two disjoint `DataPropertyRange` on one property meet on every successor |
| `range-vs-value-d11b-flagship.ofn` | class unsat | `C` unsat | **broadcast×value — the D11b flagship clash.** A per-component drop kills this |
| `two-ranges-class-unsat.ofn` | class unsat | `A` unsat | broadcast×broadcast, class-expression form |
| `exists-plus-two-forall-dataoneof.ofn` | class unsat | `A` unsat | **occurrence-position rule (R2).** `DataOneOf("a")` interns to the SAME `ClassId` as an assertion's key, so "singleton ⇒ value key" regresses this |
| `range-vs-datahasvalue.ofn` | inconsistent | `inconsistent` | broadcast×value via the ABox |
| `range-on-super-value-on-sub.ofn` | inconsistent | `inconsistent` | broadcast rides DOWN the property hierarchy; also why the union-find must stay gated on the full merge set (R3) |
| `functional-super-values-on-sub.ofn` | inconsistent | `inconsistent` | **COLLAPSE must be closed downward (R4)** |
| `functional-3-level.ofn` | inconsistent | `inconsistent` | R4, three levels, middle role broadcast-only |
| `downward-closure-two-subs.ofn` | class unsat | `C` unsat | R4 via two sub-roles sharing a functional super |
| `NEGATIVE-functional-sub-values-on-super.ofn` | class unsat | **0 unsat** | negative control: the downward closure is NOT needed upward. Must stay satisfiable — if it ever goes unsat, something over-approximates |
| `KNOWN-MISS-forall-super-value-sub.ofn` | inconsistent | **`consistent` (a MISS)** | a **pre-existing** incompleteness, NOT caused by any gate: `∀f.DataOneOf` on a super + conflicting value on a sub is missed, while the `ObjectPropertyRange` form (`range-on-super-value-on-sub.ofn`) works. Asymmetric `∀`-propagation down the data-property hierarchy. Deserves its own ticket; pinned here so it is not mistaken for a regression |

## Using them as a gate

For any change to `dkey_components` / `seed_disjoint_bucket`:

1. Confirm every row above still holds. A change that flips any of the first nine has
   lost a consumable clash — a completeness regression, not a trade-off.
2. Then **sabotage**: make the drop unconditional and confirm the first nine FAIL. If
   they still pass, they are not guarding the emission policy and provide no evidence.
   An earlier gate in this area passed while guarding nothing, which is why this step
   is written down.

## Note on punning

The related regression `crates/owl-dl-reasoner/tests/dkey_nominal_range_merge.rs` uses
object/data punning (OWL 2 Full, not DL), which rustdl accepts because `Vocabulary`
interns object and data properties into one role space. The fixtures here do not
depend on punning.

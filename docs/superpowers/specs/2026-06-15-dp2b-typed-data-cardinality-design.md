# DP-2b: Typed/faceted from-type data-cardinality inconsistency — Design

**Date:** 2026-06-15
**Status:** Approved (brainstorming), pre-plan
**Author:** rustdl (Michel Dumontier + Claude)

## Goal

Detect ABox inconsistencies of the form: an individual `a` is typed (via the
told `SubClassOf` closure) into a class `C` carrying a `≤n dp.dr`
data-cardinality constraint, yet `a` is asserted **more than `n` distinct
data-property values that are provably in `dr`**. Emit `Top ⊑ Bot` (global
inconsistency), matching Konclude's behaviour. This is a sound, preprocessing-
only completeness feature — no tableau/wedge contact.

## Where this sits in the existing matrix

The datatype data-cardinality inconsistency matrix has two cells already filled
and one missing. DP-2b fills the missing cell.

| | `≤1` (functional) | `≤n>1` (from-type) |
|---|---|---|
| **string fillers** | `emit_functional_dp_cardinality_violations` (DistinctVal incl. Str) | `emit_data_cardinality_violations` (exact `xsd:string`, commit 0fc7dd1) |
| **numeric/temporal fillers** | `emit_functional_dp_cardinality_violations` (DistinctVal: Num/Double/Date/DateTime) | **MISSING — this spec** |

The two existing functions stay **untouched** (Approach A). DP-2b adds one new
function. All three emit the same idempotent `Top ⊑ Bot`, so harmless overlap
is acceptable; the existing corpus-validated detections (`ore_ont_12174` string
cardinality; `family.ofn` functional) carry zero regression risk.

Decision (user, 2026-06-15): **facet-aware membership** — DP-2b handles faceted
ranges (`≤2 dp.DatatypeRestriction(xsd:integer minInclusive 0 maxInclusive 10)`)
by counting only values *provably in* `dr`, reusing the D5–D11 range types. This
is strictly more complete than the existing string path's conservative
bare-datatype gating, at the cost of a new membership predicate (the FP surface).

## Architecture

A single new function:

```
fn emit_data_cardinality_violations_typed<A: ForIRI>(
    src: &SetOntology<A>,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
)
```

called from `derive_data_axioms` (`data_axioms.rs`) after the existing
`emit_functional_dp_cardinality_violations`. SetOntology-based, structured like
the functional ≤1 check (which already does told-closure typing, sub-dp routing,
and DistinctVal parsing).

### Data flow

It builds three tables by scanning `src`'s Components:

1. **Individual → told-types.** Reflexive-transitive told `SubClassOf` closure
   over `ClassAssertion` subjects. Reuse `closure_sub_dp` over the atomic
   `SubClassOf` edges (same construction the string path uses via
   `f.subclass_atomic`). Anonymous individuals are ignored (no stable IRI).

2. **`(individual, dp)` → `BTreeSet<DistinctVal>`.** Every
   `DataPropertyAssertion(dp, a, lit)` parsed via the existing
   `literal_to_distinct_val`. `None` (excluded/unrecognized datatype, e.g.
   `xsd:float`, `rdf:langString`, `xsd:boolean`) contributes nothing — the sound
   under-count.

3. **`class → Vec<(dp_iri, n, dr)>`.** From `SubClassOf(C, DataMaxCardinality
   {n,dp,dr})` and `SubClassOf(C, DataExactCardinality{n,dp,dr})` only — never
   `DataMinCardinality`. The raw `dr` is retained for the membership test.
   `EquivalentClasses` is **not** mined here (the existing functions don't
   either; deferred — sound under-detection).

### Check

For each constraint `(C, dp, n, dr)` and each individual `a` whose told-types
contain `C`:

- Gather `a`'s fillers across `dp` **and every sub-data-property `dp' ⊑ dp`**
  (sub→super routing: `dp'(a,v) ⟹ dp(a,v)`, so `dp'`-values count toward `dp`'s
  budget — `closure_sub_dp` over `SubDataPropertyOf` edges, same direction as the
  string path).
- Keep only fillers **provably in `dr`** (Section: membership).
- Count *distinct* survivors (BTreeSet of DistinctVal — exact value identity).
- If `count > n` ⇒ `out.push(Axiom::SubClassOf { sub: top_id, sup: bot_id })`
  and return (one global-inconsistency axiom suffices).

Early-out if there are no class-assertions, no `DataMax`/`DataExact`
constraints, or no data-property assertions.

## Membership predicate (FP-critical)

A value counts toward the `≤n.dr` budget **only when it is provably in `dr`**;
anything uncertain is skipped (under-count — the safe direction; an under-count
can only MISS an inconsistency, never manufacture one). Membership =
**family-match ∧ `contains`**, dispatched on `dr`'s datatype:

| `dr` datatype | range type | value that can count | membership test |
|---|---|---|---|
| `xsd:integer` (bare or faceted) | `IntegerRange` (via `parse_integer_range`) | `Num(Decimal)` **with empty fraction** | `IntegerRange::contains(i64)` |
| `xsd:decimal` (bare/faceted) | `OrdRange<Decimal>` (`parse_decimal_range`) | `Num(Decimal)` (integers fold: `1 = 1.0`) | `OrdRange::contains(&Decimal)` |
| `xsd:double` (bare/faceted) | `FloatRange` (`parse_float_range`) | `Double(OrdF64)` only | `FloatRange::contains(f64)` |
| `xsd:date` | `OrdRange<DateKey>` (`parse_date_range`) | `Date(DateKey)` | `OrdRange::contains` |
| `xsd:dateTime` | `OrdRange<DateTimeKey>` (`parse_datetime_range`) | `DateTime(DateTimeKey)` | `OrdRange::contains` |
| `xsd:string` / string `DataOneOf` | `StrSet` (`parse_string_range`) | `Str(String)` | `StrSet::contains(&str)` |
| unrecognized / unparseable `dr` | — | none | skip |

The `xsd:string`/`DataOneOf` row is **additive** over the existing bare-string
`emit_data_cardinality_violations`: it newly covers faceted and enumerated string
ranges at `≤n>1` (e.g. `≤1 dp.{"a","b"}` + values `"a"`,`"b"` ⇒ 2 distinct
in-range > 1 ⇒ fires). The bare-string path keeps closing `ore_ont_12174`;
overlap on bare `xsd:string` is a harmless idempotent re-emission.

### Membership via `point().subset()` — no new boundary algebra

Every range type already has both a singleton constructor (`IntegerRange::point`,
`FloatRange::point`, `OrdRange::point`, `StrSet::singleton`) **and** a
soundness-reviewed `subset`. So `v ∈ dr` is exactly
`Range::point(v).subset(dr_range)` — this **reuses the already-opus-reviewed
boundary algebra**, adding **zero new boundary FP surface**. (This supersedes
the "new `contains` methods" framing from the approved design — strictly fewer
moving parts and no new endpoint-comparison code.)

The only new code is a value-driven dispatcher and one tiny integer helper:

```rust
// frac-empty (integer-valued) AND fits i64; else None (sound under-count).
fn decimal_as_i64(d: &Decimal) -> Option<i64>

fn value_in_range<A: ForIRI>(v: &DistinctVal, dr: &DataRange<A>) -> bool {
    match v {
        // xsd:integer range first (mutually exclusive parsers); then xsd:decimal.
        DistinctVal::Num(dec) => {
            if let Some(ir) = parse_integer_range(dr) {
                return decimal_as_i64(dec).is_some_and(|i| IntegerRange::point(i).subset(ir));
            }
            parse_decimal_range(dr).is_some_and(|r| OrdRange::point(dec.clone()).subset(&r))
        }
        DistinctVal::Double(f)    => parse_float_range(dr).is_some_and(|r| FloatRange::point(f.0).subset(r)),
        DistinctVal::Date(d)      => parse_date_range(dr).is_some_and(|r| OrdRange::point(d.clone()).subset(&r)),
        DistinctVal::DateTime(d)  => parse_datetime_range(dr).is_some_and(|r| OrdRange::point(d.clone()).subset(&r)),
        DistinctVal::Str(s)       => parse_string_range(dr).is_some_and(|r| StrSet::singleton(s.clone()).subset(&r)),
    }
}
```

The parsers are pairwise mutually exclusive (pinned by the existing D8
`parser_matrix_mutual_exclusivity` canary), so a value is tested against `dr`
only when their datatype families match; any parser returning `None` ⇒ the value
is not provably in `dr` ⇒ not counted (sound under-count). An integer-valued
`Num` against an `xsd:decimal` range correctly counts (integer ⊂ decimal); a
non-integer `Num` (`1.5`) against an `xsd:integer` range correctly does not. A
whole-number value too large for `i64` is a legitimate `xsd:integer` but
unrepresentable here ⇒ skipped (sound under-count, never a false-fire).

`decimal_as_i64` gets unit tests (frac-empty vs non-empty, negative, overflow).

### Soundness invariants

Carried over from the functional check, all preserved:

- **Exact distinctness via `DistinctVal`** — integer+decimal folded into
  `Num(Decimal)` (so `1`/`01`/`1.0` are one value; counting them as distinct
  would be a catastrophic false-fire); `xsd:float` excluded (the f32/f64 lesson);
  language-tagged literals excluded.
- **`DataMax`/`DataExact` only** (never `DataMin`).
- **Sub→super dp routing only** (`dp' ⊑ dp` ⇒ `dp'`-values are `dp`-values; the
  reverse direction would be unsound).
- **Told-closure typing** (reflexive-transitive atomic `SubClassOf`).
- **Anonymous individuals ignored.**

New invariant introduced by facet-awareness:

- **Cross-family values never count** — a `Double` value can never satisfy an
  `xsd:integer`/`xsd:decimal`/temporal/string range, etc. Enforced by the
  family-match column. A non-integer `Num(Decimal)` (non-empty fraction) is **not**
  an `xsd:integer` value and never counts against an integer range.
- **Provably-in-range only** — `parse_*_range` returning `None`, or a `contains`
  that cannot be decided, ⇒ the value is skipped, never counted.

## Testing

Negatives-first. Integration canaries in
`crates/owl-dl-reasoner/tests/datatype_inconsistency.rs`; `contains` unit tests
in `crates/owl-dl-core/src/data_axioms.rs`.

### Fires (`inconsistent`)
- `C ⊑ ≤2 dp.xsd:integer` + `a:C` with 3 distinct integers.
- Same for `xsd:decimal`, `xsd:double`, `xsd:date`, `xsd:dateTime`.
- Faceted: `C ⊑ ≤1 dp.[xsd:integer 0..10]` + `a:C` with two distinct in-range
  integers.
- Sub-dp routing: values split across `dp` and `dp' ⊑ dp` sum past `n`.
- `DataExactCardinality` variant fires identically to `DataMax`.

### Does NOT fire (`consistent`) — the FP guards
- Out-of-range values don't count: `≤1 dp.[0..10]` + values `5, 20` ⇒ only `5`
  in-range ⇒ consistent.
- Cross-datatype: `≤1 dp.xsd:integer` + one integer + one `xsd:double` ⇒ only
  the integer counts ⇒ consistent.
- Non-integer decimal under an integer range (`1.5` vs `≤1 dp.xsd:integer`) ⇒
  not counted ⇒ consistent.
- Duplicate values count once: `1` and `01`; `1` and `1.0` ⇒ one distinct ⇒
  `≤1` consistent.
- Unrecognized `dr` never fires.
- `xsd:float` / language-tagged values excluded ⇒ never counted.
- Anonymous individual ⇒ ignored.
- Boundary: `≤1 dp.[0,5)` (max exclusive) + value `5` ⇒ `5 ∉ range` ⇒ consistent.

### `decimal_as_i64` unit tests
Frac-empty integer parses; non-empty fraction (`1.5`) ⇒ `None`; negative; value
beyond `i64::MAX` ⇒ `None`. (Boundary inclusive/exclusive cases are already
covered by the existing `subset` unit tests — `point().subset()` reuses them.)

### Regression / corpus
- Existing `ore_ont_12174` string-cardinality detection unchanged.
- `family.ofn` functional detection unchanged.
- Full corpus closure-diff FP=0/MISSED=0 (pizza/ro/family/wine/sulo/bibtex/
  shoiq-knowledge/sio/ore-10908/ore-15672); `ore-15516` still inconsistent;
  consistent fixtures stay consistent.

## Review gate

The `contains` predicates are the new FP surface and are soundness-critical
(a false-`contains` over-counts ⇒ false-inconsistent ⇒ catastrophic). Per the
hardened rule, the implementation gets an **opus** spec+quality review, not
sonnet.

## Out of scope (sound under-detection, deferred)
- `EquivalentClasses`-derived cardinality typing.
- `DataMin`-driven detection (needs lower-bound reasoning over range capacity).
- `xsd:int`/derived numeric subtypes in the value/range parsers (the existing
  parsers already scope to the base XSD datatypes).
- Range-capacity counting (`≥3 dp` over a 2-value range) — a concrete-domain
  cardinality reasoner, separately deferred.

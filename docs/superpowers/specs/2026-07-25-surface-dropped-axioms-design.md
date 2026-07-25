# Design: surface dropped axioms + graceful degradation (issue #43)

**Date:** 2026-07-25
**Status:** design approved (brainstorming); pending spec review → implementation plan.
**Author:** Claude + Michel.
**Closes:** #43. Complements the expressivity work (#40 DisjointUnion, #42 datatype
gaps): even after those close, this keeps future drops honest.

## 1. Problem & goal

`convert.rs` handles unsupported constructs two inconsistent ways today:
- **Unsupported data ranges** → `ce_or_skip!` swallows `Err(UnsupportedDataRange)`
  into `Ok(None)`, **silently dropping** the enclosing axiom (invisible
  incompleteness).
- **Other unsupported constructs** — `UnsupportedConcept{kind}`,
  `UnsupportedAxiom{kind}`, `AnonymousIndividual` — return `Err`, which
  `convert_ontology` `?`-propagates → the **whole ontology is refused**
  (`ReasonError::Conversion`). Confirmed: the reasoner calls `convert_ontology(onto)?`
  (e.g. `disjointness.rs:49`), so one unsupported axiom aborts all reasoning.

**Goal:** make conversion **degrade gracefully** — reason over the supported
fragment and **surface a diagnostic** (count + kinds of dropped axioms) on stderr,
in `--json`, and in Python — so CLI users and the Protégé plugin can warn
"N axioms of kinds {…} were not understood; results are a sound under-approximation,"
instead of either silently missing entailments or refusing the ontology outright.

## 2. Soundness

Dropping any axiom **weakens** the KB, so it is a **sound under-approximation**:
subsumption/unsatisfiability queries can only MISS entailments (never assert a
false one), and consistency can only fail to detect an inconsistency (never
falsely report one). The diagnostic makes this honest — the user is told results
are an under-approximation. No FP risk. For an ontology with **no** unsupported
constructs the `dropped` map is empty and behavior is **byte-identical** to today.

## 3. Conversion error-contract refactor (core change)

- **`ce_or_skip!` stops swallowing.** It propagates `Err(UnsupportedDataRange)`
  (preserving the precise reason) instead of returning `Ok(None)`.
- **`convert_ontology`'s loop catches, never `?`-aborts on unsupported:**
  ```text
  Ok(Some(ax))          → out.axioms.push(ax)
  Ok(None)              → benign drop (metadata / annotations / declarations) — ignore
  Err(reason)           → out.dropped.record(component_kind(&ac.component), reason); continue
  ```
  After the refactor `Ok(None)` is **unambiguously benign** (no benign-set
  classification needed), and every `Err` is a recorded incompleteness drop with its
  precise reason.
- `convert_ontology`'s return type stays `Result<InternalOntology, ConversionError>`
  for API stability, but it **no longer returns `Err` for unsupported constructs**
  (they become recorded drops). It may still `Err` only on a genuinely fatal
  internal failure (none currently expected on a successfully-parsed ontology).

## 4. The diagnostic type

```rust
// owl-dl-core
#[derive(Debug, Clone, Default)]
pub struct DroppedAxioms {
    /// stable "kind" label → count, sorted for determinism.
    by_kind: BTreeMap<String, u64>,
}
impl DroppedAxioms {
    pub fn is_empty(&self) -> bool;
    pub fn total(&self) -> u64;
    pub fn by_kind(&self) -> &BTreeMap<String, u64>;
    fn record(&mut self, kind: String);
}
```
`kind` = the horned-owl component discriminant + reason, e.g.
`"SubClassOf: unsupported data range"`, `"Rule: unsupported axiom kind (SWRL)"`,
`"ClassAssertion: anonymous individual"`, `"SubObjectPropertyOf: unsupported role expression"`.
A small `component_kind(&Component) -> &'static str` helper names the discriminant;
the reason comes from the `ConversionError` variant. Stored as a new
`pub dropped: DroppedAxioms` field on `InternalOntology`.

## 5. Surface (three layers)

### 5.1 Reasoner API
`InternalOntology.dropped` flows outward: add `dropped: DroppedAxioms` to
`ClassificationStats` (and expose on `Realization` / the query results as needed),
or a standalone `pub fn dropped_axioms<A: ForIRI>(onto: &SetOntology<A>) -> DroppedAxioms`
that runs conversion and returns the map. (Plan picks the minimal threading:
`ClassificationStats.dropped` is the natural home since classify already returns stats.)

### 5.2 CLI
- **Default stderr warning** (only when non-empty), after the result, matching the
  existing `warn_if_incomplete` style:
  `warning: N axiom(s) not understood and dropped (kinds: SubClassOf: unsupported data range ×2, Rule: unsupported axiom kind ×1); results are a sound under-approximation`.
- **`--json`**: a top-level `"dropped": { "<kind>": <count>, … }` block (empty object
  when none) in `classify`/`consistent`/`realize` and the new query subcommands.
  `schema_version` stays `1` (additive optional field).

### 5.3 Python
- A `dropped_axioms(path) -> dict[str, int]` binding, exported + `.pyi`-stubbed +
  `test_stubs`-guarded. (Optionally, the reasoning wrappers emit a warning when
  drops occur — a new `DroppedAxiomsWarning` mirroring `IncompleteQueryWarning`;
  plan decides whether to add the warning or just the accessor.)

## 6. Testing

- **Graceful-degradation unit tests** (RED today — these abort/silently-drop):
  - an ontology with an unknown/SWRL axiom → reasons over the rest, `dropped`
    records `"…: unsupported axiom kind"`, classify still yields the supported
    hierarchy (no abort);
  - an anonymous-individual axiom → recorded, no abort;
  - an unsupported-data-range `SubClassOf` → recorded (was silently dropped);
  - a **benign** metadata/annotation-heavy ontology → `dropped` is **empty**
    (benign drops NOT counted).
- **Golden `--json`** `dropped` block; **stderr warning** presence/absence.
- **Python** accessor + stub-drift.
- **FP=0 / no-regression:** full workspace suite green; for the curated fixtures
  that fully convert (no unsupported constructs) the `dropped` map is empty and
  classify closures are **byte-identical** to baseline (closure-diff spot-check).

## 7. Migration surface (enumerate in the plan)

`convert_ontology` no longer returns `Err` on unsupported constructs — this changes
a load-bearing contract. The plan must enumerate and update:
- any **test** asserting a `ConversionError`/`ReasonError::Conversion` on an
  anon-individual / unsupported-axiom / data-range ontology (now expects graceful
  degradation + a recorded drop);
- any **caller** that relied on the abort to reject an ontology (none expected to
  *want* the abort, but must be checked — grep `convert_ontology`, `ConversionError`,
  `ReasonError::Conversion`);
- `ce_or_skip!` call sites (the macro's contract changes from "drop the axiom" to
  "propagate the reason" — behavior at `convert_ontology` is equivalent (still
  dropped) but now recorded).

## 8. Non-goals

- No `--strict` / abort-on-unsupported mode (graceful degradation is the sole
  behavior, per the design decision).
- No new *reasoning* capability — this is pure visibility + graceful degradation
  over the existing supported fragment.
- Not changing which constructs are supported (that's #40/#42); only how the
  unsupported ones are handled (drop+record vs silent-drop/abort).

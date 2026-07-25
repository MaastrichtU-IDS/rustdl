# Design: complex (anonymous) class-expression queries (issue #48)

**Date:** 2026-07-25
**Status:** design approved (brainstorming); pending spec review → implementation plan.
**Author:** Claude + Michel, session: rustdl API-surface brainstorming (continuation of #44–#47).
**Closes:** #48 — the last "API surface" item of the Protégé-plugin gap audit
(`docs/superpowers/specs/2026-07-24-protege-plugin-design.md` §C/§8a). One combined
spec + plan; single PR.

## 1. Goal

Accept an **arbitrary (anonymous) class expression** as a query target and answer
three OWLReasoner-style queries over it:
- `isSatisfiable(CE)`
- `isEntailed(SubClassOf(CE₁, CE₂))`
- `getInstances(CE)`

Today these queries exist for **named classes only** (`is_class_satisfiable`,
`is_subclass_of`, `instances_of`, all taking an IRI string). The gap is a stable
API accepting a *parsed class expression* as the target. Closing it lets the
Protégé plugin answer complex-expression node-set queries (instead of returning
empty) and complex boolean queries (instead of `UnsupportedOperationException`).

## 2. Reduction — probe-class injection (sound by construction, max reuse)

For a parsed class expression `CE`, mint a fresh probe class IRI `Q` and add the
axiom `EquivalentClasses(Q, CE)` to a clone of the ontology. Because `Q ≡ CE`, the
probe's satisfiability, subsumption, and instances are **identical** to `CE`'s.
Then reduce to the existing named-class queries:

| Query | Reduction |
|-------|-----------|
| `isSatisfiable(CE)` | add `EquivalentClasses(Q, CE)` → `is_class_satisfiable(onto', Q)` |
| `isEntailed(SubClassOf(CE₁,CE₂))` | add `EquivalentClasses(Q1,CE₁)`, `EquivalentClasses(Q2,CE₂)` → `is_subclass_of(onto', Q1, Q2)` |
| `getInstances(CE)` | add `EquivalentClasses(Q, CE)` → `instances_of(onto', Q)`, with `Q` filtered from output |

**Sound by construction:** the equivalence axiom is a conservative definitional
extension over a fresh name; it adds no entailments about the original signature,
so the reduction is exact. Soundness/completeness of the *answer* is exactly that
of the underlying named query (e.g. `is_class_satisfiable` is sound; a trust-`Sat`
verdict may be a MISS) — surfaced via the same `incomplete` posture used elsewhere.

**Fresh-probe guarantee:** `Q` is a distinctive URN (e.g.
`urn:rustdl-ce-probe:0` / `:1`) that must NOT already occur in the ontology's
class signature; a collision is an error, not a silent overwrite. (This mirrors
the existing `PROBE_IRI` pattern in `justify.rs`.)

## 3. Parsing (front-end concern)

The class expression is written in **Manchester syntax** — the only standalone
class-expression parser rustdl has, the syntax users write in Protégé, and
symmetric with rustdl's existing Manchester I/O. The ontology *file* stays any
supported format (OFN / OWX / RDF-XML / OMN).

Parsing uses the horned-owl fork's existing
`horned_owl::io::omn::reader::parse_class_expression(s, &pm, &build)` →
`ClassExpression<A>` (no fork change needed). The `PrefixMapping` comes from
**reading the ontology** (the CLI already has `parse_ofn_with_pm`, and the OFN/OWX
readers return a full prefix map), so abbreviated IRIs in the expression
(`:Cheese`, `pizza:Margherita`) resolve against the file's vocabulary; full IRIs
in `<…>` also work. A malformed expression yields a clean error, never a panic.

Parsing lives at the **front end** (CLI / Python). The reasoner API accepts an
already-parsed `ClassExpression<A>`, keeping the parser dependency out of the core
query fns and the API syntax-agnostic.

## 4. Surface (three layers, matching #44–#47)

### 4.1 Reasoner API (`crates/owl-dl-reasoner/src/class_expr_query.rs`, new)
- `class_expression_satisfiable<A>(onto: &SetOntology<A>, ce: &ClassExpression<A>) -> Result<CeVerdict, ReasonError>`
- `class_expression_entailed_subclass<A>(onto, sub_ce, sup_ce) -> Result<CeVerdict, ReasonError>`
- `class_expression_instances<A>(onto, ce) -> Result<CeInstances, ReasonError>`

where `CeVerdict { holds: bool, incomplete: bool }` and
`CeInstances { individuals: Vec<String>, incomplete: bool }`. Each builds the
probe ontology, runs the fresh-probe check, injects `EquivalentClasses`, and
delegates to `is_class_satisfiable` / `is_subclass_of` / `instances_of`. `incomplete`
reflects the underlying query's completeness signal (trust-`Sat`/timeout), matching
the honesty policy established for the other queries. The synthetic probe IRI(s)
are filtered from `class_expression_instances` output (they are individuals-free
anyway, but any probe *class* is excluded defensively).

### 4.2 CLI (`crates/owl-dl-cli/src/main.rs` + `json_out.rs`)
New subcommands (existing IRI-only `sat`/`subclass`/`instance`/`instances` stay
byte-stable):
- `sat-expr <file> <ce> [--json]` → `{ "schema_version":1, "incomplete":bool, "satisfiable":bool }`
- `subclass-expr <file> <sub-ce> <sup-ce> [--json]` → `{ …, "entailed":bool }`
- `instances-expr <file> <ce> [--json]` → `{ …, "instances":[<iri>,…] }`

Each: `parse_ofn_with_pm(file)` → parse the Manchester CE(s) with the prefix map →
call the reasoner fn → print (`--json` = one object on stdout, diagnostics to
stderr; else human line(s)). Parse errors → nonzero exit with a clear stderr
message.

### 4.3 Python (`crates/owl-dl-py`)
- `class_expression_satisfiable(path, ce) -> bool`
- `class_expression_entailed_subclass(path, sub_ce, sup_ce) -> bool`
- `class_expression_instances(path, ce) -> list[str]`

Each reads the ontology (+ prefixes), parses the Manchester CE, queries, and emits
`IncompleteQueryWarning` when the result is a sound under-approximation (mirrors the
#44–#47 Python parity mechanism). Exported in `__init__.py` `__all__`, typed in
`__init__.pyi`, guarded by `test_stubs`.

## 5. Testing

- **Reasoner unit tests:** `A ⊓ ¬A` unsat, `A ⊔ B` sat; `A ⊓ B ⊑ A` entailed,
  `A ⊑ A ⊔ B` entailed, `A ⊑ B` *not* entailed (negative control); `∃r.C`-style
  and nominal (`{a}`) expressions; `instances-expr(A ⊔ B)` = the union of A's and
  B's instances. Fresh-probe-collision canary (probe IRI already present → error).
  Malformed-Manchester canary (parse error surfaced, no panic).
- **HermiT oracle** (FP=0 guard, feasible via the same probe trick offline): for a
  fixture, materialize `EquivalentClasses(Q, CE)` into an OWL file, run ROBOT
  `reason` (`SubClass` / `ClassAssertion` generators), and compare the probe's
  entailed subsumers / instances against rustdl's `class_expression_*` output.
  FP-direction assertion unconditional; MISSED gated on `incomplete()`.
- **Golden `--json`** per subcommand; **Python stub-drift** (`test_stubs`).
- FP=0 corpus untouched (the reduction only adds a fresh definitional axiom; it
  cannot perturb the original signature's classification).

## 6. Scope boundary

- **Object class expressions** are the focus: `ObjectIntersectionOf` /
  `ObjectUnionOf` / `ObjectComplementOf` / `ObjectSomeValuesFrom` /
  `ObjectAllValuesFrom` / `ObjectMin/Max/ExactCardinality` / `ObjectOneOf` /
  `ObjectHasValue` / `ObjectHasSelf`.
- **Data-range-bearing CEs** (`DataSomeValuesFrom`, `DataHasValue`, …) parse and
  reduce through the identical probe path, but inherit rustdl's **existing**
  datatype under-approximations (documented in CLAUDE.md); #48 adds no new datatype
  capability and no new gap — the CE surface simply exposes what the engine already
  does.
- Not in scope: object-property-expression or data-property-expression *targets*
  (that is the property-hierarchy/#44 surface); parsing CEs in OFN/OWX syntax
  (Manchester only for v1 — a second parser is a future enhancement if a consumer
  needs it).

## 7. Risks / open items for the plan

- **Probe IRI freshness across `A`/Build:** the CE is parsed with a `Build<RcStr>`
  and added to a `SetOntology<RcStr>`; confirm IRI interning compares by string
  (it does — horned-owl `A: ForIRI`) so a probe minted in a fresh Build is equal to
  one in the ontology's Build. Plan verifies.
- **Adding an axiom to a `SetOntology`:** confirm the mutation API
  (`MutableOntology::insert` / `Component` construction for `EquivalentClasses`)
  and that a cloned ontology is cheap enough per query. Plan pins the exact calls.
- **`incomplete` extraction:** `is_class_satisfiable` / `is_subclass_of` /
  `instances_of` currently return a bare `bool` / `Vec`; the plan determines how to
  surface the underlying completeness signal (a `_with_stats` variant, or the
  `Classification`/fragment posture) so `CeVerdict.incomplete` is honest rather than
  hardcoded.
- **Manchester prefix map for RDF/XML inputs:** `parse_ofn_with_pm` returns an empty
  `PrefixMapping` for RDF/XML; full-IRI expressions still work, but abbreviated
  names against an RDF/XML file won't resolve. Document; not a v1 blocker.

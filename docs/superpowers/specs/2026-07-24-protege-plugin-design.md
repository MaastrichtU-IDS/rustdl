# Design: rustdl Protégé reasoner plugin

**Date:** 2026-07-24
**Status:** design approved (brainstorming); pending spec review → implementation plan.
**Author:** Claude + Michel, session: Protégé-plugin brainstorming.

## 1. Goal

Make **rustdl a first-class reasoner in Protégé**: it appears in the reasoner
dropdown and, on "Reason", classifies / checks consistency / builds the class
hierarchy / computes class assertions like HermiT or ELK. The install experience
is **seamless** — the user installs one Protégé plugin and rustdl works with **no
separate binary install or PATH setup**.

Reference precedent: **`kobayashi-marust` (km)**
(`github.com/bio-ontology-research-group/kobayashi-marust`), a sibling Rust SROIQ
reasoner whose Protégé plugin is "a thin OWL API `OWLReasoner` that serialises the
loaded imports closure, invokes the pure-Rust `km` binary, and maps the named-class
subsumptions back into Protégé." We adopt km's **subprocess** architecture and go
somewhat richer (wire class assertions / instance queries too).

## 2. Non-goals (v1)

- **Explanation/justify/repair UI** — rustdl has these; surfacing them in Protégé
  is a compelling **v1.x**, explicitly deferred.
- **Incremental reasoning** — the reasoner is BUFFERING; an ontology edit marks it
  stale and the next "Reason" does a full re-classify.
- **Object/data-property hierarchies & assertions, same/different individuals,
  disjoint-classes inference, complex-class-expression queries** — not backed by
  rustdl; not advertised, queries return empty (see §6).
- **JNI / native in-process / OWLlink** bridges — rejected in favor of subprocess
  (portability, matches km, no FFI/native-linking complexity beyond shipping a
  binary).

## 3. Architecture — three deliverables

1. **rustdl CLI `--json` output mode** (rustdl repo) — the machine-readable bridge
   contract. Prerequisite: today's CLI prints human tab/`#`-comment lines, not
   robustly parseable.
2. **Cross-platform standalone `rustdl` CLI binaries in CI** (rustdl repo) — today
   `release-python.yml` builds *wheels*, not standalone binaries. Add a CLI-binary
   build matrix (same targets as the wheel job) whose artifacts the plugin embeds.
3. **`protege/` Maven module** (a pure-Java OSGi bundle) — the plugin: bundles the
   binaries, extracts+invokes per platform, implements the OWLAPI `OWLReasoner`
   surface backed by rustdl subprocess calls.

Data flow (one "Reason" cycle): Protégé calls
`precomputeInferences(InferenceType…)` → plugin serialises the **imports closure**
to OWL functional syntax (a temp file) → spawns `rustdl <subcmd> --json` for each
requested inference type → parses JSON → caches → all `OWLReasoner` query methods
answer from the cache.

## 4. The JSON contract (rustdl `--json`)

`--format json` (alias `--json`) on the subcommands the plugin needs; one JSON
object on **stdout**, all diagnostics to **stderr**. This schema is the versioned
stability contract, golden-tested on the rustdl side. A top-level
`"schema_version"` field guards drift.

- `classify --json` →
  ```json
  { "schema_version": 1, "consistent": true, "incomplete": false,
    "unsatisfiable": ["<iri>", …],
    "equivalent_groups": [["<iri>", "<iri>", …], …],
    "direct_subsumptions": [["<sub-iri>", "<sup-iri>"], …] }
  ```
  `direct_subsumptions` are the Hasse edges (rustdl's `direct` lines);
  `equivalent_groups` the `equiv` sets; `unsatisfiable` the `unsat` set;
  `incomplete` reflects the `INCOMPLETE` (timed-out class pairs) warning.
- `consistent --json` → `{ "schema_version": 1, "consistent": bool }`.
- `realize --json` →
  ```json
  { "schema_version": 1,
    "individuals": [ { "iri": "<iri>", "types": ["<iri>", …],
                       "direct_types": ["<iri>", …] } ] }
  ```
v1 requires **only** `classify`, `consistent`, and `realize` with `--json` — the
cache from those answers every v1 query (named-class satisfiability = membership
in `unsatisfiable`; named subsumption entailment = the DAG). `sat` / `instance` /
`justify` / `repair` / `diagnose` `--json` are **deferred to v1.x** (they arrive
with the explanation work, §2), not part of this spec.

IRIs are the lingua franca: plugin serialises OWLAPI entities → OFN → rustdl;
rustdl emits IRIs; plugin maps them back via `OWLDataFactory`.

## 5. The Java plugin (`protege/` module)

**Components (each a focused, testable unit):**
- **`RustdlBinary`** — platform detection (`os.name`/`os.arch`), one-time extraction
  of the embedded `native/<os>-<arch>/rustdl[.exe]` to a per-user cache dir,
  `chmod +x`, `--version` verification. Honors a `RUSTDL_BIN` env / Protégé
  preference override (dev / unsupported-platform escape hatch); otherwise the
  bundled binary. Pure resolution, no reasoning.
- **`RustdlProcess`** — given an ontology file + a subcommand, spawns the binary,
  enforces the reasoner timeout, captures stdout, parses the JSON (§4). Pure I/O;
  unit-testable against JSON fixtures.
- **`RustdlReasoner`** — extends OWLAPI `OWLReasonerBase` (**BUFFERING**). Holds
  the per-`InferenceType` cache and orchestrates precompute/query.
- **`RustdlReasonerFactory`** (OWLAPI `OWLReasonerFactory`) +
  **`RustdlReasonerInfo`** (Protégé `ProtegeOWLReasonerInfo`) — registration; the
  Info class is what places "rustdl" in the dropdown.

**Flag-driven lifecycle (the InferenceType machinery):**
- `getPrecomputableInferenceTypes()` = **`{CLASS_HIERARCHY, CLASS_ASSERTIONS}`** —
  exactly what rustdl backs. Protégé (per user reasoner-preferences) will only
  request these.
- `precomputeInferences(types…)` maps each requested type to a subprocess:
  `CLASS_HIERARCHY` → `rustdl classify --json`; `CLASS_ASSERTIONS` →
  `rustdl realize --json` (only when individuals exist). Results cached;
  `isPrecomputed(type)` tracks each. A supported-but-not-yet-precomputed query
  triggers its subprocess on demand and caches.
- Serialisation happens once per precompute cycle (shared temp file across the
  classify + realize calls of that cycle).

**Query-method mapping (all cache-backed, no re-invocation):**
- `isConsistent`, `getUnsatisfiableClasses`, `getBottomClassNode`/`getTopClassNode`.
- `getSubClasses`/`getSuperClasses`/`getEquivalentClasses` (direct + indirect) from
  the cached subsumption DAG + equivalence groups.
- `isSatisfiable(namedClass)`, `isEntailed(SubClassOf(namedClass, namedClass))`.
- `getInstances`/`getTypes` from cached realize.

**Staleness:** BUFFERING ⇒ ontology edits flag the reasoner dirty; `flush()` /
next "Reason" re-serialises + re-invokes. No incremental reasoning (§2).

## 6. Error handling (mapped to the OWLReasoner contract so Protégé stays sane)

- Binary missing / non-zero exit / crash → `ReasonerInternalException` with the
  captured stderr; never hang.
- Reasoner timeout (`getReasonerConfiguration().getTimeOut()`) → kill the
  subprocess → `TimeOutException`.
- rustdl `inconsistent` → `isConsistent()=false`; consistency-requiring queries
  throw `InconsistentOntologyException` (Protégé shows its standard inconsistent
  state).
- JSON parse / `schema_version` mismatch → fail loudly naming the version.
- `incomplete` (timed-out class pairs) → log a warning; the hierarchy is a sound
  under-approximation (no false subsumptions, may miss some).
- Unsupported *node-set* queries (unadvertised inference types) → **empty node
  sets** (UI-safe sound under-approximation), not exceptions. The one exception is
  **boolean** queries that can't be "empty": `isSatisfiable` / `isEntailed` on a
  **complex** (non-named) class expression rustdl can't decide → throw
  `UnsupportedOperationException` (returning a bool would be an unsound guess).
  Named-class `isSatisfiable`/`isEntailed` are always answered from the cache.

## 7. Testing

- **rustdl side:** golden JSON tests for `classify/consistent/realize --json` on
  small fixtures (e.g. `sulo`, a tiny consistent ABox, an inconsistent fixture).
  Schema-version assertion.
- **Java side:** unit-test `RustdlProcess` JSON parsing (fixtures);
  unit-test the DAG/query mapping (canned classify result → `getSubClasses`,
  `getEquivalentClasses`, etc.); unit-test `RustdlBinary` platform routing.
- **Integration smoke test:** run the real bundled binary on a tiny ontology
  through `RustdlReasonerFactory` end-to-end (skipped in CI if the binary for the
  runner's platform isn't built).

## 8. Scope summary (v1)

**Wired & rustdl-backed:** consistency, class hierarchy (classify),
unsatisfiable classes, class assertions (types/instances via realize).
**Empty/unadvertised:** object/data-property hierarchies & assertions,
same/different individuals, disjoint-classes, complex-class-expression queries.
**Seamless install:** single fat jar bundling `{linux-x86_64, linux-aarch64,
macos-aarch64, windows-x86_64}` binaries; runtime extract + invoke; env/pref
override. Mac-Intel (sunset) uses the override/build path.

## 8a. Tracked upstream enhancements (the gaps the v1 boundary implies)

The v1 "empty/unadvertised" and "dropped construct" boundaries correspond to
filed rustdl enhancement issues — the plugin degrades gracefully today, and each
gap closing later automatically enriches the plugin:

- **Expressivity (constructs dropped):** #40 `DisjointUnion`, #41 nominal +
  cardinality realization, #42 datatype composites / non-string `DataOneOf` /
  data-cardinality counting, #43 surface silently-dropped axioms.
- **API surface (methods that return empty in v1):** #44 property hierarchy,
  #45 property values (`getObject/DataPropertyValues`), #46 same/different
  individuals, #47 disjointness, #48 complex-class-expression queries.

The §A reasoning-completeness posture (`trust_sat` misses, per-pair-budget /
`incomplete`, timeout-derived consistency) is not a discrete feature but the
documented soundness-vs-completeness stance (CLAUDE.md + known-limitation docs);
the plugin surfaces it via the `incomplete` flag rather than a code change.

## 9. Risks / open items for the plan

- **rustdl `classify` completeness on the user's ontology.** rustdl is
  near-complete-but-not-guaranteed on the broad (non-corpus) tiers, and hard
  SROIQ inputs need a per-pair budget (`wine`: DNF unbounded). The plugin should
  set a sensible default `--pair-timeout-ms`/global budget and surface `incomplete`
  so users aren't misled — a design detail for the plan.
- **Jar size** (~40 MB with four binaries) — acceptable for seamlessness; revisit
  per-OS thin jars if it becomes a distribution problem.
- **Protégé/OWLAPI version targeting** — match km's (Protégé 5.x / OWLAPI 4 or 5);
  confirm exact versions when reading km's `protege/pom.xml` during planning.
- **Serialisation fidelity** — OWLAPI functional-syntax renderer → horned-owl OFN
  reader round-trip must preserve all entities/axioms rustdl supports; anything
  rustdl drops at parse is silently invisible. Plan should include a round-trip
  sanity check on a feature-rich fixture.

# Manchester io/omn writer validation vs OWL-API + omny (2026-06-13)

Validated the horned-owl fork's Manchester **writer** output against two external
parsers, by rendering corpus `.ofn` ontologies to `.omn` (via the fork's own
`io::ofn::reader` → `io::omn::write`, throwaway harness `/tmp/omn-render`) and
re-parsing each:

- **OWL-API** (authoritative): `docker run obolibrary/robot:v1.9.6 robot convert
  --input X.omn --output X.owx` (ROBOT loads via OWL-API's Manchester parser).
- **omny** (the user's pure-Python Manchester parser): `/tmp/verify-022/bin/python`,
  `omny.parse(text)` (v0.2.2).
- (No host JRE — glibc 2.35 vs the on-disk JDKs' 2.38; OWL-API reached via the
  ROBOT docker image, which is how `pymos/bench` already drives it.)

## Results

OWL-API acceptance went **3/10 → 9/10** after four writer fixes (all committed in
the fork). The sole remaining failure (`ro`) cannot pass regardless — it carries
a 367-line non-Manchester `# General axioms` block.

| ontology | omny | OWL-API before | OWL-API after fixes |
|---|---|---|---|
| bibtex | OK | OK | **OK** |
| family | OK¹ | OK | **OK** |
| sio-fp-module | OK² | OK | **OK** |
| anch-module | OK | FAIL | **OK** (fix #1) |
| asp-module | OK | FAIL | **OK** (fix #1) |
| sio-450-module | OK | FAIL | **OK** (fix #1) |
| np-module | OK | FAIL | **OK** (fix #2) |
| pizza | OK² | FAIL | **OK** (fix #1+#2) |
| go-basic | OK² | FAIL | **OK** (fix #4) |
| ro | FAIL | FAIL | FAIL (misc block + `owl:topObjectProperty`) |

¹ family: omny mis-parses 434 typed-literal `Facts` (`"1868"^^xsd:integer`).
² omny silently drops `SubPropertyChain` axioms (unsupported keyword).

## Fixes applied (committed in the fork)

1. **emit entity `Annotations:` clauses first in each frame** — OWL-API desyncs
   when a logical clause ending in an `ObjectOneOf {…}` is immediately followed
   by an `Annotations:` clause; emitting entity annotations first (matching
   OWL-API's own layout) avoids the adjacency. Fixed anch / asp / sio-450.
2. **OWL-API-compatible parenthesization** — `ce_operand` now brackets every
   operand that is not a bare atomic class (`and (not (r some C))`, `and
   ({oneof})`, `r some (D and E)`). Fixed np + pizza.
3. **digit-leading CURIE locals → full IRI** — `EC:2.5.1.30` is emitted as a
   full `<IRI>` (OWL-API's lexer reads a digit-leading local as a number).
4. **annotation IRI values → full IRI** — annotation *values* are never
   abbreviated, because OWL-API expects a literal in the value position when the
   property is punned (OBO RO relations / `skos:exactMatch`). Fixed go-basic.

The 17-case class-expression round-trip and all 37 omn unit/integration tests
stay green (parens and full IRIs are structurally transparent on read); one
writer-output unit assertion was updated to the new parenthesization convention.
Each fix matches OWL-API's *own* Manchester renderer (verified by round-tripping
`robot convert X.ofn → X.omn → X.owx`).

## Remaining (`ro`)

`ro` cannot fully round-trip through OWL-API. Its 367-line `# General axioms`
misc block is the writer's functional-syntax fallback (skip-and-warn region);
breaking it down corrects an earlier overstatement that it "has no Manchester
form" — only ~8% of it genuinely lacks one:

| content | lines | genuinely no Manchester form? |
|---|---|---|
| `AnnotationAssertion` on undeclared/imported IRIs (mostly IAO) | ~310 | **No** — writer limitation |
| `DLSafeRule` (SWRL) | 25 | **Yes** (Manchester has no rule syntax) |
| general class axioms, complex LHS (`(R some C) SubClassOf …`) | 3 | **Yes** (frames need a named subject) |

- The ~310 annotations are **orphan entity annotations**: their subject is an
  IRI ro references but never declares (e.g. `obo:IAO_0000027`), so it heads no
  frame and the writer's annotation pass drops it to misc. These *do* have a
  Manchester form — OWL-API auto-declares referenced entities and emits
  `Class: … / Annotations: …`. A writer fix (declare-and-frame referenced
  annotation subjects) would shrink ro's misc block to ~28 lines and round-trip
  these.
- The 25 SWRL rules + 3 complex-subject class axioms genuinely have no Manchester
  frame form.
- Separately, `SubPropertyOf: owl:topObjectProperty` — OWL-API rejects the
  universal top object property as a named super-property (an OWL-API quirk).

The punning-induced annotation-VALUE failures (which previously affected both
go-basic and ro) are resolved by fix #4.

## Three-way partition of failures

**(c) omny limitations — NOT writer bugs (OWL-API accepts these):**
- `SubPropertyChain:` — omny warns "Unknown axiom keyword … silently dropped".
  OWL-API accepts it (family round-trips it). Our writer is correct.
- typed literals `"1868"^^xsd:integer` — omny mis-tokenizes the `^^` form.
  OWL-API accepts it. Our writer is correct.
- empty-default-prefix `Prefix: : <iri>` — omny accepts in isolation; ro's omny
  failure is elsewhere (line 1, unresolved — ro also has the writer issues below).

**(b) by-design non-Manchester (`# General axioms` misc block):** only `ro`
(367 lines). Neither omny nor OWL-API tolerates the functional-syntax tail; this
is the documented skip-and-warn region (our reader skips it, external parsers
can't).

**(a) WRITER bug — OWL-API rejects, omny is lenient (THE actionable finding):**
Our writer emits each frame's `Annotations:` clauses **last** (after SubClassOf/
EquivalentTo/…). OWL-API emits them **first**. When a logical clause's value ends
in an `ObjectOneOf {…}` and is immediately followed by an `Annotations:` clause,
OWL-API's Manchester parser desyncs (it does not cleanly end the class expression
before `Annotations:`). Reordering each frame so `Annotations:` precedes the
logical clauses — matching OWL-API's own output — **fixes anch / asp / sio-450**
and moves pizza's failure from line 39 to line 831 (a different, deeper construct).

Minimal reproduction (OWL-API FAIL):
```
Class: :A
    EquivalentTo: :B and {:i1, :i2}
    Annotations: rdfs:label "x"
```
Annotations-first (OWL-API OK):
```
Class: :A
    Annotations: rdfs:label "x"
    EquivalentTo: :B and {:i1, :i2}
```
(`:i1`,`:i2` declared. Parenthesizing the `ObjectOneOf` does NOT help; ordering
does. Every minimal repro of the *isolated* construct passes — the failure is
emergent in the full document, which is why it resisted reduction.)

**Residual writer issues (np-module, go-basic, pizza after the reorder):** at
least one more OWL-API-rejected construct remains, also emergent/whole-document
(each minimal repro passes). pizza's post-reorder error is at a `VegetarianPizza`
`EquivalentTo: P and not r some C and not r some D` expression. OWL-API renders
class expressions **fully parenthesized** (`and (not (r some C))`); our writer
emits flatter forms. Matching OWL-API's parenthesization is the likely next fix.

## Recommended fix sequence (writer, in the fork)

1. **Emit `Annotations:` clauses first in every frame** (before logical clauses).
   Confirmed to fix 3/5 failing ontologies; matches OWL-API's canonical layout.
2. **Match OWL-API's class-expression parenthesization** (`and (X)`, `not (X)`,
   `({oneof})`) — addresses the residual pizza/np/go-basic failures. Best driven
   by forward-transforming from OWL-API's own `robot convert X.owl → X.omn`
   output as the byte-diff target, rather than guess-and-test.
3. Re-run this harness for a clean before/after.

## Caveat

ROBOT exit-0 means "loaded + wrote", not value-identity. The PASS rows confirm
*parse conformance*; a stronger semantic-equality check (compare the reloaded
ontology's axioms to the original) was not done — parse-conformance is the
honest, achievable signal here.

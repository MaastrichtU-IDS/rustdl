# rustdl bench fixtures

Small OWL Functional-Syntax (`.ofn`) ontologies used as differential-testing
inputs for the rustdl tableau against an external reasoner oracle (HermiT
via ROBOT).

Each fixture declares a named class `:Test` (IRI
`http://rustdl.test/Test`) whose satisfiability under the fixture's TBox is
the verdict checked. The expected verdict for each fixture is pinned in
[`manifest.toml`](manifest.toml).

Pure ALC scope: no inverse roles, no cardinality restrictions, no nominal
class expressions, no datatypes. These exercise the rule set landed in
Phase 2 commits 1-6.

## Workflow

1. **Oracle (HermiT via ROBOT)**: run `../../docker/robot/oracle.sh
   <fixture.ofn> http://rustdl.test/Test` to get HermiT's verdict for the
   `:Test` class. See [`docker/robot/README.md`](../../../docker/robot/README.md)
   for setup.

2. **rustdl**: TBD — needs a public facade in `owl-dl-reasoner` that
   loads a fixture via `horned-owl`, runs absorption, and queries
   `TableauContext::is_satisfiable` for `:Test`. When that lands, the
   bench binary in this crate will cross-check both verdicts against
   `expected`.

## Adding fixtures

Keep fixtures small (a few dozen lines) and focused on one or two rules.
Use the `:` prefix for the test ontology IRI:

```ofn
Prefix(:=<http://rustdl.test/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)

Ontology(<http://rustdl.test/fixture/NN_short_name>
    Declaration(Class(:A))
    Declaration(Class(:Test))
    EquivalentClasses(:Test ... )
)
```

Then append an entry to `manifest.toml` with `expected = "sat"` or
`"unsat"`.

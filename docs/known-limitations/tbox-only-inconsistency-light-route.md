# A shortcut answer on an inconsistent `TBox`-only KB carries no inconsistency signal (#457)

**Status: a documented gap, by decision (Michel, 2026-11-06, #457 residual 2).** The answers are
correct; only the signal is missing. It is not a false positive.

## What happens

On a KB with no individual (no class or property assertion, and no nominal), the query
surfaces `sat`, `subclass`, `explain`, `prove`, `sat-expr`, `subclass-expr` and the library and
Python equivalents check consistency only after a positive answer, in one of two ways. Which
one depends on how the answer was found:

- **The prepared route** (the main tableau answered, so the query already built a prepared
  ontology). Since #457, the bounded main-tableau consistency check runs here
  (`RUSTDL_QUERY_NO_SEED_TAIL`, default on), up to `RUSTDL_CONSISTENCY_FALLBACK_MS`, once per
  prepared ontology. A proof of inconsistency makes the query error `Inconsistent` (exit 1). A
  check that is cut sets `consistency_undetermined` (and prints the stderr warning).
- **The light route** (the answer is told, comes from the EL saturation closure, or is proved
  by the H4 query wedge under `RUSTDL_QUERY_WEDGE_MS`). No check runs: the consistency wedge
  has no individual to seed it, and the main-tableau check would first have to prepare the
  KB. The answer is printed with no error and no warning, even when the KB is inconsistent.

On an inconsistent KB every subsumption holds and every class is unsatisfiable, so a light-route
`yes` (or `unsatisfiable`) is still correct. What is lost is the signal that the KB itself is
broken, which HermiT (it throws `InconsistentOntologyException` on every query) and Konclude
(it reports the inconsistency with the answer) both give.

## Example

`crates/owl-dl-cli/tests/fixtures/query_wedge_hang_457.ofn` (8 axioms, shrunk from generated
case 438; inconsistent per Konclude and HermiT):

| query | route | answer |
|---|---|---|
| `subclass C0 C1` | prepared (the tableau answers) | error: ontology is inconsistent (exit 1) |
| `subclass C0 C6` | light (`SubClassOf(:C0 :C6)` is told) | `yes`, exit 0, no warning |

## How often

The gap is not an edge case. On the 39 generated `TBox`-only cases that are inconsistent per the
oracle (the per-query laws, R7.5), the prepared-route check turned 204 of the 1,622 silent answers
into the `Inconsistent` error: 152 `sat`, 49 `subclass` and 3 `explain`, on 38 of the 39 cases.
The other 1,418 silent answers come from the light route, and every one of the 39 cases still has
at least one. On an inconsistent KB the EL closure often finds the queried classes unsatisfiable,
which answers the query before the tableau runs.

## Why the light route does not check

Option A of #457 ran the same check on the light route too, preparing the KB when the query had
not. It was measured on the ORE pool's 244 `TBox`-only non-EL ontologies, with 727 positive
`subclass` calls:
- it found no inconsistency;
- it added 81% CPU;
- the check gave up at its 10 s budget on 30 of 183 ontologies, which turned clean answers into
  `consistency_undetermined` ones at tens of seconds each, because preparing the KB has no
  deadline.

On the prepared route the KB is already prepared, so only the bounded check is paid. The
measurements of options A and A″ are in the pull request that shipped A″ (for #457).

## How to get the signal

- `rustdl consistent file.ofn` decides the KB's consistency directly (or `classify`, whose
  `consistent` field does).
- In the library, call the consistency check before trusting a positive answer on a KB with no
  individuals.

## How a fix would look

Run the check on the light route only when it is cheap: for example, with a deadline on the
prepare, or only for a KB small enough that preparing it is bounded. It must not reintroduce
option A's flag rate. The tests that pin this gap are named for it (`…_light_route_gap…`), and
should be flipped, not deleted.

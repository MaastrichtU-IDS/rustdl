# Imports are not resolved

**Status:** open by design (#579). Signalled since #579.

rustdl reasons over the one file it is given. Neither the CLI, the Python bindings
nor the reasoner loads an ontology named by an `Import(<…>)` declaration (RDF/XML
`owl:imports`), so the imported axioms are missing from what rustdl reasons with.

## What is signalled

Each `Import` is counted in `dropped` as `Import: unsupported axiom (unresolved
import)`. So:

- every completeness claim that reads `dropped` is false: `classify --json`'s
  `completeness_guaranteed`, the expression queries', `property-values`' and
  `individuals`' `incomplete`, and the structural surfaces' `*-dropped-axioms` gap;
- the reasoning commands print the `not understood and dropped` warning, except
  `justify`, `prove`, `repair`, `diagnose` and `report`, which never call
  `warn_if_dropped` (a gap of their own, for any drop kind; follow-up issue);
- `verify-el` reports `Unresolved` (exit 3) instead of `Verified`;
- Python's `class_expression_*` functions emit `DroppedAxiomsWarning`.

Answers do not change: they were always computed without the imported axioms.
Before #579 the `Import` was skipped as metadata, without a trace.

## What to do

Merge the import closure into one file first, e.g. `robot merge --input root.owl
--output merged.ofn`, and make sure the merged file carries no `Import`
declarations, or the drop is still counted. The Protégé plugin already does this:
it writes the imports closure into a fresh ontology (`FlattenedOntology.java`),
which has none.

On the ORE pool no file has an `Import` (0 of 1,921, 2026-11-26). Tracked inputs
that do: `bench-corpus/pizza.ofn` (2), `ontologies/real/pizza-ontostart-1.0.1.ofn`,
`wine.rdf`, `food.rdf`.

# rustdl `--json` output schema (v1)

Consumed by the Protégé plugin. Every object carries `"schema_version": 1`.
All arrays are sorted (byte order); pairs are `[sub, sup]`.

## `classify --json`

```json
{ "schema_version": 1, "consistent": bool, "incomplete": bool,
  "unsatisfiable": [iri], "equivalent_groups": [[iri, ...]],
  "direct_subsumptions": [[sub_iri, sup_iri], ...] }
```

`incomplete` = some class pair hit the time budget (defaulted to not-subsumed);
the hierarchy is sound (no false subsumptions) but may miss real ones.

## `consistent --json`

```json
{ "schema_version": 1, "consistent": bool }
```

## `realize --json`

```json
{ "schema_version": 1,
  "individuals": [ { "iri": iri, "types": [iri], "direct_types": [iri] } ] }
```

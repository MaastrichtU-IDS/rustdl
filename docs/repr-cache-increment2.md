# Representative-completion cache — increment 2 findings

Konclude-style status caching, ported in two increments. This note records what
increment 2 (the cache *consumer*) actually buys, measured, and why increment 3
is where the payoff is.

## What shipped

- **Increment 1** (`deterministic_signature`, committed `206e7c0`): the sound
  reuse *key* — an FNV hash of a node's **deterministic, non-nominal** atomic
  labels. "Deterministic" = empty backjumping dep-set ⇒ context-independent
  (Konclude's deterministic-vs-nondeterministic concept-set split; positive
  nominals excluded). Unit-tested.
- **Increment 2** (this change): a gated, within-`decide` **status cache**
  (`status_memo`) keyed by `completion_signature` — the *full canonical
  configuration* (labels + edges + `≤n`/`≥n` + `≠`, minus all backjumping
  bookkeeping). Consulted after `horn_fixpoint`, before branching; populated on
  every non-`Stalled` frame. Opt-in via `RUSTDL_REPR_CACHE` (or
  `with_repr_cache()`).

## Soundness (the load-bearing claim)

The Sat/Unsat verdict of the sub-search is a **pure function of the
configuration and the fixed clause set** — dep-sets only steer *how* we
backjump, never *which* clashes occur. So two frames at a byte-identical
configuration share a verdict. On a cached-`Unsat` hit the consumer sets
`clash_deps = DepSet::ALL` (conservative superset ⇒ backjumping stays sound, it
just can't skip a decision that step).

Validated:
- `repr_cache_preserves_verdict` — a battery of disjunction / cardinality /
  Horn shapes: verdict identical with and without the cache.
- End-to-end: `rustdl classify` output is **logically identical** with/without
  the cache on `anatomy`, `family`, and the disjunction/cardinality fixtures
  (the only diff observed was a `# wall breakdown ms` timing comment, which
  varies run-to-run even with the cache off).
- 128 `owl-dl-tableau` lib tests pass.

## Measured payoff: **0 hits on real classification**

| ontology | classify output | cache hits |
|---|---|---|
| anatomy.ofn | identical | 0 |
| family.ofn | identical (modulo timing comment) | 0 |
| 13_deep_disjunction_sat | identical | 0 |
| 16_forall_split_disjunction_sat | identical | 0 |
| 22_disjunction_under_exists_sat | identical | 0 |
| 27_eight_way_disjunction_sat | identical | 0 |
| 29_disjunction_..._clash_unsat | identical | 0 |
| 49_exact_cardinality_sat | identical | 0 |

The cache **populates** (entries are written every frame) but **never hits**.

### Why (the finding that motivates increment 3)

`completion_signature` keys the **whole graph**, including the root. On
disjunction search the chosen disjunct stays in the node's label set, so two
decision paths (`{A,B,…}` vs `{A,C,…}`) produce **different keys** even when
their sub-problems coincide. Transpositions never collapse to one key, so the
populated entries are never reused. (`≤n` merge transpositions are already
deduped upstream by canonical-partition enumeration, so they don't feed it
either.) Pinned by `repr_cache_populates_even_when_path_branching_prevents_reuse`.

## Conclusion / next

Increment 2 is **sound and harmless but inert** on the target workloads. Reuse
requires a **subtree-local** key, not a whole-completion one: a generated
successor's satisfiability is independent of ancestor branch choices, so its
`deterministic_signature` (increment 1) recurs across probes and branches. That
is the Konclude payoff and the next step — increment 3: consult
`deterministic_signature` as a cross-context node-status / blocking key. It is
also the soundness-sensitive piece (false `Unsat` = catastrophic), so it needs
the same parity gate plus the tree-shape / inverse-role conditions that rustdl's
existing anywhere-blocking already enforces.

For the paper-5 target specifically: **completeness is already solved** by the
ABox-assertion-nominal injection (37/37); this caching line is a *speed* lever
whose payoff is at scale, via increment 3.

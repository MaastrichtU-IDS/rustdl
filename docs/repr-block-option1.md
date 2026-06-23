# Option 1: cross-pair representative-blocker cache

The per-node status cache (Konclude `CBackendRepresentativeMemoryCache` principle)
that stops the search re-expanding the same node configurations across pairs.

## Design

`REPR_BLOCK` (gated by `RUSTDL_REPR_BLOCK`, default OFF) is a process-global set of
**pair-signatures** — a node's labels + its parent's labels + the incoming role,
i.e. the HF2 double-blocking key, with positive-nominal nodes excluded.

- **Populate** (`cache_sat_model`): only from a **confirmed `Sat` completion** —
  when a wedge `decide` returns `Sat`, the graph is a clash-free model, so every
  node's subtree is genuinely satisfiable; record their pair-signatures.
- **Consult** (`is_blocked`, double-blocking branch): after the in-completion
  blocker scan misses, a node whose pair-signature is cached is blocked — its
  required subtree was realised in a real model, so it need not be re-expanded.

Sound by construction: only nodes from a genuine model are cached (never a
failing/partial branch); reuse is the established pairwise-blocking unravelling
with a persistent witness pool.

## Validation

- 128 tableau lib tests pass with `RUSTDL_REPR_BLOCK=1`.
- Corpus classify output **byte-identical** baseline vs ON — sulo / sio / mie /
  paper5 / anatomy, including **SIO's 84 inverse-role pairs** (the case where
  unsound cross-context blocking would surface). The one pizza-run difference
  (`SpicySalami ⊑ Pizza` vs `⊑ SpicyPizza` as the *direct* parent) is sound
  timing jitter — both subsumptions hold; the direct-edge flips with which
  intermediate pair stalled.

## Result: does NOT crack pizza — and the reason is the key finding

Pizza is unchanged (705 stalled pairs, same wall). **The cache barely populates**,
for a fundamental reason:

> The cache can only record satisfiable subtrees from searches that **reach a
> model**. Pizza's hard pairs **stall before reaching `Sat`**, so they neither
> populate the cache nor (there being almost nothing cached) benefit from it.

This is the same wall increment-2 and increment-3 hit from different angles:
- increment-2 (per-completion memo): 0 hits — key included the branch path.
- increment-3 (incremental re-seed): per-step ~3× cheaper, but the exponential
  re-visiting remains.
- option-1 (representative cache): sound, but can't populate from stalling
  searches → nothing to reuse.

## What would actually crack pizza

The common blocker: pizza's hard subsumption searches **never complete within
budget**, so no sound, fully-verified satisfiable model is ever produced to cache.
Two real paths, both substantial:

1. **Mid-search partial-model caching.** Cache a satisfiable *subtree* the moment
   it is fully expanded clash-free (not waiting for the whole decide to reach
   `Sat`), with dependency tracking so a cached "sat" is invalidated correctly on
   backtracking. This is the genuine Konclude core and is high-risk (a wrong
   partial-model reuse = unsound classification).
2. **Consequence-based classification (Sequoia/ELK-style)** for the Horn/EL-ish
   majority, restricting tableau to the genuinely non-deterministic residue.
   Konclude combines this with its representative cache.

The representative-cache infrastructure here is the sound foundation for path 1;
it is committed gated-off as a building block, not a pizza fix.

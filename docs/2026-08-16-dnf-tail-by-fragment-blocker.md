# The DNF tail, decomposed by what blocks the saturation fast path

**Date:** 2026-08-16 · 143 DNFs at v0.4.19 (60 s cap, 1 thread). First decomposition of the
tail by **cause** rather than by which phase consumes the wall.

## The decomposition

| what blocks the fast path | n | share |
|---|---|---|
| **inverse + other constructs** | 67 | 47% |
| **inverse ONLY** | 35 | 24% |
| union / complement, no inverse | 30 | 21% |
| **already in EL — pure scale** | 9 | 6% |
| **∀ / cardinality only** | **2** | **1%** |

**Inverse roles block 71% of the tail** (102 of 143). Everything else is a minority.

## Consequence 1: the parked saturator branch is confirmed dead for this purpose

`feat/saturator-forall-propagation` carries two built, sound, FP=0 increments — general
`∀R.C` propagation and qualified `≤1 R.C` witness merge, byte-identical closures across 11
fixtures, 61 test groups green. It was parked as "corpus-invisible ALONE".

**That judgement is now confirmed on a much larger sample: it addresses 2 of 143 DNFs (1%).**

I had a reason to think the judgement was wrong — KM's measurements show `∀` and cardinality
are precisely the two "insufficiency channels" that decide how many concepts certify from
their forward pass, and closing both took `ore_ont_9663`'s deferred set from 2,451 to ~92 of
23k concepts. But that is KM's population, not rustdl's. **Checking before reviving saved the
work**: rustdl's failures are inverse-blocked, and `ore_ont_11311` / `9944` have *zero* ∀,
*zero* cardinality and *zero* union.

Keep the branch parked. Its increments remain sound and may matter for a different goal, but
not for this tail.

## Consequence 2: inverse is the target, and it is genuinely hard

Nobody has solved it cheaply:

* **rustdl's saturator drops inverse semantics.** `ore_ont_11311`'s inverses happen to be
  inert (Konclude gives an identical 10,667-`SubClassOf` taxonomy with and without them), so
  saturation gets the complete answer in 1.13 s — but rustdl cannot know that, and the
  12-axiom ELI probe (`tests/fixtures/eli/inverse-trigger-probe.ofn`) shows saturation
  genuinely missing `A ⊑ ⊥` when an inverse *does* bite.
* **Static detection of inert inverses is refuted** — a generous trigger analysis clears only
  6% of inverse-bearing ontologies and does not include `ore_ont_11311`
  (`docs/2026-08-16-inverse-trigger-analysis-insufficient.md`).
* **KM calls its own inverse-aware saturation "fundamentally expensive"**: forward-only ~10 s
  versus inverse-augmented **111 s** building a 6.5 M-fact model, root-caused to the NF4
  backward-link rule reading predecessor-specific labels across a shared filler
  (`KPSET-PLAN.md`). Their answer is a Konclude KPSet port — designed, not implemented.
* On `ore_ont_7914` — one of rustdl's failures — KM's cardinality port **blows up (>1.3 GB)**
  and defers to CB. **KM has not solved it either.**

## Consequence 3: 6% of the tail is not a reasoning problem at all

The 9 in-EL DNFs are pure scale — 34–394 MB inputs:

| ontology | classes | size |
|---|---|---|
| `ore_ont_9674` | 981,148 | 178 MB |
| `ore_ont_15203` | 264,019 | 394 MB |
| `ore_ont_345` | 89,735 | 111 MB |
| `ore_ont_7507` | 77,610 | 99 MB |
| `ore_ont_8475` | 74,258 | 35 MB |
| `ore_ont_15635` | 415 | 61 MB (ABox giant) |
| `ore_ont_4572` | 36 | 55 MB (ABox giant) |

These need throughput, not calculus. `ore_ont_9674` already classifies at 58.7 s — it is a
cap-boundary case, not a capability gap.

## Where this leaves the goal

The chain of eliminations this session, each by measurement:

| candidate | verdict |
|---|---|
| global-model rewrite as specced (P0/P1) | premise expired — `tableau = 0` on every fixture it named |
| merge-based refuter replacing the label cache | bar quantified at **~99.9% prune**; not refuted, but no headroom |
| static inverse-trigger affected-set | refuted — 6%, misses the motivating case |
| reviving the ∀/`≤1` saturator branch | refuted for this tail — 2 of 143 |
| ∀/card as the dominant channel (KM's finding) | does not transfer — rustdl's tail is inverse-blocked |

**What remains, in order of addressable share:**

1. **Inverse-aware saturation** — 71% of the tail. Hard; unsolved by KM; Konclude's KPSet is
   the known-good design and would be a substantial port.
2. **Union/complement without inverse** — 21%.
3. **Throughput on very large inputs** — 6%.

The honest read is that the tail no longer has a cheap lever. Every constant-factor and
gating idea tried this session has been measured out, and what is left is one large,
well-identified piece of engineering with a published reference design.

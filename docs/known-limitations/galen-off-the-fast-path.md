# galen is off the saturation fast path on this machine, and the in-repo claims about it are unreliable

> **Title corrected 2026-08-20.** This was filed as "PERFORMANCE REGRESSION". That framing is not
> supportable: the baseline it regresses against has no recorded host and the local galen has no
> ontology IRI, no versionIRI, and no entry in the corpus fetch script. See the provenance warning
> below. What is solid is the fragment analysis and the documentation defects, not a slowdown.

**Date:** 2026-08-20
**Severity:** galen costs **874 ms** on the hybrid path, against **77 ms** for the
saturation-only fast path it is documented to take — **~11×**, untracked. (It is also ~1.5× the
0.59 s the docs themselves quote for it.) Not a soundness or completeness defect: the hybrid path
galen now takes is the *more* complete of the two (see "The four subsumptions" below). The harm is
(a) an order-of-magnitude perf gap nobody tracked, and (b) three places in the repo asserting the
opposite, one of them citing a test that does not exist.
**Status:** OPEN — reproduced, not fixed. Deliberately not fixed: whether the regression or the
documentation is the thing to correct is the owner's call (see "Two readings" below).
**Pre-existing:** yes. Not introduced by the incremental-reasoning work; found by it.
**Found by:** Task 9 of `docs/superpowers/plans/2026-08-19-incremental-reasoning-p1.md` (the P1
exit-criterion measurement), sharpened by its reviewer. See
[`../2026-08-19-incremental-p1-latency.md`](../2026-08-19-incremental-p1-latency.md).

## PROVENANCE WARNING — added by the controller after review

**This finding compares numbers that may not be comparable, and the local galen has no provenance.**

rustdl is developed on **two different machines**, and the ~0.5 s figure in `classify.rs:1276` was
not recorded with a host or a file identity. Everything below therefore has to be read as
machine-and-file-specific until that is fixed.

What the local galen actually is:

| property | value |
|---|---|
| path | `ontologies/external/galen.ofn` (gitignored, **not** vendored) |
| sha256 (first 16) | `4b3f900883a9b59c` |
| size | 1,241,952 bytes / 17,329 lines |
| **Ontology IRI** | **NONE — the file declares a bare `Ontology(` with no IRI** |
| versionIRI | none |
| in `scripts/fetch-real-ontologies.sh`? | **NO** — unlike sio, sulo, family, pizza, ro, go-basic |
| classes | 2,748 |
| `InverseObjectProperties` | 207 |
| `FunctionalObjectProperty` | 150 |

Measurement host: Apple M5 Max, 128 GB.

**Consequences, in order of importance:**

1. **The local galen is unidentifiable.** No ontology IRI, no version IRI, not fetched by the
   corpus script, in a gitignored directory. The only handle on it is a sha256 of a file that
   exists on one machine. There is no way to confirm the other machine has the same bytes.
2. **The regression claim is not supportable across machines.** A ~0.5 s figure on an unrecorded
   host cannot be compared to 874 ms on an M5 Max. Different silicon alone can account for a
   large part of that, and this document should not be read as evidence of a slowdown.
3. **The most likely explanation is not a regression at all.** The fragment argument here rests on
   *this* file having 207 `InverseObjectProperties`, which `is_saturator_axiom` rejects. If the
   other machine's galen is a different serialisation or a different GALEN release **without**
   those axioms, it would genuinely be in the saturator fragment and genuinely classify on the
   fast path — and both observations would be correct about different files. That hypothesis fits
   every fact here and requires no regression.

**What survives regardless of provenance**, because it is a property of the code and not of a
timing comparison:

- `is_saturator_axiom` (`classify.rs:1346-1428`) has no `InverseObjectProperties` arm; the comment
  at `:1423-1427` names it as excluded.
- The file at sha256 `4b3f9008…` has 207 of them, so **that file** cannot take the fast path.
- `classify.rs:1252` cites a test `galen_notgalen_in_saturator_fragment` that **does not exist
  anywhere in `crates/`** (grepped). That is a documentation defect independent of any timing.
- `CLAUDE.md` lists inverse-role use as a fallback trigger eight lines above its own
  galen-keeps-the-fast-path claim, so the document contradicts itself.

**Required before this finding can be escalated further:**

1. Add galen to `scripts/fetch-real-ontologies.sh` with a source URL, as every other corpus
   ontology already has — or record where the local copy came from.
2. Re-measure on both machines against a file with a recorded sha256.
3. Only then decide whether there is a regression, a two-different-files situation, or nothing.

## What the repo claims

Three places assert galen takes the saturation-only fast path:

1. `crates/owl-dl-reasoner/src/classify.rs:1250-1252`, in the doc comment on
   `saturator_complete_fragment`:

   > GALEN/notgalen (functional, no disjoint, **no ∀, no chains>2, no inverse**) stay on the fast
   > path — verified by `galen_notgalen_in_saturator_fragment` + the corpus FP/MISSED gate.

2. `crates/owl-dl-reasoner/src/classify.rs:1274-1276`, inside `saturator_complete_fragment_impl`:

   > Empirically confirmed: GALEN classifies on the fast path with the GCI present (closure 27997
   > = Konclude, FP=0/MISSED=0, **~0.5 s**)

3. `CLAUDE.md:814-815`, in the Phase 2b / D10 entry:

   > GALEN/notgalen (EL + functional, no ∀) keep the fast path (**0.59 s** / 1.06 s).

   Note the internal contradiction: **eight lines earlier**, the same CLAUDE.md entry lists
   "**inverse-role *use***" among the constructs that "⟹ fall back to the sound+complete hybrid
   path". galen has 207 `InverseObjectProperties`. The rule that excludes galen and the claim that
   galen is exempt sit in the same paragraph.

## What is actually true

**galen classifies on the hybrid path, at ~0.87 s.**

```console
$ rustdl classify ontologies/external/galen.ofn
# classes: 2748
# mode: hybrid (saturation + tableau)
# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)
# subsumption: saturation=27951 tableau=0
# label heuristic: pruned=2029269 pass_through=9 misses=0
# wall breakdown ms: label_cache_build=427 snapshot_cache_build=0 snapshot_replay=0 tier_walk=449
```

`mode: hybrid`, not `mode: pure EL (saturation-only)`. Note the internal contradiction in that
banner: `# fragment: Horn` is the **pre-D10** clausal-Horn classification, which the D10 work
itself documents as an unsound gate (`classify.rs:1240-1247`); the post-D10 allowlist
`saturator_complete_fragment` rejects galen, so the banner advertises a fragment the dispatcher no
longer honours.

Measured walls (Apple M5 Max, release, median of 3):

| path | wall | vs galen today |
|---|---|---|
| `classify` — what galen actually takes today | **873.7 ms** | 1.0× |
| `classify_saturation_only` — the fast path it is documented to take | **77.1 ms** | **11.3× cheaper** |
| the 0.59 s the docs quote for galen-on-the-fast-path | 590 ms | 1.5× cheaper |

Two gaps, not one. The fast path is ~11× cheaper and still available — galen simply no longer
qualifies for it. And the docs' own 0.59 s figure matches *neither* number: a genuine fast-path run
on this input costs 77 ms, so whatever produced 0.59 s was not the fast path as it exists today
(different hardware, different rule set, or the figure was never a fast-path measurement). Treat
0.59 s as unreliable rather than as the regression baseline.
`tableau=0` is why the regression stayed invisible in the stats banner: the label heuristic prunes
every one of the 2 029 269 candidate pairs, so no tableau probe ever runs and the run *looks* like
a saturation run. The 873 ms is `PreparedOntology` construction + label-cache build (427 ms) +
tier walk (449 ms), none of which the fast path pays.

## The code path that changed

`is_saturator_axiom` (`crates/owl-dl-reasoner/src/classify.rs:1340-1428`) is a strict allowlist.
It has **no `InverseObjectProperties` arm**, and its terminal comment names the exclusion
explicitly (`classify.rs:1423-1427`):

```rust
// EXCLUDED ⟹ fall back to the hybrid path. All ABox assertions;
// InverseObjectProperties decls; Symmetric / Asymmetric / Reflexive /
// Irreflexive; DisjointObjectProperties; SameIndividual /
// DifferentIndividuals — none fully reasoned over by the saturator.
_ => false,
```

`ontologies/external/galen.ofn` contains **207 `InverseObjectProperties`** declarations:

```console
$ grep -oE '^[A-Za-z]+\(' ontologies/external/galen.ofn | sort | uniq -c | sort -rn
3237 SubClassOf(
3161 Declaration(
 699 EquivalentClasses(
 416 SubObjectPropertyOf(
 207 InverseObjectProperties(
 150 FunctionalObjectProperty(
  26 TransitiveObjectProperty(
```

So the `classify.rs:1250` parenthetical "**no inverse**" is simply false of this galen file, and
one `InverseObjectProperties` line is enough to fail the `all(...)` in
`saturator_complete_fragment_impl`. Whether galen gained inverse properties, or the allowlist
tightened around them, is not determinable from the tree — because:

## The cited test does not exist

`classify.rs:1252` offers `galen_notgalen_in_saturator_fragment` as the evidence for the claim.
There is no such test:

```console
$ grep -rn "galen_notgalen" .
crates/owl-dl-reasoner/src/classify.rs:1252:/// `galen_notgalen_in_saturator_fragment` + the corpus FP/MISSED gate.
```

The only occurrence in the repository is the comment citing it. Nothing pins the claim, which is
exactly why a ~10× regression on the flagship ontology could land unnoticed.

## The four subsumptions — why this is a fragment fact, not a tuning knob

The fast path is not merely *not taken* on galen; it **cannot** be taken without changing the
answer. Diffing the two hierarchies:

```console
$ rustdl classify --json ontologies/external/galen.ofn            > full.json
$ rustdl classify --json --saturation-only ontologies/external/galen.ofn > sat.json
```

`full` reports 3291 direct subsumptions, `sat` 3290. Four real subsumptions are present in the
hybrid answer and absent from the saturation closure:

```
Femur            ⊑ BodySpace
Tibia            ⊑ TibialPlateau
TibialTuberosity ⊑ TibialInterCondylarEminence     (the documented back-fold pair)
TricuspidValve   ⊑ ForamenOvale
```

(The saturation-only run also shows three subsumptions the hybrid run does not — `TibialTuberosity
⊑ Eminence`, `TibialTuberosity ⊑ MirrorImagedBodyStructure`, `TricuspidValve ⊑ HeartValve`. Those
are not extra entailments: they are *direct*-parent restatements that surface because the four
above are missing, so the affected classes get re-parented higher. Both runs agree on all 19
equivalence groups.)

`Femur ⊑ BodySpace` and `Tibia ⊑ TibialPlateau` are the inverse-role and functional-merge
consequences the saturator has no rule for; the `TibialTuberosity` pair is the already-documented
back-fold case (see [`galen-defined-class-monotonicity-residual.md`](galen-defined-class-monotonicity-residual.md)
and [`galen-inverse-functional-completeness.md`](galen-inverse-functional-completeness.md)).

**Consequence:** galen's complete classification is not computable from the EL saturation closure,
in principle and not just in this implementation. Any change that puts galen back on the fast path
without first adding the missing inference rules would trade 0.87 s for 0.08 s *and four missed
subsumptions* — the D10 "unsound completeness" bug class the allowlist exists to prevent.

### A null result, recorded so nobody repeats it

An `RUSTDL_HORN_SHORTCIRCUIT` A/B looks like it should settle whether galen takes the shortcircuit,
and does not:

```console
RUSTDL_HORN_SHORTCIRCUIT=1 (default)  → 855.7 ms
RUSTDL_HORN_SHORTCIRCUIT=0            → 871.9 ms
```

The ~2 % delta is inside run-to-run noise (±1.5 % measured over three 100-sample runs), so this
proves nothing on its own. The axiom-histogram and hierarchy-diff evidence above is what settles
it. Recorded here because the A/B is the obvious first thing to try.

## Two readings — the owner picks

1. **The regression is the bug.** galen is documented at 0.59 s and measures 0.87 s, while the
   fast path it is supposed to be on costs 77 ms; the missing
   inference rules should be added so it can go back on the fast path *with* the four
   subsumptions. This is the ambitious reading and is a real engineering project (inverse-role and
   functional-merge completeness in the saturator).
2. **The documentation is the bug.** The allowlist is correctly conservative, galen genuinely needs
   the hybrid path, and the three claims plus the phantom test citation should be corrected to say
   so. This is the cheap reading and is almost certainly *also* needed regardless of (1).

They are not exclusive, and (2) is not safe to do blindly either: deleting the "~0.5 s fast path"
claim would erase the only surviving record that galen once *was* faster, which is the evidence
for (1). Hence: filed, not fixed. **`CLAUDE.md` and `classify.rs` are deliberately left untouched.**

## Also worth pinning while here

Whatever is decided, `galen_notgalen_in_saturator_fragment` should either be written or the
citation removed. A comment asserting a perf/fragment property "verified by" a nonexistent test is
worse than an uncited comment, because it stops the next reader from checking.

## How this surfaced

Task 9 measured `IncrementalSession` per-revision latency and found galen at 886 ms/revision
against a 12 ms bar. Diagnosing that required establishing which classification path a session over
galen takes — which is how the fast-path claim came to be checked against the binary.

An adjacent finding from the same measurement, recorded in the results doc rather than here: **all
eight local ontologies** (sulo, mie, ro, sio, paper5, pizza, family, galen) report `mode: hybrid`.
galen is the only one whose documentation claims otherwise, but it is not the only one off the fast
path.

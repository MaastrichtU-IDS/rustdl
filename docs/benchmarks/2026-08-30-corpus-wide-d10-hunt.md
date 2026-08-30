# Corpus-wide D10 hunt with `verify-el` — 0 new engine defects, 2 instrument defects

**What.** `rustdl verify-el` run over all **1,920** ORE ontologies on the post-#81 engine
(pin `57ef02ff0b39`), 60 s per-ontology cap, single-thread, via `owl-reasoner-harness`. The
`owl-dl-verify` crate exists to find D10 instances — a fragment gate certifying completeness
while the engine drops an axiom — and it has now found real ones (#80, #81, #82), so the question
is whether any remain on a cleaner engine.

## Result

| verdict | count | meaning |
|---|---:|---|
| unresolved | 1,357 | off-fragment or refused — never checked |
| verified | 358 | model built and every axiom holds |
| **NOVERDICT (rc 124)** | **196** | **timed out — UNMEASURED, not passing** |
| **violated** | **8** | candidates |
| NOVERDICT (rc 1) | 1 | I/O or parse error |

**Actually checked: 366.** All **8** `Violated` adjudicated: **8 of 8 are FALSE POSITIVES of the
instrument**, from two distinct mechanisms, and **zero new engine defects were found.**

## F4 (new) — `SubClassOf(owl:Thing, C)` is not applied to model elements

`ore_ont_11522`, `ore_ont_14128`, `ore_ont_14826`, `ore_ont_7270` (80–122 classes).

The violation count equals the ontology's count of explicit `owl:Thing`-LHS axioms **exactly**:

| ontology | `SubClassOf(owl:Thing, …)` axioms | violations |
|---|---:|---:|
| ore_ont_14128 | 2 | 2 |
| ore_ont_7270 | 2 | 2 |
| ore_ont_11522 | 8 | 8 |
| ore_ont_14826 | 19 | 19 |

The reported element is a Tseitin synthetic — e.g. `Element(79)={<synthetic#80>}` — whose label
carries only itself. `⊤ ⊑ C` requires EVERY element to be a `C`; the builder never propagates it,
so the checker reports the very axiom that should have closed the label.

**The engine is right and the instrument is wrong.** `rustdl subclass-expr <ont> "owl:Thing"
"<C>"` returns **`yes`** on every probed pair (5/5). rustdl simply omits `owl:Thing` from
classification OUTPUT — a reporting convention, not a completeness gap.

> **This nearly read as an engine gap.** Against Konclude the four ontologies show MISSED=8/2/19/2
> with EXTRA=0, all of the form `Thing ⊑ RELAPPROXCnnn`, and Konclude emits
> `EquivalentClasses(Thing, C131, C142)` where rustdl emits `[[C131, C142]]`. The discriminating
> check is that rustdl reports **zero** `Thing` rows anywhere, i.e. a convention rather than a
> selective miss — and then that it answers `yes` when asked directly. The project has hit this
> before: 73% of an apparent ~1,795-row gap against Kobayashi-MaRust was the same ⊤-equivalence
> convention artifact.

**Consequence beyond the instrument:** any oracle comparison that counts `Thing ⊑ C` rows will
report spurious MISSED against Konclude on any ontology asserting `⊤ ⊑ C`. Normalise ⊤ out, or
report it separately.

## F5 (new) — `BoundTripped` yields `Violated`, not `Unresolved`

`ore_ont_12317`, `ore_ont_13220`, `ore_ont_15249`, `ore_ont_283` (50k–60k classes).

All four have **zero** `owl:Thing`-LHS axioms and produce violations on the order of the domain
size (25,369 / 20,292 / 54,304 / 92,573). Every domain exceeds the builder's
`Bounds::max_elements = 50_000`, and the reasons list confirms it:

```
verdict: violated   domain: 50738   violations: 25369
unresolved: ['BoundTripped { bound: "max_elements", limit: Some(50000) }', … ]
```

The model was TRUNCATED, so most axioms cannot be witnessed — the violations are artifacts of the
cut, not engine defects.

**This is an asymmetry in `fold_build_reasons`.** Its `Verified` arm downgrades to `Unresolved`
whenever build reasons are non-empty — the safety-critical arm that stops a false all-clear. Its
`Violated` arm APPENDS the reasons and keeps the verdict. That is right for a complete model with
a minor residual and wrong for `BoundTripped`, where the model is known-incomplete by
construction. A truncated model should exit **3** (no verdict), not **2** (real defect).

## What this does and does not establish

**Does:** on the post-#81 engine, `verify-el` finds **no new D10 instance** across the 366
ontologies it can actually check, and its false-positive rate there is **8/366 (2.2%)**, all
attributable to two instrument mechanisms rather than to the reasoner.

**Does NOT:** clear the 196 timeouts or the 1,357 off-fragment ontologies — those were never
checked, and a timeout is *no verdict*, not a pass. Coverage of the corpus by this instrument is
**19%**, and the largest single obstacle is the pure-EL fragment restriction, not defect density.

## Method note — the instrument was broken on the first pass and its numbers were NOT reported

The v1 wrapper merged stderr into stdout and read line 1, so rustdl's dropped-axioms stderr
warning shadowed the verdict on **638 of 1,920** ontologies (442 read as `warning`, 195 blank).
It passed its smoke test because all three fixtures happened to emit no warning — **a guard tested
only against clean input.** Since a violated ontology emitting a warning would land in that
bucket, the v1 `violated` tally was a LOWER BOUND, not a count.

Fixed by discarding stderr and matching the verdict by PATTERN anywhere in stdout rather than by
position, re-running the 638 plus a **60-ontology control** drawn from the cases v1 did classify.
The control agrees **60/60**, which is what makes it safe to keep v1's verdicts for the other
1,282 rather than re-running everything.

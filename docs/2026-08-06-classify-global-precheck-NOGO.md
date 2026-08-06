# A global consistency pre-check on `classify` is NO-GO: addressable set is 2 ontologies, cost is corpus-wide

**Date:** 2026-08-06 · **Binary:** pinned `bin/rustdl-invpair-9f74f15`
**Supersedes the open question in** `docs/2026-08-06-classify-global-consistency-scoping.md`,
which distinguished this proposal from the previously-refuted `decide(Top)` probe and said
the deciding input was corpus-scale cost. Measured; the answer is no, and for a different
reason than the old dead-end.

## The proposal

`classify` runs an inconsistency pre-check that misses inconsistencies only the global
consistency route finds (`ore_ont_16372`: `consistent` says `inconsistent` in 0.13–0.37 s,
`classify --json` says `consistent: true`). The proposal was to reuse `is_consistent`'s
pipeline as a **budgeted** classify pre-check.

## Cost: measured over all 1,920 ORE ontologies

`rustdl consistent`, 10 s cap, single-thread, 4-way concurrency — the cap deliberately far
above any plausible budget so the distribution's shape is visible.

| | |
|---|---:|
| completed within 10 s | 1,571 |
| **did not finish in 10 s** | **349 (18.2%)** |
| median | **0.10 s** |
| p90 | **1.98 s** |
| p95 | 3.97 s |
| p99 | 8.53 s |

Ontologies that would **exceed a budget and return nothing**:

| budget | exceed | share |
|---:|---:|---:|
| 0.25 s | 452 | 23.5% |
| 0.5 s | 330 | 17.2% |
| 1.0 s | 242 | 12.6% |
| 3.0 s | 116 | 6.0% |

**The median is cheap and the tail is not.** "Cheap on 6 curated fixtures" (pizza 0.02 s,
sio 0.20 s) was, as suspected, weak evidence: at corpus scale roughly a fifth of ontologies
would pay a full budget for no verdict. Against a median classify of ~50 ms, a 0.5 s
pre-check is an order-of-magnitude regression on the ontologies it does not help.

## Benefit: 2 ontologies

`consistent` calls **41** of the 1,920 inconsistent (identified by digest class — there are
exactly 2 digests among completers, anchored on the known-inconsistent `ore_ont_16372`), and
classify completes on all 41. Checking classify's own verdict on each:

| | count |
|---|---:|
| classify already **correct** (`consistent: false`) | **39** |
| classify **wrong** (`consistent: true`) | **2** |

The two are **`ore_ont_16372`** and **`ore_ont_7610`** — the latter a new find, a second
ontology where classify contradicts `consistent` in shipped defaults.

## Verdict: NO-GO

**2 ontologies fixed against ~330 paying a budget for nothing** (at 0.5 s), on a change that
touches every classify call. That ratio is not defensible, and no budget choice rescues it:
lowering the budget shrinks the tax but also drops `ore_ont_16372`, which needs ~0.4 s.

The existing pre-check is doing far better than the single failing example suggested — **39
of 41, i.e. 95%**. The gap is a 2-ontology defect, not an architectural one, which is the
opposite of what it looked like from `ore_ont_16372` alone.

## Both remaining instances characterised — and every candidate fix is measured out

### `ore_ont_16372` — nominal-enumeration pigeonhole, worth exactly 1 ontology

Mechanism known (a class defined twice by different `ObjectOneOf` enumerations plus
`DifferentIndividuals`, forcing two distinct individuals into one slot). `consistent`
catches it; **classify misses it, and so does classify on the minimal 3-axiom fixture and
on scaled variants** — all report `consistent: true` while `consistent` reports
`inconsistent`, with the genuinely-consistent control correctly `true`. So the gap is a
missing `abox_check` pattern, which would slot into the existing P1–P8 architecture, and
the counting argument is sound and search-free: `C ≡ E1` bounds `|C| ≤ |E1|`, `C ≡ E2` with
`E2` pairwise distinct forces `|C| ≥ |E2|`, so `|E2| > |E1|` is a contradiction.

**But the census kills it.** Across all 1,920 ontologies, the number with a class defined
by ≥2 different `ObjectOneOf` enumerations *and* `DifferentIndividuals` is **1** —
`ore_ont_16372` itself. A new default-path pattern in a pre-check that runs on every
ABox-bearing classify, to fix one ontology, is not a good trade.

### `ore_ont_7610` — a different mechanism, and NOT the one this session fixed

**Zero `ObjectOneOf`, zero `DifferentIndividuals`**, so it is not the pigeonhole. It carries
4 `InverseObjectProperties` and 4 functional/inverse-functional declarations — but it
**already declares both halves** (`InverseFunctionalObjectProperty(has_case)` alongside
`FunctionalObjectProperty(case_of)`), so `RUSTDL_INVERSE_PAIR_FUNC`'s derivation is
redundant there. Measured: the flag changes nothing —
`classify --json` reports `consistent: true` with **91 unsatisfiable classes** at both flag
settings, while `consistent` reports `inconsistent` in **0.08 s**. Its mechanism is
unidentified.

Note classify is being *correctly* conservative in reporting 91 unsat classes without
declaring inconsistency: all-named-classes-unsat is not inconsistency (`{A ⊑ ⊥, B ⊑ ⊥}` has
a model). It simply cannot prove `⊤` unsatisfiable here.

### Status of the divergence question: CLOSED as a documented residual

Every candidate fix is now measured, not argued:

| candidate | verdict |
|---|---|
| budgeted global pre-check reusing `is_consistent` | **NO-GO** — 2 fixed vs ~330 paying a budget for nothing |
| `abox_check` pigeonhole pattern | **NO-GO** — census says 1 ontology corpus-wide |
| `RUSTDL_INVERSE_PAIR_FUNC` | **does not help either instance** (measured) |
| `decide(Top)` probe | previously refuted (hangs on consistent inputs) |

So classify's inconsistency detection stays the sound under-approximation CLAUDE.md already
documents, now with the residual **quantified at 2 of 1,920** and both instances
characterised. That is the useful output: not a fix, but a closed question with the size of
the gap known.

## What this changes

- **Treat the divergence as a 2-ontology bug**, not a missing subsystem. Both are named and
  both classify quickly, so they are directly investigable — the tractable next step, if any.
- **The `RUSTDL_INVERSE_PAIR_FUNC` divergence blocker is narrower than I framed it.** I called
  it a blocker on the strength of one fixture; the surface actually disagrees on 2 of 1,920
  ontologies in defaults. It remains a real defect and the flag still has its own unresolved
  `ore_ont_16372` classify regression, so the flag stays OFF — but the divergence is not the
  architectural obstacle it appeared to be.
- **A per-ontology instance is a hypothesis about a population, not a measurement of one.**
  `ore_ont_16372` looked like the tip of a systematic classify gap. It is 1 of 2. This is the
  fourth time this session that a single instance implied a general problem that measurement
  cut down to size — and, notably, also the second time measurement showed an existing
  mechanism was working better than a failing example suggested.

## Threats to validity

- 4-way concurrency inflates walls, so the cost table is pessimistic and the true
  budget-exceed shares are somewhat lower. Direction is stated rather than corrected for; it
  does not change a 2-versus-330 verdict.
- The `inconsistent` set is identified by output digest, not by parsing verdicts. There being
  exactly **2** digest classes among 1,571 completers, anchored on a known-inconsistent
  ontology, makes the mapping unambiguous.
- The 349 that do not finish in 10 s are excluded from the benefit count; some may be
  inconsistent and undetected by both surfaces. That could only make the *benefit* larger,
  and is bounded by the fact that classify already answers 39 of 41 correctly.

# Performance snapshot — v0.3.16 (2026-06-27)

Measured with the in-tree harness (`owl-dl-bench corpus <dir> --repeats 5`,
median of 5) on the development host. All ontologies classify **soundly**
(`tab-sub = 0` everywhere — every positive subsumption is saturation-derived;
the tableau only refutes) with **FP = 0 / MISSED = 0** against the
Konclude ∩ HermiT oracle on the diffed corpus.

## `ontologies/external` — 612 ms total

| ontology | classes | mode | median |
|---|---|---|---|
| ore-15516-alchoiq | 84 | hybrid | 12.2 ms |
| alehif-test | 167 | hybrid | 39.3 ms |
| ore-15672-shoin | 82 | hybrid | 58.8 ms |
| ore-10908-sroiq | 692 | hybrid | 135.4 ms |
| galen | 2748 | EL | 166.6 ms |
| notgalen | 3087 | EL | 200.0 ms |

## `ontologies/real` — 28.0 s total

| ontology | classes | mode | median |
|---|---|---|---|
| bibtex | 15 | hybrid | 1.6 ms |
| sulo / sulo-stripped | 17 | hybrid | 4.4 ms |
| ro / ro-stripped | 58 | hybrid | 18–24 ms |
| sio | 1585 | hybrid | 481 ms |
| pizza | 99 | hybrid | 1.85 s |
| go-basic | 51967 | EL | 2.29 s |
| family | 58 | hybrid | 3.24 s |
| wine | 137 | hybrid | 20.05 s (default 1000 ms cap) |

`wine` dominates the `real` total, but that is an **operating-point artifact**
of the default 1000 ms per-pair cap: wine's positive subsumptions are all
saturation-derived (`tab-sub = 0`), so the per-pair tableau is pure refutation
and the cap can be tightened with **no completeness loss**. At a wine-appropriate
tight cap wine classifies in **~1.6 s, sound, MISSED = 0** (`--pair-timeout-ms 1`).

## wine is no longer a DNF

At the start of this work wine was a ~49 s DNF (combinatorial
nominal + disjunction). It now classifies soundly in **~1.6 s** — a ~30×
reduction — via, in order of impact:

- **Coupled-saturation ∃-seed** (v0.3.13): seed the per-class wedge search with
  the saturator's named subsumers + derived existential facts. Collapses wine's
  hard nominal/disjunctive model builds (49 s → 3.2 s, sound).
- **Value-derived type-disjointness + tautology-skip** (v0.3.13): wine's
  8 hardest descriptor classes drop to ms-class.
- **Restrict the MRV ⊔-scan to anchored clauses** (v0.3.15) and the
  **per-clause match-plan precompute** (v0.3.14): constant-factor wedge wins
  that roughly halved wine's *default-cap* classify (~41.5 s → ~20 s) and gave
  −5…−12 % across disjunction-heavy SROIQ (sio, ore-10908, ore-15672).

## Where rustdl stands vs other reasoners

Unchanged in character from the head-to-head
([`reasoner-comparison-2026-06-21.md`](reasoner-comparison-2026-06-21.md),
[`perf-2026-06-08-konclude-vs-rustdl.md`](perf-2026-06-08-konclude-vs-rustdl.md)):

- **EL:** rustdl's saturation kernel is the fastest measured — beats whelk-rs
  (1.4–1.9×) and ELK (4.5×) on galen/notgalen, deriving a sound superset.
- **DL:** **Konclude is the speed leader** (mature C++ tableau; faster on every
  real-reasoning ontology). rustdl is competitive but not the leader; the gap is
  engineering maturity on the disjunctive/nominal tableau, not a missing
  technique.

## Residual hard tail

A re-measurement of the ORE-2015 pilot DNF tail on this build: of the original
13 DNF ontologies, 4 now complete; **9 remain hard** at the default cap. They
split into (a) a few small onts with genuine wedge-needed completeness gaps that
are idiosyncratic and tableau-hard (recursive-list ∀/⊔ reasoning, DnS/DOLCE
definitional subsumption, `ExactCardinality` + disjunction), and (b) a few large
onts whose saturator closure is fast but whose per-class wedge label-build is
O(n)-sats × thousands of classes. These need either a deep wedge/tableau
speedup or per-ontology tuning — see
[`build-once-redesign`] history (internal) for the full lever census; every
cheap, sound lever has been measured.

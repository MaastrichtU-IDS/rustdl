# SP-B viability gate — wine ⊔ branches are majority FREE under immediate-clash BCP — 2026-06-23

SP-B (saturation-guided model construction) aims to shrink the wedge's per-test
branching by forcing disjuncts during construction. Before designing it, gated the
load-bearing question (measure-first): **are wine's `⊔` branch points FORCED (≤1
disjunct survives an immediate-clash check ⟹ a context-dependent BCP would not
branch) or FREE (≥2 survive ⟹ genuine combinatorial choice BCP cannot avoid)?**

## Method

Read-only instrumentation at the wedge's `⊔` branch point (`hyper.rs`
`find_open_disjunction` loop): for each pure-Class disjunction, count disjuncts
that would NOT immediately clash with the node's current label (`c ⊓ cb ⊑ ⊥` for
some existing label `cb`, via `disjoint_pairs`). Classify the branch point: 0
survivors = immediate-unsat (BCP prunes the node), 1 = forced (BCP takes survivor),
≥2 = free. `BCP-skippable = forced + immediate-unsat`. Adaptive budget OFF, 30s cap.

## Result

| build | branches | forced(=1) | imm-unsat(=0) | free(≥2) | BCP-skippable% |
|---|---:|---:|---:|---:|---:|
| decide(Alsatian⊓¬American) | 68 796 | 6 192 | 9 | 22 918 | **21.3%** |
| sat(SweetWine) | 682 479 | 85 630 | 78 | 168 437 | **33.7%** |
| sat(Zinfandel) | 604 110 | 67 072 | 28 | 200 961 | **25.0%** |
| sat(RedWine) | 201 055 | 7 072 | 3 899 | 95 252 | **10.3%** |

**66–90% of wine's `⊔` branch points are FREE** (≥2 disjuncts survive the
immediate-clash check).

## Interpretation

- **The cheap form of SP-B — immediate-clash forced-disjunct BCP — is INSUFFICIENT
  for wine.** It would skip only ~10–34% of branch points; the majority are free
  combinatorial choices, and the search is exponential in *those* — removing a
  quarter of an exponential tree is still exponential. No collapse.
- **But this is the IMMEDIATE check only.** Branches clash semi-deep (~209
  `match_attempts/branch`, not at depth 0), so many "free" disjuncts actually clash
  after a few propagation steps — caught by a *deeper look-ahead* (propagate the
  disjunct's consequences to fixpoint, then check clash), NOT by the immediate
  check. Konclude classifies wine in 230 ms (≈ tens of branches/class), which means
  *something* forces ~99% of choices — necessarily a **deep** saturation-guided
  forcing, far beyond immediate-clash BCP.
- So the gate splits SP-B into two:
  - **SP-B-shallow (immediate BCP):** measured insufficient (this gate). ~modest.
  - **SP-B-deep (look-ahead / approximated-saturation forcing):** the real Konclude
    mechanism; the ~99%-forcing that collapses the search. Not measured here; it is
    the hard, multi-month core, and prior cheap proxies in this neighborhood
    (semantic branching = inert, 1-UIP = bjgap≈1 NO-GO) lean skeptical — yet
    Konclude's 230 ms proves a sound deep forcing IS achievable.

## Verdict / decision point

The cheap SP-B will not deliver the wine collapse. The collapse requires deep
saturation-guided forcing (propagate-to-fixpoint look-ahead, Konclude-style), which
is the genuinely hard, multi-month, uncertain core — and per the standing rule
("prove substantial value before landing"), its payoff cannot be cheaply proven in
advance. The honest next gate, if pursued, is a **bounded look-ahead measurement**
(does depth-1/2 propagation force most of the 66–90% "free" disjuncts?) before
committing months to the deep build. SP-A remains a sound, FP=0 foundation on the
`feat/build-once-redesign` integration branch regardless.

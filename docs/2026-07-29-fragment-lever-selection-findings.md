# Fragment-gate lever selection: what pays, what doesn't, and why we can't currently tell

**Date:** 2026-07-29
**Status:** Findings + a measured NO-GO. Motivates the first-blocker diagnostic.

## The pattern behind three wins and one NO-GO

Four "widen the fragment gate" levers have now been measured. Payoff is determined by a
single property: **is this construct the LAST thing keeping the ontology off the fast
path?** Nothing else predicts it — not how often the construct appears, not how
expensive the construct is.

| lever | shipped? | payoff | why |
|---|---|---|---|
| Lever 1 — TBox-only gate when nominal-free | yes (v0.3.26) | ~40 ORE DNF recoveries | ABox presence was the last blocker |
| Lever 1b — admit `Bot` in `is_el_concept` | yes (2026-07-20) | fast-path-eligible 46 → 100 | `A ⊓ B ⊑ ⊥` was the last blocker for 54 |
| Atomic-negation → `⊥`-GCI (`RUSTDL_NEG_TO_BOT_GCI`) | yes (2026-07-29) | **13 onts flip to pure-EL, 5 from DNF** | `X ⊑ ¬Y` was the last blocker |
| **Domain/Range negation** | **NO — measured out** | **0** | never the last blocker |

## The Domain/Range NO-GO (measured 2026-07-29)

`ObjectPropertyDomain(r, ¬B)` ≡ `∃r.⊤ ⊓ B ⊑ ⊥` and `ObjectPropertyRange(r, ¬B)` ≡
`∃r.B ⊑ ⊥`. Both are EL-positive, and both are shapes the saturator now handles after
this branch's Part A — so the lever is *mechanically* free. It still does not pay:

- **Only 5 ontologies** in the 1920-ontology ORE pool contain a Domain/Range complement
  at all (`ore_ont_714`, `7639`, `8094`, `10460`, `15993`) — versus 80 for the
  `SubClassOf` form that did pay.
- A semantics-preserving `sed` rewrite of the three clean atomic cases (`15993`, `714`,
  `8094`) into the lowered form flipped **zero** fragment verdicts:
  `15993` out-of-EL → out-of-EL; `714` Horn → Horn; `8094` out-of-EL → out-of-EL.
  The rewrite fired; the ontologies simply have other blockers behind it.

**Do not re-propose this lever without new evidence.** It is not wrong, it is inert.

Also ruled out and worth recording: `EquivalentClasses(A, ¬B)` appears in **114** ORE
ontologies — by far the largest complement population — but it is *correctly* unliftable.
It carries a covering half (`⊤ ⊑ A ⊔ B`) as well as a disjointness half; lifting only the
disjointness would trade a slow-but-complete answer for a fast wrong one. Keeping the
original axiom alongside a derived disjointness is sound but pointless: the `Not` remains,
so the gate still rejects. That population is genuine DL expressivity, not a gate artifact.

## Why lever selection is currently guesswork

The only available blocker histogram (produced 2026-07-28, `frag.py` / `frag2.py`) is
**regex-over-source-text**, not a gate verdict. For the 289-ontology DNF tail it reports:

| bucket | count | share |
|---|---|---|
| needs I+R (inverse + chains/transitivity) | 87 | 30.1% |
| needs O/ABox+I+R | 67 | 23.2% |
| needs O/ABox+D+I+R | 41 | 14.2% |
| needs O/ABox+D | 15 | 5.2% |
| needs O/ABox+D+I | 15 | 5.2% |
| needs R alone | 12 | 4.2% |
| ALCH (nothing missing) | 8 | 2.8% |

Its buckets do not map onto real gate decisions. The clearest tell: "needs R" treats role
chains and transitivity as blockers, but `saturator_complete_fragment` **already admits**
role hierarchy, length-≤2 chains and transitivity, and `decompose_long_chains` reduces
longer ones — so much of the R population is likely not blocked on R at all. This is the
same **grep ≠ gate** trap that once produced a 67-ontology estimate for a lever whose real
gate-eligible count was ~40, and that this session hit again (80 complement-bearing
ontologies, 13 actual flips).

## What would fix it

A **first-blocker diagnostic**: have `is_saturator_axiom` / `is_el_axiom` report *which
axiom and which construct* first disqualified the ontology, instead of collapsing to a
bool, and surface it (e.g. a `# fragment-blocker:` banner line). The gate functions already
visit every axiom — they discard the reason. Read-only, FP-free by construction.

With it, ranking levers becomes a query instead of a build-and-see. It would have returned
"Domain/Range negation: 0 ontologies" in seconds.

**Honest expectation:** if the real histogram confirms inverse/symmetric dominates the
tail, that construct already carries **two recorded NO-GOs** in this repo — inverse-aware
classification (refuted on perf grounds: the saturator answers 100% of positives; the
residual cost is refutation) and backward propagation (NO-GO on payoff-vs-cost, with no FP
net at giant scale). The diagnostic may therefore conclude *the DNF tail is out of reach of
gate levers*, redirecting effort to the dense-timeout working-set memory (10 of 12 measured
timeouts exceed 8 GB resident; `ore_ont_9347` at 35.7 GB). That is a useful answer, and
cheaper to learn from a histogram than from another engine build.

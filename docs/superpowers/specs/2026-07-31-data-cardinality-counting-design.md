# Data-cardinality counting instead of O(k²) DKey disjointness

**Date:** 2026-07-31
**Status:** Design — for adversarial review before any implementation
**Flag:** `RUSTDL_DATA_CARD_COUNTING`, default **OFF** until gates pass
**Evidence:** `docs/2026-07-31-small-dnf-highmem-rootcause.md` (Group B)
**Predecessors:** the bounded DKey seeding (v0.3.29), the non-merging-component gate and the
collapse/broadcast split (both v0.4.6). This is the fourth iteration on the same channel and the
first that is *not* a gate.

## The measured problem

Three ORE ontologies DNF because conversion materialises millions of
`DisjointClasses(DKey, DKey)` axioms:

| ont | classes | `DataPropAssertion` | distinct keys | concept_rules | of which DKey-disjointness | conversion |
|---|---|---|---|---|---|---|
| `ore_ont_16632` | 11 | 17,415 | 6,934 + 1,789 | 6,614,036 | 6,605,217 (99.87%) | 12.4 s / 1.83 GB |
| `ore_ont_11126` | 11 | 16,387 | 6,853 + 1,627 | 6,397,838 | ~99.9% | 11.8 s / 1.77 GB |
| `ore_ont_10425` | 18 | 8,227 | 4,566 + 721 | 4,228,740 | 4,223,387 (99.87%) | 7.4 s / 1.29 GB |

`RUSTDL_DATA_PROPERTIES=0` reduces these to **24 / 24 / 33** rules, so the cost is entirely this
channel.

**Every existing gate correctly declines to help.** `16632` carries **74 `DataMaxCardinality`**
axioms; a `≤n` is a genuine COLLAPSE source, so two distinct data values really can be forced onto
one node and the pairwise disjointness is what detects the clash. `RUSTDL_DKEY_SPLIT_STATS` reports
`would_drop = 0` — correct, not a bug. **These pairs are consumable; suppressing them naively is a
completeness regression.**

So this is not an over-seeding defect. It is the missing algorithm CLAUDE.md already names:

> *"data cardinality — full range-size-aware counting (`≥3 p` over a 2-value range → ⊥) is a
> concrete-domain cardinality reasoner with **zero measured corpus reward**."*

Group B is that reward, now measured.

## The idea

`DataMaxCardinality(≤n, p)` with k distinct values on one node is unsatisfiable iff `k > n`.
That is an **arithmetic** test in O(k). Materialising C(k,2) pairwise-disjointness axioms so a
merge-plus-clash rule can rediscover it is O(k²) — 24 M pairs for k = 6,934.

## Design: two halves, and the second is the FP-critical one

### Half A — the counting rule (pure addition, sound by construction)

A new `abox_check` pattern (call it **P10**): for each `(individual a, data property p)`,
let `k` = the number of **distinct** DKey values asserted on `a` via `p` (or via a sub-property
of `p`, using the existing role-hierarchy closure), and let `n` = the tightest `≤n` bound on `p`
implied by `a`'s types. If `k > n`, the KB is inconsistent.

Sound by construction: it derives ⊥ only from facts already entailed (asserted values plus an
asserted/derived type bound). It is additive, so **on its own it cannot cause an FP and cannot
reduce any axiom count**. It also cannot fix the DNF on its own — see below.

Home: `crates/owl-dl-reasoner/src/abox_check.rs`, which already has P1–P9 and already receives
`abox`, `axioms`, `told`, `hierarchy` via `AboxCheckInputs`. Cost O(assertions).

### Half B — suppression (the part that actually fixes the DNF)

Half A changes no axiom count, so it does **not** fix the DNF by itself. The DNF is caused by
materialisation at conversion; the pairs must stop being emitted. Suppression is where all the
risk is.

**A value×value DKey-disjointness pair may be suppressed only if every route by which it could
be consumed is covered by Half A.** Enumerated routes for such a pair:

| route | mechanism | covered by Half A? |
|---|---|---|
| R-merge | `≤n` / functional merges two successors carrying the two keys → clash | **yes**, when both values are ABox-asserted on the same named individual and the bound is on that individual's type |
| R-forall | `∃p.DKey(v) ⊓ ∀p.DKey(r)` with `v ∉ r` | **no** — but this is value×**broadcast**, not value×value, so it is out of scope |
| R-told | the told-disjoint table, read by `abox_check` P2/P7, `disjointness.rs`, `approx_saturation::is_incompatible` | **NO — open question, see below** |
| R-wedge | `labels_disjoint` / `build_disjoint_pairs` in the hypertableau | **yes** iff the co-labelling can only arise from R-merge |
| R-sat | the EL saturator's `disjoint_pairs` | **yes** iff likewise |

**Proposed suppression condition (deliberately narrow):** suppress the value×value quadrant of a
bucket only when *all* hold —

1. the collapse/broadcast machinery already classifies both keys as **value-only** in the
   component (reuse `broadcast_in` from v0.4.6 — no new classification);
2. the component's only COLLAPSE sources are `≤n`/functional on **data** properties (not object
   properties, and no nominal-forcing range/∀);
3. **every** key in the bucket originates from an ABox `DataPropertyAssertion` on a **named**
   individual — i.e. there is no TBox `∃p.DKey` or `DataHasValue` in a class expression that could
   introduce a value on an anonymous node Half A never sees;
4. there are no `SameIndividual` axioms touching the involved individuals, or the counting rule
   folds them first (a merge of two individuals unions their value sets).

Condition 3 is the load-bearing one: Half A is an **ABox** check over named individuals, so any
value reaching a node by another path is invisible to it. Group B satisfies 3 (0 declared
individuals but 17k assertions, 11 classes, no TBox `∃p.DKey`).

## Open questions for review — I do not have answers to these

1. **R-told is unresolved.** Suppressing the axioms removes told-disjoint entries that
   `approx_saturation::is_incompatible` and `disjointness.rs` read for purposes unrelated to
   cardinality. Losing them is a MISS, not an FP — but is it an acceptable MISS, and does Half A
   cover any of it? My instinct is that these pairs are *not* what those consumers are for, but
   instinct is not evidence.
2. **Is condition 3 checkable soundly?** It requires proving no value can reach a node except via
   an ABox assertion on a named individual. Role chains, inverse properties and `SameIndividual`
   all complicate this. If it is not decidable cheaply, the suppression must be gated on something
   coarser (e.g. "the TBox mentions no data range in a concept position at all", which Group B
   satisfies with 11 classes).
3. **Should Half B be per-bucket or per-component?** Today's lesson (R1 of the collapse/broadcast
   design) was that a per-component drop destroyed working clashes and the correct granularity was
   per-pair. The same trap likely applies here.
4. **Is the ABox-only scoping too narrow to be worth it?** If the answer to (2) forces an even
   narrower gate, the addressable set may shrink below the 3 ontologies that motivate this.
5. **Does `concrete_domain_clash` already do some of Half A?** `dkey_ranges: ClassId → CardRange`
   and `data_counting_classes` already exist on `PreparedOntology`, and `owl-dl-datatypes` has
   `card_sat`. Half A may be partly built. This must be checked before writing new code — the
   session that produced this spec twice discovered a "new" defect that was an unfixed sibling of
   existing machinery.

## Soundness posture

- **Half A is additive** ⇒ cannot cause an FP. Worst case it is redundant.
- **Half B is subtractive** ⇒ cannot cause an FP either (fewer axioms ⇒ fewer clashes ⇒ fewer
  derived ⊥). Its risk is entirely **completeness**: a suppressed pair whose consumption route
  Half A does not cover becomes a silent MISS.
- Therefore the FP=0 invariant is preserved by construction in both halves, and the entire review
  burden is on **completeness of the route enumeration above**. That is the question to attack.

## Gates

1. **The 11 preserved DKey fixtures** (`tests/fixtures/dkey_collapse_broadcast/`) — all must keep
   their verdicts. These exist because a previous naive suppression would have destroyed the D11b
   flagship clash.
2. **The 3 concrete-domain canaries** + `dkey_nominal_range_merge`.
3. **A new counting canary set:** `≤1` + 2 distinct values ⇒ inconsistent; `≤2` + 2 values ⇒
   consistent (boundary); `≤2` + 3 values ⇒ inconsistent; the same three via a **sub**-property of
   the bounded property; and the same three with `SameIndividual` merging two individuals'
   value sets. Negatives-first.
4. **Non-vacuity by sabotage** for both halves: disable Half A and confirm the counting canaries
   fail; force Half B to suppress unconditionally and confirm the fixtures fail.
5. **FP=0 net** 22/0 with closures exact.
6. **Flag-OFF byte-identity** on the curated ABox fixtures.
7. **Recovery, pinned binaries:** `16632`, `11126`, `10425` — concept_rules, wall, RSS, and whether
   any now completes. **State plainly if none does**: reducing 6.6 M axioms may expose a
   *different* bound (these are 7.5 GB / DNF for more reasons than conversion — full is 7.57 GB
   against a 2.44 GB saturation, so ~5 GB is post-conversion).
8. **Full-pool ON-vs-OFF answer identity** via `owl-reasoner-harness compare`.

## Explicitly out of scope

- Range-size-aware counting in its general form (`≥3 p` over a 2-value range ⇒ ⊥). This spec
  handles only the `k` distinct **asserted values** versus `≤n` case, which is what Group B needs.
- Object-property cardinality.
- Group A (`ore_ont_11085`) — a different mechanism entirely, cause still unidentified.

## What this does not claim

- It does not claim the three ontologies will complete. Conversion is 1.3–1.8 GB of their
  5.9–7.6 GB; the rest is post-conversion and unaddressed here.
- It does not claim Half A is novel — it may already exist in part (open question 5).
- The addressable set is **3 measured ontologies**. If review shortens the suppression condition
  further, that number can only go down, and the honest response is then to not build it.

# Stage-4 deep-engine characterization — part 1 (deadline-narrow) — 2026-06-26

**The genuinely-hard wine core is 8 classes, not 19.** The advisor's caution held: "19" was
NoVerdict *at a 2 s deadline*, conflating deadline-starved with truly-hard.

## Measurement (∃-seed default-ON, one 30 s-budget label pass per class)

| deadline | NoVerdict classes (of 137) |
|---|---|
| 2 s | 19 |
| 10 s | 12 |
| **30 s** | **8** |

**Genuinely-hard @30s (8):** `Burgundy, Chardonnay, Gamay, Meursault, PinotBlanc, Port,
Tours, WhiteBurgundy`. The other 11 are deadline-starved (label between 2 s and 30 s —
slow-but-tractable; a larger label-cache budget labels them, but the adaptive-deadline U-shape
makes that net-negative on the wall, so they stay misses-at-the-sweet-spot but are NOT the
engine frontier).

## Proportionality (the decision input)

The deep-engine rearchitecture (per-test tree-shrinking on the nominal+disjunction fragment)
would target **8 classes on one fixture (wine)**. That is a very large, FP-critical subsystem
for a narrow genuine core. The honest framing for the go/no-go: the banked win (∃-seed,
wine 49 s → 3.2 s ~15×, sound corpus-wide, now default-ON on main) already captured the bulk;
the residual 8 are the genuine combinatorial core, and the bet's ROI is 8 classes × 1 fixture.

## Remaining characterization (before any engine build)

- **Dropped-∃ recovery** on the 8 — do they have recoverable (currently-dropped Tseitin/DKey)
  ∃-facts whose translation would collapse them? (Tests whether a cheap translation path
  reopens for some of the 8.)
- **Read one search tree** (e.g. Chardonnay/Burgundy) — what are they branching on
  (value-assignment? ∀-propagation chains? inverse-role interactions?)? This reveals the
  *actual* mechanism the engine work would need — not the guessed CSP.

Only after those does the engine mechanism (and whether it's worth 8 classes) become a
grounded decision. Branch `feat/stage4-engine-characterization`; `main` has the banked win.

## Part 2 — dropped-∃ on the 8 (recovery path is low-value)

Per-class kept vs dropped derived ∃-facts (the 8 genuinely-hard):

| class | kept ∃ | dropped ∃ |
|---|---|---|
| Burgundy | 3 | 1 |
| Chardonnay | 2 | 1 |
| Gamay | 1 | 1 |
| Meursault | 7 | 1 |
| PinotBlanc | 2 | 1 |
| Port | 5 | 1 |
| Tours | 6 | 1 |
| WhiteBurgundy | 5 | 1 |

**Each has exactly 1 dropped ∃-fact, and 1–7 kept facts that ARE seeded and still don't
collapse the class.** So dropped-∃ recovery is low expected value: if 3–7 seeded entailed
∃-facts don't collapse a class, recovering 1 more (a synthetic-filler fact) almost certainly
won't. More importantly, this **confirms the 8 are genuine disjunctive choice, not
∃-starvation** — they have the saturation's determinism seeded and still branch.

## Characterization conclusion

- **Genuine core = 8 classes** (Burgundy, Chardonnay, Gamay, Meursault, PinotBlanc, Port,
  Tours, WhiteBurgundy), all on one fixture (wine).
- **Mechanism = genuine disjunctive model-search** over the wine-descriptor value space that
  all-model saturation cannot determine (they're ∃-seeded and don't collapse; wine-wide the
  search is disj-dominated, merge≈0). Not CSP (values already seeded), not ∃-starvation
  (facts present), not dropped-∃-recoverable (1/class, low value).
- **The deep-engine subsystem would target these 8 disjunction-bound classes on 1 fixture** —
  per-test tree-shrinking on genuine nondeterminism, the no-cheap-entry frontier. Proportionality
  is poor: a large, FP-critical engine for 8 classes after the ∃-seed already banked 15×.

**Recommendation:** bank the shipped 15× win as the result; treat the characterized 8-class
genuine core as the documented frontier and the deep engine as a scoped *future* option, not
a now-justified build. (Engine first-step, if ever pursued: a read-one-tree instrument to
confirm the precise branch structure before committing — but the ROI on 8 classes/1 fixture
argues against it.)

## Part 3 — read-one-tree + revisit/context (the mechanism, conclusive)

**Read-one-tree (sat(Gamay), first 60 ⊔ decisions):** branches are almost all **Class
disjunctions** (arity 2–5, `["Cls","Cls"]` … `["Cls"×5]`), overwhelmingly on the **home node
(node 0)**, with depth resetting and re-ascending on the same nodes (0/12/13) at identical
arity/kind signatures — i.e. **re-derivation of a small state set**, not a wide product.

**Revisit + context probe (sat(Gamay), post-∃-seed, 100k ⊔ visits):**

| metric | value |
|---|---|
| total ⊔ visits | 100 000 |
| distinct node **label-sets** | **70** |
| distinct **label-set + edge/neighbour context** | **81 303** |

**Reading (conclusive):** the wedge re-computes ~70 label-states ~1 400× each — but under
**81 303 distinct contexts** (successor/edge structure differs almost every visit). So:
- **Label-keyed memoization is UNSOUND** (same labels, different context = the reuse-trap;
  the SP-0-era "0 context-independent" violation, **re-confirmed post-∃-seed** — the ∃-seed did
  NOT make it cacheable).
- **Sound (label+context) memoization ≈ 19% reuse** (81 303/100k distinct) — marginal, nowhere
  near the 1 400× needed to collapse it.

## Conclusion — GENUINE; the only path is the from-scratch integrated engine

Every cheaper mechanism is now measured-out on the genuine core:
- richer saturation — rules (∀/≤1/nominal) already present;
- dropped-∃ recovery — 1/class, classes seeded-but-uncollapsed;
- memoization — label-keyed unsound (reuse-trap), context-keyed ~19% (marginal).

The genuine-hard core (8 wine classes) has a **genuinely context-rich disjunctive state
space** (70 label-configs × ~81k contexts). Konclude resolves this class in ms because its
**integrated representation** makes these context-rich states efficient/cacheable where
rustdl's wedge re-derives them. **The deep engine is therefore a from-scratch Konclude-style
SROIQ tableau** (integrated nominal/merge representation + sound completion-graph reuse under
applicability conditions), not a bolt-on to the wedge — a large, correctness-critical,
multi-sub-project program. Characterization complete; this is the scoped engine target.

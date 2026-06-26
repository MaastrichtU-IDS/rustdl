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

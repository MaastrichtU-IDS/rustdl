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

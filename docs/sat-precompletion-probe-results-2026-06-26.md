# SP3 Phase-1 precompletion-graph probe — RESULTS + VERDICT (2026-06-26)

**VERDICT: GO (with a non-uniformity caveat).** Seeding the saturation's derived ∃-facts
(`Zinfandel ⊑ ∃hasColor.{Red}`-style deterministic value-assignments) into the wedge
**collapses the hardest wine class CabernetFranc DNF→209 ms (~250×), verdict-preserved
(Sat)** — the amplifier the named-subsumer seed (SP2, ~7.5% ceiling) could not reach. The
nominal translation (NomKey → wedge nominal via the shared `IndividualId`) is sound; the
garbage control confirms it's the *real* saturation ∃-knowledge, not arbitrary structure.
Caveat: the win is **not uniform** — one breadth class (Chardonnay) regressed, so Phase-2
must measure the *net* classify wall and likely apply the ∃-seed selectively.

## Probe (depth 256, adaptive budget OFF, 60 s, modes: none / named-only / named+∃ / garbage)

| class | named-only | named+∃ (n_exist) | factor | verdict |
|---|---|---|---|---|
| **CabernetFranc** | **Stalled 933 143 / 60 s (DNF)** | **3 752 / 209 ms** (5) | **~250×** | Sat ✓ |
| CabernetSauvignon | 7 298 / 390 ms | 1 614 / 77 ms (3) | 4.5× | Sat ✓ |
| Merlot | 9 443 / 482 ms | 2 700 / 121 ms (3) | 3.5× | Sat ✓ |
| WhiteWine | 1 227 / 63 ms | 326 / 14 ms (1) | 3.8× | Sat ✓ |
| **Chardonnay** | 297 931 / 16 s | **766 668 / 42 s** (2) | **0.4× (REGRESS)** | Sat ✓ |

(CabernetFranc `none`-mode = Sat 805 261 / 51 s; `named-only` actually DNFs it — the named
seed alone does not help this class. `garbage-∃` mode = Unsat / 0 branches: arbitrary ∃-clauses
over-constrain to a trivial wrong Unsat, never a correct fast Sat — the control passes.)

## Interpretation

- **The ∃-seed is the wine amplifier.** CabernetFranc — the documented hard class that DNFs
  even with the named seed — collapses to **209 ms** when seeded with its 5 derived ∃-facts.
  These are the deterministic value-assignments (`∃hasColor.{Red}` etc.) that resolve wine's
  value-search; named subsumers can't, ∃-facts do. The arc's goal (a sound sub-second wine
  collapse) is reached on the hardest class.
- **Sound.** Every verdict stays `Sat` (wine classes are satisfiable; the ∃-facts are
  all-model entailed, so seeding is monotone). The nominal translation fires (`n_exist > 0`)
  and produces no wrong verdict. The garbage control gives wrong-Unsat — confirming the
  collapse is the *real* saturation knowledge, not label-count or arbitrary ∃-structure.
- **Not uniform (the caveat).** Chardonnay *regressed* (297 931 → 766 668 branches, 16 s →
  42 s) — sound but slower. The extra ∃-structure (deterministic successors) can reorder the
  MRV search unfavorably for some classes. So the ∃-seed is a large net win on the hard tail
  but occasionally a per-class loss.

## Consequence → Phase-2 (production)

GO to Phase-2: **wire the ∃-seed into `classify_labels`** (the label-cache build, where the
SP2.1 lesson located the wine wall), then gate on:
1. **Full-corpus `konclude_closure_diff` FP=0/MISSED=0 byte-identical** (the classify-scale
   soundness proof — Phase-1 only checked per-class verdict preservation).
2. **Net wine classify wall, flag ON vs OFF** — the real number, accounting for the
   Chardonnay-style regressions. If net-positive, ship; if the regressions cancel the hard-tail
   wins, add **selectivity** (apply the ∃-seed only where it helps — e.g. gate on the seeded
   sat completing faster, or only seed classes whose named-only sat times out).

This is the genuine path to the Konclude-class wine collapse: SP2 (sound named seed, shipped) +
SP3 (sound ∃-seed, collapses the hard tail ~250×) — the coupled-saturation precompletion,
validated sound on the hardest class, reversing the arc's eight prior NO-GOs.

## Disposition

Probe on `feat/sat-precompletion-probe` (saturator `saturate_with_exists_facts` accessor +
`precompletion_probe` fn, commit b27119b; gate `precompletion_probe_gate.rs`). The accessor is
the keep-on-GO piece; the probe fn/harness are diagnostic. `main` untouched. Phase-2 is a
separate spec.

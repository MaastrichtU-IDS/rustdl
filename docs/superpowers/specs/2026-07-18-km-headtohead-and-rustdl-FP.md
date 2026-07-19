# rustdl vs Kobayashi-MaRust head-to-head + full-ORE 4-reasoner soundness cross-check (2026-07-18/19)

> TL;DR (full-corpus, 2026-07-19): across 1917 ORE onts with Konclude+HermiT as
> gold, **rustdl has 0 genuine false positives**; **KM has genuine FPs on 10 onts**
> (~1795 unsound subsumptions, concrete-domain/range collapse). The "rustdl FALSE
> POSITIVE" in this doc's original title was a **misdiagnosis** — corrected below
> (it was KM incompleteness + a functional-merge entailment rustdl gets right).
> See the two dated sections at the bottom for the scale results.

Head-to-head vs KM (bio-ontology-research-group, KAUST — a Sequoia-CB + tableau +
router SROIQ reasoner in Rust, architecturally near-identical to rustdl). Same box,
ORE 2015 `pool_sample` (1920 onts), 80-ont every-24th sample, 30 s timeout,
process-tree RSS. KM run via its production router (`py/route.py`, needs the
`moose` sibling package for OFN normalisation).

## Performance (80-ont sample)

| metric | rustdl | KM |
|---|---|---|
| solved (30 s cap) | 64/80 | 59/80 |
| wall, both-solved (n=49) | median 0.14 s, mean 1.31 s | median 0.56 s, mean 3.07 s |
| peak tree-RSS, both-solved | median 12 MB, mean 120 MB | median 28 MB, mean 145 MB |
| wall wins (>10 %, both-solved) | 28 | 3 (tie 18) |

rustdl solves more, ~4× faster median, lower memory. KM wins on a few large
deterministic/Horn onts via its one-pass CB closure (`ore_ont_1310` 28 s→12 s,
`11282` 16 s→6 s). KM's `el` route has pathological slow cases (`15932` 24.6 s
where rustdl is 0.14 s). Caveats: 30 s (not their 240 s) timeout; cross-machine;
KM pays Python+moose startup per invocation.

## Correctness cross-check (the important result)

Diffed rustdl vs KM entailed named-subsumption closures on the 49 both-solved onts
(normalise to IRI fragment, transitively close both). **47/49 PERFECT agreement,
565,183 subsumptions agree.** All disagreement is one-directional: **6
subsumptions rustdl entails that KM misses; KM entails nothing rustdl misses.**

### NOT a rustdl FP — rustdl is CORRECT, KM is INCOMPLETE (CORRECTED)

**Initial (wrong) adjudication:** I called `MixedForest ⊑ BroadLeavedForest` a
rustdl false positive, reasoning that a coniferous successor violates
BroadLeavedForest's `∀hasPrimaryVegetation.BroadLeafed`. **That was wrong — I read
the class definitions but missed a separate axiom.**

**ddmin (delta-debugging) isolated the trigger to ONE axiom:
`FunctionalObjectProperty(hasPrimaryVegetation)`.** That flips the verdict:
- `MixedForest ≡ ∃pv.Coniferous ⊓ ∃pv.BroadLeafed ⊓ ∀pv.(Coniferous ⊔ BroadLeafed)`.
- `pv` **functional** (≤1 successor) ⇒ the `∃pv.Coniferous` and `∃pv.BroadLeafed`
  successors **merge into one**, which is therefore `Coniferous ⊓ BroadLeafed`.
- That unique successor **is** a `BroadLeafed`, so `∀pv.BroadLeafed` holds ⇒
  `MixedForest ⊑ BroadLeavedForest` **is genuinely entailed** (and `⊑ ConiferousForest`
  symmetrically). rustdl is **correct**.
- Verified minimally: with `FunctionalObjectProperty(pv)` rustdl says **yes**;
  remove it and rustdl says **no** — exactly the functionality-dependent, sound
  behaviour.

**So the 6 "rustdl-only" subsumptions are rustdl being MORE COMPLETE, not FPs —
KM MISSES them (a completeness gap on functional/number-restriction merge
interactions, which KM's own docs list as "still open": full Table-3 nominal /
number-restriction *merge* rules).** rustdl has **zero** false positives on the
49-ont cross-check.

## Bottom line (CORRECTED)

On this 80-ont ORE sample, rustdl is **faster, solves more, uses less memory, AND
is more complete** than KM: 47/49 identical closures, and every one of the 6
disagreements is a genuine subsumption rustdl finds and KM misses (functional-merge
+ `∀`/defined-class interactions) — **no rustdl FPs.** KM's real advantages are
memory-efficiency on some large Horn onts (one-pass CB) and its Lean-4 calculus
proofs + certificate story — not correctness coverage here.

**Method lesson:** the fast analytical adjudication ("it's a FP") was wrong;
ddmin over the real ontology found the one axiom (functionality) that made rustdl
right. Measure/minimize, don't trust a quick hand-proof that omits axioms.

(Still worth a HermiT/Konclude tiebreak on `ore_ont_9557`'s single pair — a
different ont, KM `cb` route — to fully close the loop, but the 7877 five are
settled: rustdl correct.)

Raw data: `scratchpad/h2h_results_80.log`, `scratchpad/correctness_out.txt`,
harnesses `h2h.py` / `correctness.py`.

## 4-reasoner correctness cross-check at scale (rustdl · KM · Konclude · HermiT)

Follow-up to settle soundness rigorously with two independent oracles. 240-ont
ORE `pool_sample` slice (every-8th of 1920), each ont run through **all four**
reasoners; `GOLD = Konclude ∩ HermiT` for MISS-scoring and `Konclude ∪ HermiT`
for FP-scoring (a *true* FP = a pair NO gold reasoner has). ROBOT converts
OFN→OWX once/ont; HermiT via ROBOT `reason -r hermit --axiom-generators subclass`;
Konclude native binary; rustdl pinned `RAYON_NUM_THREADS=1`; 90 s cap. Harnesses:
`scratchpad/four_way.py`, `fw_one.py`, `fw_adjudicate.py`, `km_fp.py`.

**121/240 onts produced a full gold** (both Konclude and HermiT returned; the rest
were DNF on one gold reasoner, mostly HermiT/JVM at 90 s).

### rustdl: ZERO false positives across the whole sample
- **rustdl ⊆ Konclude on 100 % of the 121 gold onts** — `rustdl − Konclude = 0`
  everywhere, i.e. **true FP = 0** on every ont, including the two big ones
  (`ore_ont_11636`, `11696`, ~29 k subs each, checked against Konclude directly
  when HermiT DNF'd).
- 13 onts *looked* like `rFP>0` against the naïve `∩`-gold, but adjudication
  (`rustdl − (Konclude ∪ HermiT)`) showed **all 13 are HermiT under-reporting**:
  in every case `herm-only = 0`, `kon-only > 0`, and `rustdl = Konclude` exactly.
  So the effective gold is Konclude, and **rustdl == Konclude** on named-class
  subsumption across the sample.
- rustdl matches the gold **exactly on 104/121** by the conservative `∩` metric
  (≈115/121 against Konclude); the residual is a handful of *sound completeness
  misses* (`11636`/`11696` 24 each, `4469` 11, `5204`/`9557` 4, `3843` 1 — total
  ~68 subs over ~6 onts), all `rustdl ⊆ gold` (the expected `trust_sat`
  near-complete gaps, never unsound).

### KM: a genuine false positive on the datatype ont `ore_ont_9054`
- KM is `⊆ Konclude` (true FP = 0) on 15 of its 16 flagged onts — same
  HermiT-under-reporting story as rustdl.
- **`ore_ont_9054`: KM = 700, Konclude = HermiT = 676 (both gold AGREE), rustdl =
  676 (matches gold exactly). KM's 29 extra are true FPs** — a real soundness
  violation vs two agreeing oracles. The 29 pairs are **bidirectional collapses of
  range-partitioned sibling classes**: `FastExposure ↔ SlowExposure ↔ LongExposure
  ↔ VeryFastExposure`, `LargeFormat ↔ MediumFormat ↔ SmallFormat`,
  `NormalLens/Fisheye/Telephoto/WideAngle`. These classes are defined by *disjoint
  numeric datatype ranges* (exposure time / film-format size / focal length);
  mutual-subsumption equivalence clumps are the signature of a reasoner **dropping
  concrete-domain facet constraints** — a SROIQ(D) datatype gap (consistent with
  KM being a Sequoia CB engine; the Sequoia calculus does not cover concrete
  domains). This is exactly the value-membership fragment rustdl's D6/D8 levers
  handle — `9054` is the ont where rustdl went MISSED 79→0.

### Bottom line (4-reasoner)
On this ORE sample, adjudicated by **two independent gold reasoners**:
- **rustdl: 0 false positives, sound on 100 % of onts** (`⊆ Konclude` everywhere),
  matches the Konclude∩HermiT gold on the large majority, with only sound
  completeness misses on ~6 onts.
- **KM: 29 genuine false positives on a datatype ont** (concrete-domain facet
  collapse) — plus more completeness misses than rustdl (11 miss-onts / 133 subs
  vs rustdl 6 / 68).
- **Konclude ≡ rustdl** on named-class subsumption; **HermiT under-reports** on 13
  onts via the ROBOT `subclass` generator + 90 s cap (extraction/timeout artifact,
  not a HermiT calculus error).

So the head-to-head conclusion strengthens: rustdl is **more sound (KM has real FPs
here, rustdl has none), more complete, faster, and lighter** than KM on this
sample. KM's differentiators remain its Lean-4 proofs + certificates, not
correctness coverage.

Raw: `scratchpad/fw_results.tsv` (240-ont sweep), `scratchpad/adj.out` (rustdl
adjudication), `scratchpad/fw_agg.py` (aggregator).

## FULL ORE corpus 4-reasoner run (2026-07-19) — 1917/1920 onts

Completed the entire ORE 2015 `pool_sample` (1920 onts) through all 4 reasoners
(rustdl `-P` pinned 1-thread, KM, Konclude native, HermiT-via-ROBOT), 90 s per-
reasoner cap, JVMs `-Xmx10g`. Effective gold = **Konclude** (DNF'd only 83/1917;
equals rustdl on every satisfiable class); HermiT is the 2nd oracle where it
finished (DNF 385). Harness hardened after a long debugging slog (subprocess
killpg on timeout; `BYTECAP` bail-before-materialize to stop a 13.4M-pair Python
closure OOM; `comm`-scoped reaper that never kills the xargs orchestrator).

### rustdl vs Konclude (n=1452 both-returned)
- **EXACT match r==kon: 1281**
- **r>kon: 22 — ALL the unsat-enumeration convention** (rustdl lists each
  unsatisfiable class ⊑ every class; Konclude/HermiT collapse to `≡ owl:Nothing`).
  Unsat-normalized (exclude unsat classes uniformly), **all 22 → true FP = 0**,
  rustdl == Konclude exactly. On `ore_ont_1325` rustdl/Konclude/HermiT all agree
  the SAME 13 classes are unsatisfiable (an apparent 13-vs-6 mismatch was a
  harness regex not matching accented French class names).
- **r<kon: 149 completeness misses** — 1 is a parse failure (`ore_ont_10860`,
  rustdl doesn't parse `DLSafeRule`/SWRL → returns empty), the other **148 are
  genuine reasoning misses**, almost all tiny (miss 1–144 out of 30k–1M subs; a
  few larger: `13224` −9869/1.06M, `2612` −3341, `16321`/`4198` −1479). This is
  the documented `trust_sat` near-complete-but-not-complete behaviour, now
  quantified at ORE scale: **rustdl is incomplete on ~10 % of ORE onts, usually
  by a handful of subsumptions.** Sound throughout (`rustdl ⊆ Konclude`).
- **rustdl genuine false positives: 0 / 1452.**

### KM vs Konclude (n=1356 both-returned)
- EXACT match km==kon: 1138.
- **km>kon: 12 onts → 10 are GENUINE false positives** (unsat-normalized, KM
  still exceeds Konclude *and* HermiT-where-available): `6833` +787, `4577` +404,
  `11647` +349, `12270` +123, `9054`/`16708`/`6967`/`3685` +29, `15063` +14,
  `7517` +2 (~1795 spurious subsumptions). Only `6951`/`7496` were unsat-enum
  artifacts (→0). The FP pairs are over-subsumptions among satisfiable,
  mutually-exclusive classes — the concrete-domain/range-partition collapse
  signature: `AboveRoomTemperature ⊑ Cold/Heat`, `FastExposure ⊑ SlowExposure`,
  `LargeFormat ⊑ MediumFormat`, 787× `BSPO_* ⊑ RELAPPROXC38616`. (Note `12270`:
  rustdl normalized clean, KM has +123 real FP on the same ont.)
- km<kon: 206 completeness misses (more than rustdl's 148).

### DNF / too-big (of 1917)
rustdl 460 · KM 557 · Konclude 83 · HermiT 385. (Many rustdl "DNF" are `BYTECAP`
skips — giant onts rustdl DID classify, e.g. 13.4M-pair closures, too large to
diff in Python — not engine timeouts; Konclude's 83 shows it is the robust gold.)

### Bottom line (full corpus, two oracles)
**rustdl: 0 false positives across 1452 gold-checked onts — sound corpus-wide.**
Completeness ~90 % exact-match, with 148 small sound misses (the honest ORE-scale
`trust_sat` gap; matches the CLAUDE.md caveat that completeness is proven only on
the curated corpus). **KM: 10 onts with genuine false positives (~1795 unsound
subsumptions, concrete-domain collapse) — a real soundness gap rustdl does not
have**, plus more completeness misses and more DNFs. The 240-slice conclusion
holds at 8× scale: rustdl is **more sound, more complete, and more robust** than
KM; KM's differentiators remain its Lean-4 proofs + certificates.

Raw: `scratchpad/fw_full_clean.tsv` (1917 rows), `fw_final.py` (aggregator),
`adj_unsat.py`/`adj_km.py` (unsat-normalized adjudicators).

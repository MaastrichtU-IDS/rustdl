# Inconsistency-detection gap — characterization (2026-06-11)

The ORE-2015 doc (`ore-2015-results-2026-06-08.md` line 76) flagged "11 inconsistent
ontologies where Konclude detects it but rustdl `classify` returns a hierarchy and
does NOT flag inconsistency." This characterizes the *current* gap (the pilot data
was stale — A1 ABox-check + D-phases shipped after it).

## Re-measure (today's binary): gap is 6, not 11

A1 ABox pre-check (P1–P8, shipped after the Jun-8 pilot) now **detects 5 of 11**:
ore_ont_443, 2669, 7052, 15288, 15516 → `consistent` correctly returns `inconsistent`.

**Remaining 6** (Konclude finds inconsistent in 0.1–0.6 s):
| ont | abox | `consistent` | `classify` |
|---|---|---|---|
| 15993 | **0** (TBox-only) | hang | **hangs** (label-cache build) |
| 12174 | ~8k | timeout | **silent** (246 classes, no flag) |
| 13219 | 2687 | timeout | silent |
| 6446 | 1731 | timeout | silent |
| 8941 | 1752 | timeout | silent |
| 2749 | 720 CA + 51 DisjointClasses (BioPAX) | **bailout** (internal cap, NoVerdict) | silent (38 classes) |

## Two distinct failure modes (do NOT conflate)

1. **Silent-complete** (2749, 12174, …): `classify` finishes fast, all classes probed
   via **saturation/wedge `trust_sat`** which returns `Sat` → **masks the inconsistency**
   → emits a bogus hierarchy with no warning. This is the real C2 silent hole.
2. **Hang-in-a-boundable-phase** (15993, TBox-only): `--saturation-only` terminates
   (25 classes), but the hybrid path hangs in the **label-cache build** (wedge per-class
   × 5 s deadline × 25 classes). Boundable → the *existing* `warn_if_incomplete` would
   fire with a tighter bound.

## Root cause (the key insight)

`is_consistent` runs `prepared.decide(Top)` on the **MAIN tableau**; everything else
in classify runs the **WEDGE** (hyper engine, fast, trusted). The consistency check is
on the slow, non-default engine — and `decide(Top)` **hangs 60 s even on CONSISTENT
alehif and pizza**. So a *bounded global `decide(Top)` probe* (the first signal idea)
is **unviable**: it would hit its deadline on essentially the entire out-of-EL corpus
(alehif/sio/wine/pizza/ore-10908, all consistent) → false-flag them "undetermined" =
noise, not calibration.

The wedge (`HyperEngine::new(clauses, root: ClassId)`) seeds a single root class +
TBox clauses — it does **NOT seed the ABox**. That is *why* consistency falls back to
the main tableau. Wedge `Unsat` is sound (unconditional); only `Sat` is trust-based.

## Plan (advisor-endorsed; re-scoped from "signal-first")

The clean "signal-first via global probe" the user picked is unviable. Re-scope:

1. **Gate experiment (decides everything — run before building):** wedge-based
   consistency check. TBox-only (15993) needs no ABox seeding (run wedge on `Top`).
   ABox cases need ABox-seeding into the wedge (ground nominals + role edges) — a real
   build. Test: fast + `Sat` on consistent alehif/pizza; sound `Unsat` on 2749 + 15993?
   - If yes: closes the hangs AND gives sound detection from one change (route
     consistency through the trusted fast engine). Minimal new signal surface.
   - If the wedge also masks 2749: detection needs A1-extension (P9 ABox-disjoint-via-
     range/closure) or the cap bump below.
2. **Cheap parallel:** 2749 bailed on `MAX_SEARCH_DEPTH = 256` (lib.rs:1398) — an
   internal recursion cap, *not* the wall clock. Bump it → does 2749 resolve to
   `inconsistent` fast? (Trivial sound win if so.)
3. **Bound the label-cache-build phase** so hang cases (15993) emit the existing
   INCOMPLETE signal instead of hanging.

**Likely honest outcome:** closes the refutable cases soundly + makes the rest
honestly-incomplete — NOT a universal consistency guarantee. That is a fine, on-thesis
result (calibrated incompleteness). The gate experiment determines whether this is a
clean fix or a re-scope; do not write design past it.

## EXECUTED RESULTS (2026-06-11) — cheap routes are dead ends

Ran the planned experiments. Verdict: **no cheap sound win; the remaining 6 are the
closed engine-termination problem.**

- **Cap-bump (step 2) — DEAD END.** Made `MAX_SEARCH_DEPTH` env-tunable, tested 2749 at
  depth 1024/8192/65536: the fast `NoVerdict` bailout becomes a **60 s hang** — the
  main-tableau search over 2749's 720-individual ABox explodes at any depth. Reverted
  the knob (inert, no win).
- **15993 (hang) — bounding works for the SIGNAL.** With `RUSTDL_LABEL_CACHE_TIMEOUT_MS=800
  --pair-timeout-ms 300`, classify terminates and emits the existing `⚠ INCOMPLETE`
  warning (98 pairs). The wedge does NOT refute it fast. Under *defaults* it already
  terminates+flags, just slowly (~125 s = 25 classes × 5 s label-cache). So it is
  *eventually-flagged*, not silent.
- **A1-pattern (P9) detection — DOESN'T FIT.** Clash-finder over 2749 (per-individual
  asserted+range/domain types, closed under told-subclass, vs the 51 disjoint pairs):
  **0 shallow clashes**. 2749's driver is **cardinality (10 Max/6 Min/6 Exact) + 6 ∀ +
  transitive** — a deep tableau-level ABox entailment, the family class of problem, NOT
  an A1-extensible shallow pattern.

**Conclusion.** The shallow ABox clashes are already caught by A1 (5/11). The remaining 6
(15993 TBox-∀/disjoint; 2749/12174/… cardinality+∀ over big ABoxes) need real
tableau/ABox reasoning that rustdl's engine can't *complete* — the same boundary as the
hard classification pairs. Konclude does them in 0.1–0.6 s via optimized ABox
precompletion; rustdl will not win that by extending pre-checks or bumping caps.

**Recommended disposition (on-thesis):** treat as a *documented calibrated limitation*
for the paper's C2, not a bug to fix. Honest framing: "rustdl detects shallow ABox
inconsistencies (A1 pre-check: 5/11 of the ORE inconsistent set, sound); deep
cardinality/∀-driven ABox inconsistencies are not detected — the same engine-completion
boundary as the hard classification pairs." Do NOT reopen the engine-termination /
global-model project for 6 ontologies (it is the documented P2 NO-GO; see
[[next-big-bet-reuse-trap-nominal-termination]]). The one residual robustness nit worth a
separate fix: the silent-complete cases (2749/12174) finish via the EL saturator's
per-class Sat short-circuit with `tableau=0` and no flag — a future "global consistency
not verified" caveat would need to avoid false-flagging the (consistent) majority of
out-of-EL ABox ontologies, which is itself unsolved cheaply.

## DEEP INVESTIGATION + WEDGE GATE EXPERIMENT (2026-06-11, session 2)

User pushed back on the "document as limitation" disposition — correctly. Ran the
advisor's gate experiment: a throwaway wedge-ABox-consistency spike
(`HyperEngine::new_seeded` + `wedge_abox_consistency_spike` in lib.rs + hidden
`rustdl wedge-spike` CLI). Seeds every individual as a node (atomic ClassAssertions
as labels, OPA as edges, DifferentIndividuals as `≠`), runs the hyper engine bounded.

### Gate results (decisive)
| ont | wedge verdict | wall | nodes | note |
|---|---|---|---|---|
| 2749 | consistent (MISS) | 377 ms | 720 | faithful seed (all CA atomic, 0 SameInd) yet no clash |
| 13219 | consistent (MISS) | 0.3 ms | 323 | |
| 6446 | consistent (MISS) | 1.9 ms | 564 | |
| 8941 | consistent (MISS) | 1.9 ms | 215 | |
| 12174 | **stalled** | 5 s | 2198 | only one that didn't terminate (8k-axiom ABox) |
| 15993 | consistent | 0 ms | 0 | abox=0; inconsistency is in 19 **dropped DL-safe rules** → SOUND |
| family | consistent (MISS) | 1.8 s | 508 | |
| **alehif** (control) | **consistent** ✓ | 48 ms | 1387 | soundness gate holds |
| **pizza** (control) | **consistent** ✓ | 0 ms | 5 | |
| **ore-10908** (control) | **consistent** ✓ | 0.5 ms | 18 | |

### What the gate proved
1. **TERMINATION: GO.** The wedge processes 720–1387-node ABoxes in <2 s with **no
   explosion** — the main-tableau hang is NOT inherent to the problem, it's the main
   tableau's *ancestor-only blocking*. The wedge's anywhere-blocking handles them.
   (Only the 8k-axiom 12174 stalled.) My earlier "engine-termination is the wall" was
   WRONG for the consistency check.
2. **SOUNDNESS (seeding): GO.** All 3 consistent controls + family stay `consistent` —
   the ABox seeding manufactures no spurious clash (the dangerous false-INCONSISTENT
   direction is clean).
3. **WEDGE DETECTION: NO-GO.** Even with a *faithful* seed of 2749 (all-atomic CA, no
   SameInd), the wedge finds a (spurious-vs-Konclude) model — its `Sat` is trust-based
   and it does NOT reach these deep clashes. So routing consistency through the wedge
   gives a FAST verdict but its `Sat` cannot CONFIRM consistency and MISSES the deep 6.

### What the clashes actually are (verified, not assumed)
Direct clash-finders came up empty for every shallow pattern:
- 0 disjoint-types-on-one-individual (closed under told-subclass + domain/range).
- 0 functional-data-property-with-2-distinct-values conflicts.
So the 5 genuine ones (2749/12174/13219/6446/8941) are **deep multi-step ABox
entailments** (cardinality + ∀ + disjoint + role-hierarchy + transitive interactions
over hundreds of individuals) — not A1-extensible, not data-functional-conflict.
**15993 is a non-target** (DL-safe-rule-dependent; rustdl drop → sound "consistent").

### ACTION PLAN (evidence-backed)
The detection of the deep 5 is the engine-completion problem (Konclude precompletes
them; the wedge is incomplete-for-them, the main tableau hangs). Do NOT chase it. The
high-value, achievable, on-thesis fix is **architectural**: make the consistency check
FAST and HONEST instead of hanging.

- **A. Wedge-routed consistency (the real fix).** Productionise `new_seeded` (add
  complex ClassAssertions via clausified names, nominal labels + range, SameIndividual
  merge) and route `is_consistent` / classify's global check through it:
  `Unsat` → inconsistent (SOUND); `Sat` within budget → consistent, but on
  out-of-fragment input mark it **trust-Sat (not verified)**; stalled/deadline →
  **undetermined**. Eliminates the HANGS (worst behaviour), gives sound detection of
  every clash the wedge *does* reach (a superset of A1's 8 shallow patterns — measure
  the gain), and a calibrated signal for the rest. **Soundness gate: FP=0 corpus-wide +
  controls stay consistent (already green on alehif/pizza/ore-10908).**
- **B. Honest signal (complements A).** classify flags "consistency not verified" for
  trust-Sat / stalled out-of-fragment inputs instead of silently emitting an
  authoritative hierarchy. Low-noise now *because the wedge is fast* — most consistent
  onts get a real fast `Sat`, not a timeout (the noise objection that killed the
  main-tableau global probe doesn't apply to the wedge).

**Reframe achieved (this IS "getting on top of it"):** "rustdl misses 6 inconsistencies"
→ "5 caught by A1; 15993 is a sound DL-safe-rule drop; 5 are deep ABox clashes no *fast*
engine detects (Konclude precompletes) — and rustdl can be made FAST + sound-Unsat +
honestly-flagged instead of HANGING." The spike scaffolding (`new_seeded`,
`wedge_abox_consistency_spike`, `rustdl wedge-spike`) is the foundation for Plan A;
currently uncommitted, spike-quality.

## AXIOM PINPOINTING (2026-06-11, session 3) — the clashes are DATA-driven, not deep

User pushed for real understanding of the "deep multi-step ABox entailments." Ran
**black-box axiom-category ablation** against the native Konclude oracle
(`Konclude consistency -i`, the binary at the docker snapshot path) on each ont's
cached `canon.owx` (Python `ElementTree` removes a top-level axiom category, re-run
Konclude). **This overturns the "deep object-side multi-step entailment" guess.**

### 2749 full category ablation (Konclude verdict after removing each category)
Removing DisjointClasses / SubClassOf(cardinality+∀) / ClassAssertion /
ObjectPropertyAssertion / Object{Range,Domain} / Transitive / SubObjectProperty →
**still INCONSISTENT**. Removing **DataPropertyAssertion → consistent**; removing
**DataPropertyRange → consistent**. So 2749's clash is purely a **data-property-range
violation** — the cardinality/∀/disjoint axioms are red herrings.

### All 5 genuine targets (DataPropertyAssertion removal → consistent for every one)
| ont | no-DataPropAssertion | no-DataPropRange | no-FunctionalDataProp | clash axis |
|---|---|---|---|---|
| 2749 | consistent | **consistent** | inconsistent | data-range violation |
| 13219 | consistent | **consistent** | inconsistent | data-range violation |
| 6446 | consistent | **consistent** | inconsistent | data-range violation |
| 8941 | consistent | **consistent** | inconsistent | data-range violation |
| 12174 | consistent | inconsistent | inconsistent | data (not range/functional — cardinality?) |

### THE COMPLETE, EVIDENCED REFRAME
The ORE "11 inconsistent onts rustdl misses" decompose into:
- **5 caught by A1** (shallow ABox object clashes).
- **15993** — inconsistent only via 19 **dropped DL-safe rules** → SOUND "consistent".
- **5 (2749/12174/13219/6446/8941)** — inconsistent only via **ABox data-property
  reasoning** that rustdl DELIBERATELY DROPS (Phase D1 + A1-handoff "concrete-domain
  reasoning on DataPropertyAssertion literals — out of scope"). Removing the data
  assertions makes every one consistent. **rustdl's "consistent" is SOUND
  under-approximation, NOT a bug.**

**∴ rustdl has ZERO UNSOUND inconsistency misses on the ORE sample.** Every "miss" is
either detected (A1) or a sound under-approximation of a deliberately-dropped fragment
(SWRL / concrete-domain ABox data). The earlier "deep multi-step entailment / engine
problem" framing was WRONG (assumed from axiom *presence*; ablation proved otherwise).

### PLAN TO ADDRESS (now tractable — a completeness FEATURE, not a bug fix)
"Addressing them" = adding **ABox data-property consistency** (the dropped fragment),
as a sound A1-style pre-check reusing the **existing D5–D11 datatype value machinery**
(`IntegerRange`/`FloatRange`/`OrdRange`/`StrSet`, `subset`/`disjoint`, the literal
parsers). NOT the tableau / engine-termination path.

- **DP-1 Range violation** (closes 4/5: 2749, 13219, 6446, 8941):
  `DataPropertyAssertion(p, a, v)` + `DataPropertyRange(p, R)` with `v ∉ R` ⇒
  inconsistent. Reuse the D-phase literal parsers + range-membership; covers facet
  restrictions, type mismatch, and empty/unsatisfiable ranges. Subproperty-closed
  ranges (a value must satisfy the ranges of all super-data-properties).
- **DP-2 Data-cardinality** (12174 candidate; needs its own pinpoint): `≤n p` (or
  `FunctionalDataProperty`) + `> n` provably-distinct asserted values ⇒ inconsistent.
- **DP-3 Functional/equality** conflict (FunctionalDataProperty + two unequal values),
  using typed literal inequality (the D8 exact `Decimal`, `StrSet`, etc. — NEVER `f64`
  for decimals/strings; the same soundness landmines as D8/D9).

Sound by construction (A1/D-phase posture: `Inconsistent` is unconditional; a positive
requires the literal genuinely outside the constraint). Cheap (ABox scan, no tableau).
Gate: FP=0 corpus-wide + the consistent controls stay consistent. **Open scoping item:**
pinpoint 12174's exact data axis (data-cardinality vs datatype-definition vs
DisjointDataProperties) before fixing the DP-2/DP-3 shape.

## DP-1 IMPLEMENTED (2026-06-11, session 4) — data-range-violation pre-check

Shipped the sound ABox data-range pre-check in `data_axioms.rs` (reuses the
scan + runs at convert time). Mechanism pinned first (per advisor): the clashes
are **datatype value-space-family mismatches**, not facet/enumeration:
- 2749: plain `xsd:string` `"1394"` on `xsd:unsignedLong` range (string vs numeric).
- 8941: language-tagged `"…"@de` (`rdf:langString`) on `xsd:string` range.

**Implementation:** `DtFamily` macro-family classifier (TextPlain / LangString /
Numeric [all numerics merged] / Boolean / Temporal / Binary; unknown → `None`),
`literal_family` / `data_range_family` (union/oneOf/complement → `None`),
`emit_data_range_violations`: for each `DataPropertyAssertion(p,a,lit)` and each
range on `p` or a **super**-dp `q`, if `family(lit) ≠ family(R)` (both classified,
hence disjoint) ⇒ emit `Top ⊑ Bot` once. `Top ⊑ Bot` verified to make
`is_consistent`/`classify` report inconsistent (every class unsat).

**Results:** detects **3 of 5** (2749, 6446, 8941) — `consistent`→inconsistent,
classify→all-classes-unsat (38/38, 33/33, 79/79), matching Konclude.
- **13219** not caught — same-FAMILY value/enumeration violation (`xsd:int` range +
  out-of-set value, `DataOneOf`) → needs **DP-1b** (in-range/enumeration membership,
  reuse `IntegerRange`/`StrSet` value checks).
- **12174** not caught — data-cardinality (not range) → **DP-2**.

**Soundness (FP=0):** only shoiq-knowledge + wine trigger DP-1 corpus-wide (only
fixtures with both `DataPropertyAssertion` and `DataPropertyRange`); both stay
FP=0/MISSED=0 (shoiq 449=449, wine 653=653 — wine's `positiveInteger`-on-
`positiveInteger` is same-family, correctly not flagged). Structural no-op on every
other fixture. **11 negatives-first canaries** (`tests/datatype_inconsistency.rs`)
incl. the `int ⊆ decimal` trap, union-range, unknown-datatype, wrong-property,
wrong-subproperty-direction gates + 4 positives. core (184) + datatype (50) green;
clippy/fmt clean.

### DP-1b (same session) — string `DataOneOf` membership
Pinned 13219: 21 violations of the shape `""` (or any string) asserted on a
property whose range is a string enumeration (`hasHeatedtSeats ∈
{"all","driver",…}`). `emit_data_oneof_violations` reuses `parse_string_range`
(→ `StrSet::Set`) + `exact_string_literal`: an asserted string ∉ the enumerated
set (on `p` or a super-dp) ⇒ `Top ⊑ Bot`. **Closes 13219** (91/91 unsat, matches
Konclude). Sound: `DataOneOf` *range* = value must be a member; exact string
membership; string-only (mixed/typed enums skipped). 5 more canaries (in-enum
consistent, non-string-value consistent [string-only under-approx], unrelated-
property, ∉-enum inconsistent, super-dp propagation). shoiq/wine FP=0/MISSED=0
re-verified.

**Status: ORE inconsistency gap now 5 (A1) + 4 (DP-1/DP-1b: 2749, 6446, 8941,
13219) detected; 1 remains (12174); 15993 is a sound DL-safe-rule drop.**

### 12174 (NASA QUDT) — residual, DEFERRED (deep multi-axiom; DP-2/3)
Full category ablation: the clash needs **ClassAssertion + DataPropertyAssertion
+ SubClassOf + SubDataPropertyOf together** (removing any one ⇒ consistent). The
trigger is a TBox data constraint on a *class* — both `C ⊑ DataExactCardinality(1,
dp)` and `C ⊑ DataAllValuesFrom(dp, R)` (e.g. `QuantityValue ⊑ =1 numericValue`,
`QuantityValue ⊑ ∀numericValue.xsd:double`) — fired via `ClassAssertion(a, C)`
with `a`'s values routed through `SubDataPropertyOf`. Detecting it is a deep
multi-axiom ABox concrete-domain step (individual typing + told-closure + TBox
cardinality/∀ extraction + sub-property value routing + count/range check) — far
heavier than DP-1/DP-1b's direct assertion-vs-range, for a SINGLE ontology.
**Deferred** (the "don't balloon a pre-check into an engine project" line). If
pursued, DP-3 = the `DataAllValuesFrom`-through-typing analog of DP-1 (value family
∉ R ⇒ unsat) + DP-2 = data-cardinality (`≤n`/`=n` dp on C + individual with `>n`
provably-distinct values).

**Net result of this thread:** the ORE "11 inconsistent onts rustdl misses" is now
**fully characterized and 9/10 detected** (5 A1 + 4 DP-1/DP-1b; 15993 is a sound
DL-safe drop, never a real miss). The one remaining (12174) is a documented deep
concrete-domain case, and rustdl's "consistent" on it stays a *sound*
under-approximation (dropped data fragment) — zero unsound inconsistency misses
throughout.

## Soundness posture

Wedge `Unsat` → inconsistent (sound, unconditional). Wedge `Sat` → consistent (the
same `trust_sat` already the default everywhere). Fast timeout → honest "undetermined".
A1 `Inconsistent` is unconditional. No false subsumption is ever emitted; the only risk
is a false "consistent" from trust_sat — which is the *existing* posture, not a regression.

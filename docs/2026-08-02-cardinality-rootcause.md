# Root cause: `ore_ont_10407` (Task A) — the wedge depth cap, not cardinality

**Date:** 2026-08-02 · **Binary:** `rustdl 0.4.11`, `sha256
78b6309aaf46653647c67ffc7406e89f7f1754cc02412dd17c717ee22eabb86f`, built with
`RUSTUP_TOOLCHAIN=stable cargo build --release` · **Oracle:** Konclude
v0.7.0-1138 (native) · **Host discipline:** every probe under
`( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`, run serially.

**Verdict: root cause found, and it is neither of the two named hypotheses.**
The gap is the hard-coded wedge branch-depth cap `HYPER_WEDGE_DEPTH = 256`
(`crates/owl-dl-reasoner/src/lib.rs:1506`) sitting **below this ontology's
genuine requirement of 319**. Both named hypotheses (H1 naive `≤n`, H2 blocking
never engages) are **refuted by measurement**, H2 with a numeric null.

---

## 1. The measured gap

| arm | wall | directs | timed-out pairs |
|---|---|---|---|
| rustdl default, unbounded | **DNF @120 s** (no banner emitted) | — | — |
| rustdl `--pair-timeout-ms 1` | 1.63 s | 510 | 616 |
| rustdl `--pair-timeout-ms 10` | 12.54 s | 510 | 394 |
| rustdl `--pair-timeout-ms 100` | 81.97 s | 510 | 386 |
| **rustdl `RUSTDL_HYPER_DOUBLE_BLOCK=0`, unbounded** | **0.26 s** | **510** | **0** |
| Konclude | 0.19 s (classify 169–186 ms) | — | — |

`directs=510` is **identical in every arm**, and the `DOUBLE_BLOCK=0` closure is
**byte-identical** to the 100 ms default arm. Extra time buys nothing: the 386
hard pairs are non-subsumptions the engine cannot refute within any budget it is
given, not answers it is slowly finding.

**Phase attribution** (`--pair-timeout-ms 100` banner, `# wall breakdown ms:`,
trustworthy on v0.4.11):

```
saturate=1 precheck=0 prepare=2 label_cache_build=113 unsat_probe=0
tier_walk=9214 sweeps=72624 matrix=0 unattributed=2
subsumption: saturation=582 tableau=0     satisfiability probes: saturation=50 tableau=0
wedge-cost-histogram ms: 0|1|2-4|5-9|10-19|20-49|50-99|100-999|≥1000
                         0 | 0 | 0  | 69  | 161 | 0   | 1    |  385   | 0
timed-out pairs: 386        fallthrough: ran=386 rescued=0 noverdict=386 from_diverged=55
```

`sweeps=72624 ms` is 88% of the wall and `tableau=0` — the main tableau is never
used. **All the cost is the wedge.** Conversion is free (`tbox-stats` returns
instantly) and EL saturation is free (`--saturation-only`: **0.01 s**, 890
subsumptions). In the `DOUBLE_BLOCK=0` arm the same banner reads
`sweeps=206 label_cache_build=5 tier_walk=33` and **all 616 pairs land in the
0 ms bucket**.

## 2. What the ontology actually says (Step 2 — read, not grepped)

It is a machine translation of the **XML Encryption / XML Signature schemas**
(`xenc.owl`), 50 classes, no ABox. Three lines:

1. **38 of the 50 "cardinality axioms" are `ObjectMinCardinality(0 R)` — i.e.
   literally `⊤`** (3 more are `DataMinCardinality(0 p)`, also `⊤`). Via
   `EquivalentClasses(X, ≥0 R)` this makes **18 named classes equivalent to `⊤`**
   (rustdl derives exactly that 18-member equivalence group, and Konclude agrees).
2. Because those 18 are `⊤`, **their axioms become global**: `residual-triggers`
   reports **49 residual GCIs, 13 of them disjunctive** (`defer_or: 13`), firing
   at *every* node — including `⊤ ⊑ D:KeyInfoType` where `KeyInfoType` is an
   **8-way `ObjectUnionOf`**, plus 6-way (`X509DataType`), 3-way, 2-way unions,
   **and generating `⊤ ⊑ DataMinCardinality(1 p)` for `Algorithm`, `URI`, `Target`.**
3. Everything else is `DataAllValuesFrom(p, xsd:string|anyURI)`. There is
   **no `⊥`-source at all**: 0 `DisjointClasses`, 0 `ObjectComplementOf`, 0
   `ObjectMaxCardinality`, 0 `ObjectExactCardinality`, 0 inverse, 0 transitive,
   0 functional, 0 nominals, and **0 `ObjectSomeValuesFrom`**.

**A model is obviously small** — a single node satisfies almost everything.
Konclude classifies the expressiveness as `SIN(D)`.

The generating mechanism is subtle and worth naming: rustdl lowers **data
properties to object roles**, so a `DataMinCardinality(1 p)` successor is an
*ordinary node*, which inherits the same global `⊤` label and generates its own
`Algorithm`/`URI`/`Target` successors — an unbounded tree in which **every node
carries 13 disjunctions**. Termination rests entirely on blocking. This is why an
ontology with **zero `ObjectSomeValuesFrom`** produces a completion graph of
~400 nodes (`RUSTDL_TRACE=1`: `graph_nodes=393`).

**Note on the ~570× framing:** the profile that selected this instance
(*"35 cardinality axioms"*) is arithmetically right but semantically misleading —
**41 of those axioms are tautologies.** This is not a cardinality-reasoning
workload at all. Verified by reading, per the Step 2 instruction.

## 3. The single-pair result (Step 3)

`rustdl explain <file> D:RSAKeyValueType D:DSAKeyValueType` →
**`no — answered by tableau (closure didn't witness it; tableau adjudicated)`, in
0.68 s** (mostly process start-up). Three further pairs behaved identically
(0.68 s each).

So **this is neither non-termination nor a slow pair in isolation.** The main
tableau decides these pairs in milliseconds; `explain` routes to it. The failure
is specific to the **wedge oracle used by classify**, which stalls, and whose
fallthrough-to-tableau then inherits an already-exhausted per-pair budget
(`fallthrough ran=386 rescued=0 noverdict=386`) — it is handed ~0 ms and cannot
rescue anything. That fallthrough is doing nothing useful and is worth a separate
look.

## 4. Konclude's own statistics (Step 4) — partially unavailable, reported honestly

What Konclude v0.7.0 **does** expose (`-w 1 -v`): parse 3 ms, preprocessing 2–4 ms,
precomputing 3–5 ms, **class classification 169–186 ms**, total 187–206 ms,
expressiveness `SIN(D)`.

**Satisfiability-test and backtracking counts could not be obtained.** The
counters exist in the binary (`strings` finds `clashed-backtracking-started-count`,
`backtracked-clashed-descriptors-count`, `answering-statistics`, …) and the config
keys `Konclude.Calculation.{Classification,Precomputation,Answering}.CollectProcessStatistics`
are accepted, but **this shipped build emits them through neither the
`classification` CLI nor an OWLlink `GetSubClassHierarchy` response** — both were
tried with the keys set to `true`; the response contains no statistics element.
Recording this as a **tooling dead end**, not a result.

**The question those counts were meant to answer is nonetheless settled**, by an
equivalent measurement inside rustdl. Across the two rustdl arms the **number of
tests is identical** — `pairs_branched: 1726`, `subsumptions: 926`, `616`
classify pairs probed in both — while the wall moves 44 s → 0.44 s. So the gap is
**work per test, not more tests.** Konclude's 169 ms against rustdl's 260 ms in
the fast arm confirms the per-test comparison is close once the pathology is gone.

## 5. Hypotheses

### H1 — naive `≤n` handling (`solve_at_most` partition enumeration): **REFUTED**

*Pre-declared criterion:* H1 requires at-most constraints to be doing the work.

- **Structural:** the ontology contains **0 `ObjectMaxCardinality`, 0
  `ObjectExactCardinality`, 0 `FunctionalObjectProperty`.** The only at-most
  constraints that can arise are `≤0 p` from negating `≥1 p` in a subsumption
  probe — degenerate, with no partitions to enumerate.
- **Decisive (intervention):** the entire gap closes to **0.26 s** with
  **cardinality handling completely untouched** — the only thing changed is the
  blocking mode. A mechanism that is not touched cannot be the mechanism that
  was fixed.

H1 is not supported. Faddoul & Haarslev-style algebraic cardinality reasoning
would buy nothing here.

### H2 — blocking never engages: **REFUTED, with a numeric null**

*Criteria declared before running:* instrument fires iff `is_blocked_calls > 0` in
both arms; H2 confirmed iff the double-blocking arm shows
`blocks_fired / block_eligible < 1%`; **refuted if ≥ 10%**.

`RUSTDL_ANYWHERE_BLOCKING` — **flat null**, recorded as instructed:

| | wall | directs | timed-out |
|---|---|---|---|
| `RUSTDL_ANYWHERE_BLOCKING=1` | 81.82 s | 510 | 386 |
| `RUSTDL_ANYWHERE_BLOCKING=0` | 81.96 s | 510 | 386 |

Forcing anywhere-blocking on classify changes **nothing** (0.2% — noise). That
knob governs the *main tableau*, which this workload never reaches (`tableau=0`).

`rustdl hyper-classify-probe` counters (instrument fired: `is_blocked_calls > 0`
in both arms ✓):

| arm | stalled | max_depth | wall | is_blocked_calls | blocks_fired / eligible |
|---|---|---|---|---|---|
| `DOUBLE_BLOCK=1`, depth 256 | 357 | **256 (cap)** | 44.1 s | 48,733,549 | 35,905,895 / 48,188,627 = **74.5%** |
| `DOUBLE_BLOCK=0`, depth 256 | 0 | 44 | 0.44 s | 438,688 | 346,138 (79%) |

**74.5% ≫ the 10% refutation line.** Blocking engages constantly under
double-blocking. H2 as stated is wrong.

## 6. The actual root cause: the depth cap is below the requirement, and truncation thrashes

Varying only the branch-depth cap, with double-blocking **on**:

| depth cap | stalled | max_depth_reached | wall | subsumptions | block_eligible |
|---|---|---|---|---|---|
| **256 (production)** | **357** | **256 (cap hit)** | **44.1 s** | 926 | 48,188,627 |
| 512 | **0** | **319** | 10.0 s | 926 | 11,850,119 |
| 1024 | 0 | 319 | 10.1 s | 926 | 11,850,119 |

The search under double-blocking is **finite and needs depth 319** — only 63
levels above the production cap of 256. Two consequences:

1. **The cap truncates a search that would have terminated.** Every branch that
   reaches 256 returns `Stalled`, so all 386 pairs are undecidable-by-budget and
   default to not-subsumed.
2. **Truncation is not fail-fast — it is actively more expensive.** The capped run
   does **4.1× more block-eligible work and 4.4× more wall (44.1 s) than the
   completing run (10.0 s)**, for the same 926 answers. On hitting the cap the
   search cannot conclude anything, so it backtracks and re-descends through the
   sibling disjuncts, each re-hitting the cap. The adaptive-budget early-cut only
   caught 55 of 386 (`from_diverged=55`).

**Why depth 319 is needed** is §2: 18 classes ≡ `⊤` ⇒ 49 residual GCIs, 13 of them
disjunctive (8-way, 6-way, 3-way, 2-way), firing at every node of a
data-property-generated tree of ~400 nodes. Plain anywhere subset-blocking is
strictly more permissive than double-blocking (it drops the *same-incoming-role*
and *parent-label-subset* conditions, `hyper.rs:1902–1979`), so it caps the graph
much earlier: **depth 44, comfortably under 256** — hence 0.26 s. Double-blocking
still fires on 74.5% of calls, but leaves ~133× more nodes unblocked
(12.3 M vs 92.5 K), and each surviving node re-instantiates 13 disjunctions.

**Correctness of the fast arm:** `rustdl RUSTDL_HYPER_DOUBLE_BLOCK=0` vs Konclude,
via `owl-reasoner-harness/scripts/normalise.py compare`:
**`FP 0 / MISSED 0`, closures 8 = 8, `unsat_disagreement 0`** — on *both*
ontologies.

## 7. Replication on `ore_ont_9941` (Step 6): **exact**

Same file size, different content hash; it is the **same source ontology** with
the `owlapi#ErrorN` placeholders renumbered. Independently measured:

| | 10407 | 9941 |
|---|---|---|
| `ObjectMinCardinality(0 …)` / `DataMinCardinality(1 …)` | 38 / 9 | 38 / 9 |
| `ObjectSomeValuesFrom` / inverse / max-card / disjoint / nominal | all 0 | all 0 |
| residual GCIs / disjunctive | 49 / 13 | 49 / 13 |
| default classify | **DNF @120 s** | **DNF @120 s** |
| `DOUBLE_BLOCK=0` unbounded | **0.26 s**, 510 directs | **0.27 s**, 510 directs |
| vs Konclude | FP 0 / MISSED 0 | FP 0 / MISSED 0 |

The mechanism explains the twin. **Replicates.**

## 8. Falsifiable predictions (stated before any fix is written)

Neither is implemented; no source was changed in this investigation.

**P1 — raise the cap.** Changing `HYPER_WEDGE_DEPTH` from 256 to ≥512 will make
both ontologies classify with **`timed-out pairs: 0`** and a closure
byte-identical to the `DOUBLE_BLOCK=0` arm, in **5–20 s** each.
*Falsified if* either still reports `timed-out pairs > 0`, or wall > 30 s.
This is a real improvement (DNF → completes) but leaves rustdl ~50× slower than
Konclude, and raising a global cap risks lengthening genuinely-diverging searches
elsewhere — so P1 is a **safety net, not the fix**.

**P2 — gate the blocking mode.** Selecting plain anywhere subset-blocking in the
wedge when the ontology contains **no inverse roles and no max/exact cardinality**
(the constructs double-blocking exists to handle) will make both classify in
**≤ 0.5 s** with **FP 0 / MISSED 0** vs Konclude, closure byte-identical to today's
`DOUBLE_BLOCK=0` arm.
*Falsified if* wall > 1 s, or any FP ≠ 0, or the closure differs, or any curated
corpus fixture changes closure.
**Caveat that must be discharged before shipping:** `hyper.rs:1960` claims the
legacy path is "sound for SHIQ-no-inverse". That envelope needs to be *derived*,
not inherited from a comment, and the gate predicate must be proven to imply it —
in particular whether unqualified `≥n` (Konclude reports `N` in `SIN(D)`) is
safe under subset blocking, and how the data-property-as-object-role lowering
interacts. **P2 is a correctness-gated change, not a perf toggle.**

**A third, cheaper observation worth its own look:** the wedge→tableau fallthrough
ran on all 386 pairs and rescued **0**, because it inherits an exhausted per-pair
budget — while `explain` shows the main tableau answers these very pairs in
milliseconds. A fallthrough that is structurally guaranteed to fail is pure waste.

## 9. Scope

Two ontologies, root-caused to depth. **No population claim is made** — this
document deliberately does not extrapolate to the rest of the expressive tail,
per the plan's standing warning that such statistics have been retracted three
times. Whether other DNFs are depth-cap truncations is a separate, testable
question: the signature to look for is `max_depth_reached == 256` together with
`stalled > 0` in `hyper-classify-probe`.

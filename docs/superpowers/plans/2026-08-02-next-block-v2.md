# Next Work Block v2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`.
> **This supersedes `2026-08-02-next-block.md`, which two adversarial reviews rejected** (6 blockers + a value verdict of "not worth executing as written"). Read `## What v1 got wrong` before starting.

**Goal:** Find why rustdl needs >120 s on ontologies Konclude finishes in 0.2 s, by root-causing **single small instances** rather than running another population statistic. Plus three bounded hygiene tasks.

**Architecture:** Tasks A and B are *investigations* producing a root cause and a falsifiable prediction — no production code. Tasks C–E are bounded changes. A and B are the point of this plan; C–E are cleanup that must not consume the block.

**Tech Stack:** Rust (build with `RUSTUP_TOOLCHAIN=stable`), `owl-reasoner-harness`, Konclude + HermiT as oracles.

## What v1 got wrong — read this first

Two reviews rejected v1. The failures are instructive and each has a rule attached:

1. **An unsound inference, already shipped.** v1 rested on *"deletion is strictly stronger than absorption, so no-rescue-under-deletion implies no-rescue-under-absorption."* **False.** Deletion is semantically *weaker*, not computationally *stronger* — it turns cheaply-proved subsumptions into non-subsumptions that must be **refuted**, and removes clashes that terminate branches. Refutation is where this reasoner's cost lives, so a cut arm can DNF for work the absorbed arm never does. ⇒ **Rule: a deletion-based probe proves nothing unless the cut arm's cost profile also improves** (timed-out pairs / branch counts down).
2. **A dead grep.** v1's cut targeted `SubClassOf(ObjectIntersectionOf` — measured at **955** occurrences against **175,409** `EquivalentClasses(…ObjectIntersectionOf…)`. It would not have fired, repeating a failure from the previous block verbatim. ⇒ **Rule: every intervention must prove it fired, by a pre-declared numeric threshold.**
3. **Would have introduced unsoundness.** v1 said to add a fast path to `is_consistent` gated on a **TBox-only** predicate, which would make it ignore the ABox. ⇒ **Rule: never reuse a gate without also porting its guards.**
4. **A self-blinding analysis script** that dropped any ontology whose *ON* arm timed out — exactly the regression signature it existed to detect.
5. **Selecting against the goal.** v1 picked targets by `concept_rule_or`, whose top-3 are all `many-classes` and one of which **no peer solves**. And 22 of the 54 expressive ontologies have `concept_rules < 1000` — below the lowest band the AUC study measured, so **that study never covered the cluster that matters.**
6. **Hygiene dressed as progress.** Only one v1 task touched the algorithmic question, and it was designed to kill the last lead.

**The governing lesson:** population statistics on this tail have been retracted or bounded **three times** (the residual census, the 300× calibration, the qualified-∃ extrapolation). The project's two largest wins — the blocked-`⊔` termination bug (138 s → 0.05 s) and the `ore_ont_10125` print loop (DNF → 15 s) — both came from **reading one failing instance**.

## Global Constraints

- **FP=0 is absolute.** Any false positive is a hard stop.
- **Build:** `RUSTUP_TOOLCHAIN=stable cargo build --release`. A bare `cargo` FAILS; a skipped build silently reuses a stale binary.
- **Pin binaries** to uniquely named paths immediately after the build that produced them; `sha256sum` before measuring.
- **Cap every probe:** `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`. An uncapped run once OOM-killed this shared host at 237 GB.
- **Prove every instrument fires** before believing its output, by a **pre-declared numeric** criterion.
- **Sabotage every canary**, strictly serially, reporting counts as run **including survivors**.
- **Gates for reasoner changes:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --exclude owl-dl-py` (exclude `owl-dl-py` — pre-existing Python link failure); `./scripts/run-soundness-diff.sh` → **11 VERIFIED, closures exact** (galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16).
- Never `cmd | tail` then read `$?`. `grep -c`/`pgrep -c` print `0` **and exit 1** on no match.
- Corpus: `/data/dumontier/ore-run/pool_sample/files/<stem>.owl`. Commit with `git -c user.name="Michel Dumontier" -c user.email="michel.dumontier@maastrichtuniversity.nl" commit`; do not push or merge.

---

## Task A: Root-cause `ore_ont_10407` — pure cardinality, 50 classes, Konclude 0.21 s

**Why this instance:** 50 classes, **0 `∃`, 5 `∀`, 35 cardinality axioms, 0 nominals, 0 disjointness.** rustdl DNFs at 120 s; Konclude answers in **0.21 s** — a ~570× gap on an ontology small enough to hold in your head. No nominals means the cardinality mechanism is isolated. `ore_ont_9941` is a near-twin (50 classes, same profile, 0.24 s) and serves as a replication check.

**Files:** Create `docs/2026-08-02-cardinality-rootcause.md`. **No source changes.**

- [ ] **Step 1: Establish the gap and where it lives**

```bash
cd /data/dumontier/rustdl
C=/data/dumontier/ore-run/pool_sample/files
( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 RUSTDL_TRACE_RSS=1 timeout 120 \
  ./target/release/rustdl classify $C/ore_ont_10407.owl ) >/tmp/a.out 2>/tmp/a.rss
echo "rc=$?"; grep '^#' /tmp/a.out; grep -o '\[rss\] [a-z_]*=' /tmp/a.rss | tail -1
for t in 1 10 100; do
  s=$(date +%s.%N)
  ( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout 150 \
    ./target/release/rustdl classify --pair-timeout-ms $t $C/ore_ont_10407.owl ) >/tmp/a$t.out 2>/dev/null
  e=$(date +%s.%N)
  echo "pair=$t $(python3 -c "print(f'{$e-$s:.2f}')")s subs=$(grep -c '^direct' /tmp/a$t.out) $(grep -oE 'timed-out pairs: [0-9]+' /tmp/a$t.out)"
done
```

Record the full banner. **`# wall breakdown ms:` is now trustworthy** — its `tier_walk` was a residual subtraction until v0.4.11 and reported 8964 ms for a 6 ms walk, so any older reading of it is void.

- [ ] **Step 2: Read the actual axioms — this is the step that matters**

```bash
sed 's|http://[^ )>]*[#/]||g' $C/ore_ont_10407.owl | grep -E 'Cardinality|AllValues|SubClassOf|Equivalent' | head -60
```

Write down, in the doc, **what the ontology actually says** — the cardinality axioms, what roles they constrain, and whether a model is obviously small. On `ore_ont_3281` this step is what revealed the real structure after a structural profile had *undercounted* by grepping only top-level occurrences.

- [ ] **Step 3: Find one pathological pair and time it alone**

Pick two named classes and run `rustdl explain <file> <sub> <sup>` with a large timeout. On `ore_ont_3281` a single ordinary non-subsumption took **294 s** and *terminated* — establishing search explosion rather than non-termination. Determine which this is. Record whether `explain` says the answer came from closure, wedge or tableau.

- [ ] **Step 4: Ask Konclude what it did, not just how fast**

```bash
/data/dumontier/reasoners/run-konclude.sh $C/ore_ont_10407.owl /tmp/k10407.owx
```
Konclude prints statistics on stderr/stdout. **Capture and report its satisfiability-test and backtracking counts.** This distinguishes *more tests* from *more work per test* — a question answered once on `wine` (~1000× fewer branches) and never repeated. It decides which mechanism is worth building.

- [ ] **Step 5: Test the two named hypotheses**

**H1 — naive `≤n` handling.** rustdl enumerates partitions in `solve_at_most`; Konclude uses algebraic/ILP cardinality reasoning (Faddoul & Haarslev 2010; Steigmiller). Probe with `RUSTDL_TRACE=1` (one line per search/branch decision) and count branches attributable to cardinality. Report the count.

**H2 — blocking never engages.** Check whether any node is blocked at all. `RUSTDL_ANYWHERE_BLOCKING=1` forces anywhere-blocking on classify (default is ancestor-only there); measure with it on. **If it changes nothing, say so** — a null result here is worth recording.

State which hypothesis the evidence supports, or that neither does. **"Neither" is a valid and valuable outcome.**

- [ ] **Step 6: Replicate on `ore_ont_9941`**

Same profile, independent file. A mechanism that explains one but not its twin is not the mechanism.

- [ ] **Step 7: Write `docs/2026-08-02-cardinality-rootcause.md`**

Must contain: the measured gap, the axioms in plain terms, the phase attribution, Konclude's own statistics, each hypothesis tested with its result **including nulls**, and — if a cause is identified — **a falsifiable prediction stated before any fix is written**. If no cause is found, say that; a documented dead end beats a speculative fix.

---

## Task B: Root-cause `ore_ont_2182` — nominals + cardinality, 74 classes, Konclude 0.17 s

**Why this instance:** 74 classes, **146 `ObjectOneOf`/`ObjectHasValue`**, 24 cardinality, 28 `∀`. `ore_ont_16481` is a near-twin (76 classes) for replication. This targets a mechanism CLAUDE.md **already documents as deferred**: nominal-tainted nodes are excluded from *both* blocking predicates (`is_blocked_anywhere` and `is_blocked_ancestor`, `lib.rs:1021/1062`), so a nominal-bearing ontology loses termination-by-blocking entirely. The issue-#35 v4 entry states the complete fix — nominal-aware blocking / an NN-rule redesign — was **deferred**, with only a node-cap safety net shipped. **20 of the 54 expressive ontologies are `nominal+cardinality`.**

**Files:** Create `docs/2026-08-02-nominal-blocking-rootcause.md`. **No source changes.**

- [ ] **Step 1: Same measurement protocol as Task A Steps 1–4**, on `ore_ont_2182`.

- [ ] **Step 2: Test the blocking-exclusion hypothesis directly**

Instrument or trace to answer: **how many nodes are nominal-tainted, and how many are ever blocked?** The hypothesis predicts near-zero blocking and unbounded graph growth. Contrast against a completing nominal-bearing ontology to see what normal looks like.

`RUSTDL_MAX_NODES` (default 50000) caps the deadline-free search; if the graph is hitting that cap, say so — it converts the question from "slow" to "generating unboundedly".

- [ ] **Step 3: Bound the upside before proposing a redesign**

Strip the nominals (replace `ObjectOneOf`/`ObjectHasValue` with fresh atomic classes) and re-measure. **This is semantics-changing and therefore NOT a soundness argument** — per the v1 lesson it proves nothing on its own about a nominal-aware fix. Its only legitimate use is as an **upper bound**: if the stripped ontology *still* DNFs, nominal handling is not the whole story and a blocking redesign would not rescue it. Report it as a bound, never as a confirmation, and report the cut's cost profile (timed-out pairs, branch counts) alongside.

- [ ] **Step 4: Replicate on `ore_ont_16481`.**

- [ ] **Step 5: Write the doc**, same requirements as Task A Step 7, plus an explicit statement of whether the deferred issue-#35 v4 redesign is supported by this evidence — and at what estimated scope.

---

## Task C: DKey volume scan (v1 Task 1, with all four blockers fixed)

**Why:** `RUSTDL_DKEY_EMIT_ORDER` fixes a **live non-monotonic** defect — adding an *unrelated* axiom removes an entailment. `RUSTDL_DKEY_ONEOF_SEED` is the sixth D10 bug. Both are implemented, gated, FP=0-verified, and OFF only for want of this scan.

**Files:** Modify `crates/owl-dl-core/src/convert.rs:2689` and `:2762` (defaults only, if the rule passes). Create `docs/2026-08-02-dkey-volume-scan.md`.

- [ ] **Step 1: Prove the flags fire.** Scan all four arms (neither / EMIT_ORDER / ONEOF_SEED / both) for `concept_rules` via `rustdl tbox-stats`. **`ore_ont_5368` must read 18,620,251 in the OFF arm** — it is the DKey discriminator. **`ore_ont_9347` CANNOT validate this area** (reads 113 under both a correct gate and a no-op build); report it but never judge on it.

- [ ] **Step 2: Record conversion WALL and told-edge counts too, not just `concept_rules`.** `ONEOF_SEED` also emits told `DKey ⊑ DKey` edges, which `tbox-stats` does not print and which `told.rs` closes transitively at build — the v0.3.27 fix was a DNF in exactly that table. Without this the instrument is blind to the failure mode.

- [ ] **Step 3: Treat an ON-arm timeout as a BLOCKING result, not a dropped row.** v1's script filtered to rows where every arm parsed, which would have silently discarded any ontology that converts at baseline and times out only with the flag on — the v0.3.29 conversion-DNF signature. Count `NA` **per arm** and report.

- [ ] **Step 4: Decision rule, fixed now.** Flip a flag ON only if **all** hold:
  - no ontology's `concept_rules` grows **>2× OR by >100k** (v1 used AND, which passes 1 → 99,999);
  - no ontology gains an ON-arm conversion timeout;
  - `ore_ont_5368` is unmoved.
- [ ] **Step 5: FP adjudication on the movers.** This lever's own doc says its failure mode is a **false positive**. For every ontology whose numbers move, diff the closure against **Konclude ∪ HermiT** (`owl-reasoner-harness/scripts/normalise.py compare`). The curated corpus **cannot** validate the DKey area — `datatype_value_membership.rs` says so itself — so the FP=0 net shows non-regression only.
- [ ] **Step 6:** If flipping, use the house default-ON idiom `is_none_or(|v| v != "0")`; update each default test to pin **both** halves (unset ⇒ ON, `=0` ⇒ OFF); run all gates; write the doc and commit.

---

## Task D: Guard the `CLASSIFY_INCONSISTENCY_MS` value (v1 Task 5, tautology removed)

**Why:** sabotage of the v0.4.11 fix showed that slashing the default 3000 ms to 1 ms **passes every canary**. The substance of a shipped default is untested. v1's proposed test asserted `ms >= 2500` and then mutated the constant — arithmetic, not evidence.

**Files:** Modify `crates/owl-dl-reasoner/tests/classify_inconsistency.rs`.

- [ ] **Step 1: Measure `family.ofn`'s actual pre-check cost on the current binary.** The "~2.0 s" figure may be stale — ABox-saturation indexing took family detection from 21 s to ~0.7 s in 2026-07-23. Time `classify` with `RUSTDL_CLASSIFY_INCONSISTENCY_MS` at 0 (unbounded) versus with the pre-check disabled; the difference is the real cost. **Pin the threshold to that measurement with margin**, not to 2500.
- [ ] **Step 2: Write the behavioural test only** — `family.ofn` detected at the default budget, and **not** detected at 1 ms. That is the pair that proves the budget governs.
- [ ] **Step 3: If the 1 ms arm still detects**, do **not** relax the assertion. Find and name the other route, and guard *that*. An unexpected detection is information about the system, not an inconvenience.
- [ ] **Step 4:** Gates and commit.

---

## Task E (optional, only if A and B finish early): domain-absorption default, descoped

v1 proposed two full-corpus sweeps (~8–16 h shared host) to decide a flag that recovers **4** ontologies, and 883 of the 1,913 are provably unaffected. **Instead:** run the **ON arm over only the 1,030 affected ontologies** and diff against the existing v0.4.11 OFF baseline; re-run serially only the transitions. Same decision rule (`any ok → dnf` blocks the flip), roughly one-sixth the wall.

**Explicitly cut from this plan:** v1's Task 3 (single-class gate parity). It is a real API absurdity (3.50 s for one class vs 2.52 s for all) but is worth **zero** DNFs, and v1's version would have introduced unsoundness. Re-scope it separately later as gate parity **with guard parity** — porting the `abox_verdict()` read that `classify` pairs with those gates — and with the consistency entry point excluded.

---

## Execution order and stopping rules

**A → B → C → D**, with E only if time remains. A and B are the point; C and D are bounded and must not expand.

- **A/B:** if no cause is found, write the dead end and stop. Do **not** propose a fix without one — that is the pattern that produced this project's NO-GO chain.
- **A/B:** any deletion- or strip-based probe is an **upper bound only**, and must report the cut arm's cost profile. A non-rescue is uninformative unless cost also improved.
- **C:** any ON-arm conversion timeout, or any FP against Konclude ∪ HermiT, blocks that flag. Do not override on correctness grounds — the v0.3.29 DNFs came from exactly that trade.
- **D:** if the 1 ms arm still detects inconsistency, that is a finding to chase, not an assertion to weaken.
- **Any task:** if a premise does not reproduce, stop and report it. Four premises were refuted in the previous block and each saved more than it cost.

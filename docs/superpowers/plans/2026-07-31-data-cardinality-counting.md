> **REJECTED 2026-07-31 — do not execute.** Its spec was rejected by two independent reviews; see
> `docs/superpowers/specs/2026-07-31-data-cardinality-counting-design.md` § Rejection. In short: the
> premise (three ontologies DNF) is false — they complete in 33–50 s; Half A already exists as
> `data_axioms.rs:119`; the route enumeration missed two routes, each with a counterexample; and the
> targeted axioms are provably inert on all three targets.
>
> **The stopping rule failed exactly as its own Self-Review feared.** Task 1 Step 4 said "if they
> still DNF with the data channel disabled, stop" — they don't DNF *with it enabled*, so the rule
> passed while the design was aimed at inert axioms. A stopping rule must bind to an outcome
> ("suppression changes ≥1 answer"), not to the absence of a failure. Retained as an example of a
> plan whose gate was decoration.

# Data-Cardinality Counting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decide `DataMaxCardinality(≤n, p)` versus k distinct asserted values **arithmetically** (O(k)) instead of materialising C(k,2) `DisjointClasses(DKey, DKey)` axioms (O(k²)) — 6.6 M axioms on `ore_ont_16632`.

**Architecture:** Two halves. **Half A** adds an ABox counting check (additive, cannot cause an FP). **Half B** suppresses the value×value disjointness quadrant that Half A subsumes (subtractive, cannot cause an FP; its risk is entirely *completeness*). Half A alone changes no axiom count and therefore **does not fix the DNF** — Half B is the fix, and Half B is where all the risk is.

**Tech Stack:** Rust (edition 2024), `cargo test` / `clippy` / `fmt`.

**Spec:** `docs/superpowers/specs/2026-07-31-data-cardinality-counting-design.md` — read its **§ Open questions** first. Five are unresolved and one (#5) could make part of Half A redundant.

## Global Constraints

- **Toolchain.** `cargo` is NOT on `PATH`; prefix every command with
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and run `RUSTUP_TOOLCHAIN=stable cargo …`.
- **`owl-dl-py` does not build in this sandbox.** Use `--exclude owl-dl-py`.
- **Warnings are errors**; clippy `pedantic` workspace-wide; `clippy::doc_markdown` is ON, so a bare `DKey`/`ABox`/`TBox` in a doc comment is an **error** — backtick it. This has bitten five tasks.
- **Check real exit codes.** Never `cargo … | tail` then read `$?`.
- **FP=0 is absolute.** Both halves are FP-safe by construction (additive / subtractive). Every review finding will therefore be about **completeness**. Treat a fixture flip as a real regression, never as a test to adjust.
- **Follow `owl-reasoner-harness`'s `skills/corpus-measurement`** for any measurement: pin the binary, verify the marker is *in* the binary, smoke-test on 3 known cases, state exclusions.
- The addressable set is **3 measured ontologies**. If Task 1 shortens the suppression condition, that number only goes down, and **not building it is an acceptable outcome** — say so rather than proceeding.

---

## Task 1: Close the spec's open questions BEFORE writing any feature code

The spec cannot be implemented as written. This task is investigation whose deliverable is a
decision, and it is legitimate for that decision to be "do not build Half B".

**Files:** none modified. Findings appended to the spec.

- [ ] **Step 1: Does Half A already exist? (spec Q5)**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
grep -rn "fn card_sat" crates/owl-dl-datatypes/src/
grep -rn "data_counting_classes\|dkey_ranges" crates/owl-dl-reasoner/src/lib.rs | head
grep -rn "fn concrete_domain_clash" -A 40 crates/owl-dl-tableau/src/lib.rs | head -50
grep -rn "DataMaxCardinality" crates/owl-dl-core/src/data_axioms.rs
```

Answer in writing: does any existing path already decide "k distinct values versus `≤n`"? The
session that wrote this spec twice found a "new" defect that was an unfixed sibling of existing
machinery — assume that until disproved. If `concrete_domain_clash` + `card_sat` already do this
for **class-level** cardinality, Half A shrinks to wiring the **ABox** case into it.

- [ ] **Step 2: Enumerate the real consumers of a value×value told-disjoint entry (spec Q1)**

```bash
grep -rn "are_told_disjoint" crates/ --include=*.rs | grep -v test
```

For each caller, state whether a *value×value DKey* pair can reach it and what is lost if the
entry is absent. Known callers: `approx_saturation.rs:90,95`, `abox_check.rs` (3 sites),
`disjointness.rs:85`. Deliverable: a route table with a verdict per row.

- [ ] **Step 3: Decide whether spec condition 3 is soundly checkable (spec Q2)**

Condition 3 requires that no value can reach a node except via an ABox assertion on a named
individual. Determine whether that is decidable cheaply given role chains, inverse properties and
`SameIndividual`. If not, propose the coarser gate the spec suggests ("the TBox mentions no data
range in any concept position") and check whether Group B satisfies it:

```bash
P=/data/dumontier/ore-run/pool_sample/files
for n in 16632 11126 10425; do
  printf "%s: DataSome=%s DataAll=%s DataHasValue=%s\n" "$n" \
    "$(grep -c DataSomeValuesFrom $P/ore_ont_$n.owl)" \
    "$(grep -c DataAllValuesFrom $P/ore_ont_$n.owl)" \
    "$(grep -c DataHasValue $P/ore_ont_$n.owl)"
done
```

- [ ] **Step 4: Establish the ceiling on the payoff BEFORE building**

Conversion is 1.3–1.8 GB of these ontologies' 5.9–7.6 GB. Measure what happens when the axioms are
absent entirely — `RUSTDL_DATA_PROPERTIES=0` is the upper bound on any suppression:

```bash
S=<scratchpad>; P=/data/dumontier/ore-run/pool_sample/files
cp target/release/rustdl "$S/rustdl-ceiling"          # pin, per the skill
for n in 16632 11126 10425; do
  for e in "" "RUSTDL_DATA_PROPERTIES=0"; do
    printf "  %s %-26s " "$n" "${e:-default}"
    /usr/bin/time -f "%e s %M kB" env $e RAYON_NUM_THREADS=1 timeout 300 \
      "$S/rustdl-ceiling" classify "$P/ore_ont_$n.owl" > /dev/null
  done
done
```

**If they still DNF with the whole data channel disabled, Half B cannot fix them and this plan
should stop here.** That is the single most important number in this task; get it first if time is
short. Note `DATA_PROPERTIES=0` is an *upper bound*, not the lever's value — report the fraction
achieved later, never the bound (a bound was cited as a result earlier in this arc and retracted).

- [ ] **Step 5: Record the decision in the spec and commit**

Append a `## Task 1 findings` section answering Q1–Q5 with measurements, and a GO / NO-GO. If
NO-GO, say so plainly and stop — the spec's own § "What this does not claim" already licenses that.

```bash
git add docs/superpowers/specs/2026-07-31-data-cardinality-counting-design.md
git commit -m "spec: Task 1 findings -- resolve the five open questions, GO/NO-GO on Half B"
```

---

### Task 2: Counting canaries (negatives-first), failing

Only start if Task 1 is GO. These pin the arithmetic before any engine change, and they are the
non-vacuity net for Half A.

**Files:** Create `crates/owl-dl-reasoner/tests/data_cardinality_counting.rs`

- [ ] **Step 1: Write the canary set**

Six cases, boundary-first. `n` is the `≤n` bound, `k` the count of distinct asserted values:

| case | expectation |
|---|---|
| `≤1`, k=2 | **inconsistent** |
| `≤2`, k=2 | **consistent** (boundary — must not over-fire) |
| `≤2`, k=3 | **inconsistent** |
| `≤1`, k=2 via a **sub**-property of the bounded property | **inconsistent** |
| `≤1`, k=2 split across two individuals joined by `SameIndividual` | **inconsistent** |
| `≤1`, k=2 where the two literals are **equal** | **consistent** (distinctness, not count) |

Use the fixture-loading idiom from `crates/owl-dl-reasoner/tests/dkey_collapse_broadcast.rs`
(relative `tests/fixtures/...` const + `read_ofn`), one `#[test]` per case, fixture stem in every
assertion message.

- [ ] **Step 2: Run against unmodified `main` and RECORD which already pass**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test data_cardinality_counting > /tmp/t.log 2>&1
echo "rc=$?"; grep -E "^test |test result" /tmp/t.log
```

Some will already pass via the existing pairwise-disjointness route — **that is expected and is the
point**: those are the behaviours Half B must not break. Record the pass/fail split; it is the
before-picture for the sabotage gate.

- [ ] **Step 3: Commit**

```bash
git add crates/owl-dl-reasoner/tests/data_cardinality_counting.rs
git commit -m "test(data-card): counting canaries, boundary-first, with the pre-change pass/fail split recorded"
```

---

### Task 3: Half A — the ABox counting check

**Files:** Modify `crates/owl-dl-reasoner/src/abox_check.rs` (new pattern beside P1–P9).

- [ ] **Step 1: Implement P10 using only the eight `AboxCheckInputs` fields**

Do **not** widen `AboxCheckInputs` — its narrowness is load-bearing for the classify fast path
(`docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md`). Per `(individual, data
property)`: count **distinct** DKey values (folding `SameIndividual` via the existing union-find,
and sub-properties via `hierarchy`), find the tightest `≤n` from the individual's types, and return
`Inconsistent` when `k > n`.

- [ ] **Step 2: Canaries pass; the whole suite still green**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test data_cardinality_counting 2>&1 | grep -E "^test |test result"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner 2>&1 | grep -cE "test result: ok"
```

All six canaries must pass. Half A is additive, so **any** pre-existing test that changes is a bug.

- [ ] **Step 3: Non-vacuity — disable P10, confirm the canaries that only it can satisfy fail**

- [ ] **Step 4: fmt, clippy, commit**

---

### Task 4: Half B — suppression, exactly as Task 1 scoped it

**Files:** Modify `crates/owl-dl-core/src/convert.rs` (`seed_disjoint_bucket` emission), plus the flag.

- [ ] **Step 1: Implement the Task-1-approved condition, per-pair not per-component**

Reuse `broadcast_in` / `collapse_comps` from v0.4.6 — add no new classification. Gate on
`RUSTDL_DATA_CARD_COUNTING`, default **OFF**. Per-pair granularity is mandatory: the
collapse/broadcast design's R1 records that a per-component drop destroyed the D11b flagship clash.

- [ ] **Step 2: The 11 preserved fixtures + 3 canaries + `dkey_nominal_range_merge`, flag ON**

All must hold. A flip here is a lost clash.

- [ ] **Step 3: Non-vacuity by sabotage** — force unconditional suppression, confirm fixtures fail, revert.

- [ ] **Step 4: Flag switches, on the discriminating cases**

`16632` concept_rules must fall substantially with the flag ON and be **unchanged** with it OFF.
Use pinned binaries; `9347` cannot discriminate this lever (no `DataMaxCardinality`), so use
`16632` and a `≤n`-free control.

- [ ] **Step 5: fmt, clippy, full suite, commit**

---

### Task 5: Gates and the honest write-up

- [ ] **Step 1: FP=0 net**, flag ON — 22/0, closures exact (galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16).
- [ ] **Step 2: Flag-OFF byte-identity** on wine/family/pizza/ro/alehif-test.
- [ ] **Step 3: Recovery, pinned** — `16632`/`11126`/`10425`: concept_rules, wall, RSS, and **whether any completes.** Report the fraction of the Task-1 ceiling achieved, never the ceiling.
- [ ] **Step 4: Full-pool ON-vs-OFF answer identity** via `owl-reasoner-harness compare`, so the result carries a provenance header.
- [ ] **Step 5: Record results; flip the default only if all gates pass.** If the three still DNF, say so in the headline — the spec already warns that ~5 GB of their footprint is post-conversion.

---

## Self-Review

**1. Spec coverage.** Q1–Q5 → Task 1; Half A → Task 3; Half B → Task 4; every spec gate → Tasks 4–5.
Spec gate 3 (counting canaries) is Task 2, deliberately before both halves.

**2. Placeholder scan.** Task 1 has no code because it is investigation; its steps are literal
commands with a stated deliverable. Tasks 3 and 4 name the files and the constraint (do not widen
`AboxCheckInputs`; per-pair not per-component) but do **not** contain the implementation, because
Task 1 may change it — writing it now would be a placeholder pretending to be a plan.

**3. Type consistency.** `AboxCheckInputs` is used as it exists on `main` (8 fields) and explicitly
not extended. `broadcast_in: HashMap<ClassId, Vec<usize>>` and `collapse_comps: HashSet<usize>`
match the v0.4.6 signatures. `RUSTDL_DATA_CARD_COUNTING` appears once, default OFF.

**Known weakness of this plan:** Task 1 Step 4 may kill it. That is deliberate — the addressable
set is 3 ontologies, ~5 GB of their footprint is outside conversion, and a plan that cannot be
stopped by its own first measurement is not a plan.

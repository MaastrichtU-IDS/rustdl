# DKey Collapse-vs-Broadcast Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop seeding `DisjointClasses(DKey_a, DKey_b)` for pairs where both keys are *value-only* in a role component that has no COLLAPSE source — because nothing can put two values in one node label there.

**Architecture:** The classification is **already implemented and validated on `main`** in report-only mode (`RUSTDL_DKEY_SPLIT_STATS`, merge `01e9925`): `collapse_comps`, `broadcast_in`, and `filler_is_pure_dkey` in `crates/owl-dl-core/src/convert.rs`. This plan does one thing — make the already-computed decision *act* instead of only counting — behind `RUSTDL_DKEY_COLLAPSE_SPLIT`. That is why it is short: the risky reasoning is done and measured, and what remains is flipping a counter into a `continue`.

**Tech Stack:** Rust (edition 2024), `cargo test` / `clippy` / `fmt`.

**Spec:** `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` — read § "What adversarial review refuted" (R1–R6) before writing code. Those six requirements are the design.

## Global Constraints

- **Toolchain.** `cargo` is NOT on `PATH`; `rust-toolchain.toml` pins a `cargo`-less 1.95.0. Prefix every command with
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and run `RUSTUP_TOOLCHAIN=stable cargo …`.
- **`owl-dl-py` does not build in this sandbox** (missing Python headers, pre-existing). Use `--exclude owl-dl-py`.
- **Warnings are errors.** CI sets `RUSTFLAGS: -D warnings`; clippy `pedantic` workspace-wide. `clippy::doc_markdown` is ON — a bare `DKey`, `ABox`, `TBox` in a `///` or `//!` comment is an **error**; backtick it. `clippy::needless_range_loop` and `match_same_arms` have both already bitten this file.
- **Check real exit codes.** Never `cargo … | tail` then read `$?` — that reads `tail`'s status. Redirect to a file, then `echo $?`.
- **FP=0 is absolute.** This change only ever REMOVES axioms, so it cannot create a false positive. The exposure is a lost clash (a MISS). Treat any fixture flip as a real completeness regression, never as a test to adjust.
- **The curated corpus cannot validate this area** — `datatype_value_membership.rs` says so itself. The FP=0 net shows inertness; the 11 fixtures + 3 canaries are the real gate.
- **Never rebuild a binary while a measurement reads it, and pin each build immediately.** Two measurement failures this session came from reusing `target/release`. Also: `ore_ont_9347` **cannot** discriminate this lever (it has zero ranges) — use `5368` (must stay 18,620,251) and `7607`.

---

## File Structure

| file | responsibility |
|---|---|
| `crates/owl-dl-core/src/convert.rs` | `dkey_collapse_split_enabled()`; the `continue` in `seed_disjoint_bucket`'s group loop |
| `crates/owl-dl-reasoner/tests/dkey_collapse_broadcast.rs` | **new** — drives the 11 preserved fixtures as a test |

---

### Task 1: Wire the 11 preserved fixtures into a test

They exist as `.ofn` files with a README of measured verdicts but **nothing executes them**. Do this first: it is the gate for Task 2, and it must pass on unmodified `main`.

**Files:**
- Create: `crates/owl-dl-reasoner/tests/dkey_collapse_broadcast.rs`
- Read (do not modify): `crates/owl-dl-reasoner/tests/fixtures/dkey_collapse_broadcast/README.md`

**Interfaces:**
- Consumes: `owl_dl_reasoner::{classify, is_consistent}`; fixture files by path.
- Produces: nothing consumed later.

- [ ] **Step 1: Read the README and the fixtures**

```bash
cat crates/owl-dl-reasoner/tests/fixtures/dkey_collapse_broadcast/README.md
```

It gives, per fixture, the **measured** verdict on `ef41128` and which requirement it guards. Note the
distinction it flags: "class unsat" needs `classify` + the unsatisfiable list, "inconsistent" needs
`is_consistent`. Conflating them produced a false alarm on five fixtures during spec preparation.

- [ ] **Step 2: Write the test**

Load each fixture from disk (they are OFN). Follow the loading pattern in an existing integration test
that reads a fixture file — grep for `ontologies/` or `fixtures/` in `crates/owl-dl-reasoner/tests/`
and copy the idiom rather than inventing one.

```rust
//! Drives the preserved adversarial-review fixtures for the DKey collapse/broadcast
//! split. See the README beside the fixtures for what each one guards, and
//! `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` R1-R6.
//!
//! These pin CONSUMABLE clashes: every one must keep its verdict when the split is
//! enabled. A flip is a lost clash, i.e. a completeness regression.

#![allow(clippy::unwrap_used)]

const DIR: &str = "tests/fixtures/dkey_collapse_broadcast";

/// (fixture stem, expect_inconsistent, expect_unsat_class_count)
const CASES: &[(&str, bool, usize)] = &[
    ("two-disjoint-ranges", false, 1),
    ("range-vs-value-d11b-flagship", false, 1),
    ("two-ranges-class-unsat", false, 1),
    ("exists-plus-two-forall-dataoneof", false, 1),
    ("range-vs-datahasvalue", true, 0),
    ("range-on-super-value-on-sub", true, 0),
    ("functional-super-values-on-sub", true, 0),
    ("functional-3-level", true, 0),
    ("downward-closure-two-subs", false, 1),
    // NEGATIVE control: must stay satisfiable. If this ever goes unsat something
    // over-approximates.
    ("NEGATIVE-functional-sub-values-on-super", false, 0),
    // PRE-EXISTING miss, pinned so it is not mistaken for a regression: forall on a
    // super + conflicting value on a sub is missed, while the ObjectPropertyRange
    // form (range-on-super-value-on-sub) works.
    ("KNOWN-MISS-forall-super-value-sub", false, 0),
];
```

For each case: parse the file, and
- if `expect_inconsistent`, assert `is_consistent` is `Ok(false)`;
- else assert `is_consistent` is `Ok(true)` **and** the classification's unsatisfiable-class count
  equals `expect_unsat_class_count`.

Put the fixture stem in every assertion message — with 11 cases in one test, a bare failure is
useless. Prefer one `#[test]` per case (a loop that stops at the first failure hides the rest).

- [ ] **Step 3: Run against unmodified `main` — all must PASS**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test dkey_collapse_broadcast > /tmp/t1.log 2>&1
echo "rc=$?"; grep -E "^test |test result" /tmp/t1.log
```

Expected: 11 passed. If any fails, the README's expectation or your loading is wrong — reconcile
against the README's measured values before continuing. Do NOT weaken an expectation to get green.

- [ ] **Step 4: fmt, clippy, commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check > /dev/null 2>&1; echo "fmt rc=$?"
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings > /tmp/c1.log 2>&1; echo "clippy rc=$?"
git add crates/owl-dl-reasoner/tests/dkey_collapse_broadcast.rs
git commit -m "test(dkey): execute the 11 preserved collapse/broadcast fixtures

They existed as .ofn files with a README of measured verdicts but nothing ran them.
Each pins a CONSUMABLE clash that the collapse/broadcast split must not lose,
including the D11b flagship, plus a negative control that must stay satisfiable and a
pinned pre-existing miss. All 11 pass on unmodified main."
```

---

### Task 2: Make the decision act, behind a flag

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` — add `dkey_collapse_split_enabled()` beside `dkey_split_stats_enabled()`; add the `continue` inside `seed_disjoint_bucket`'s `try_emit`.

**Interfaces:**
- Consumes: `comp.collapse_comps`, `comp.broadcast_in`, and the `in_comp: Option<usize>` argument — **all three already exist and are validated** (merge `01e9925`).
- Produces: no new public surface.

- [ ] **Step 1: Add the flag reader**

Directly below `dkey_split_stats_enabled`:

```rust
/// Collapse/broadcast split (2026-07-30). **Default OFF** until the gates in
/// `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` pass; set
/// `RUSTDL_DKEY_COLLAPSE_SPLIT=1` to enable.
///
/// Omits a `DKey`-disjointness pair when its component has no COLLAPSE role and BOTH
/// keys are value-only there: a BROADCAST source puts one key on EVERY successor, so a
/// value meets the broadcast key but never another value. Subtractive only — it can
/// never create a false positive; the exposure is a lost clash.
fn dkey_collapse_split_enabled() -> bool {
    std::env::var("RUSTDL_DKEY_COLLAPSE_SPLIT").is_ok_and(|v| v != "0")
}
```

- [ ] **Step 2: Act on the decision**

In `seed_disjoint_bucket`, `try_emit` currently computes the droppable condition only to bump
`DKEY_SPLIT_WOULD_DROP`. Hoist that condition so it can also gate emission. Read the existing block
first, then restructure to:

```rust
        // Would the collapse/broadcast split drop this pair? Only when the component
        // has NO collapse role AND BOTH keys are value-only there. `in_comp` is
        // `None` for the unanchored `global` pairings, which are unconditional
        // (spec R6) and therefore never droppable.
        let droppable = in_comp.is_some_and(|c| {
            let value_only = |cid: &ClassId| {
                !comp.broadcast_in.get(cid).is_some_and(|v| v.contains(&c))
            };
            !comp.collapse_comps.contains(&c) && value_only(a_cid) && value_only(b_cid)
        });
        if stats {
            use std::sync::atomic::Ordering;
            DKEY_SPLIT_TOTAL.fetch_add(1, Ordering::Relaxed);
            if droppable {
                DKEY_SPLIT_WOULD_DROP.fetch_add(1, Ordering::Relaxed);
            }
        }
        if split && droppable {
            return;
        }
```

with `let split = dkey_collapse_split_enabled();` beside the existing `let stats = …`.

Note `try_emit` returns `()` and is called for its effect, so `return` (not `continue`) is the right
early exit — it sits inside the closure, not the loop.

**Do not change** `dkey_components`, the union-find, or step (d) — spec R3 forbids it, and the
classification is already validated as-is.

- [ ] **Step 3: Confirm the flag switches, on the discriminating cases**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli > /dev/null 2>&1
P=/data/dumontier/ore-run/pool_sample/files
for n in 7607 5368; do
  printf "  %s OFF=" $n; timeout 900 ./target/release/rustdl tbox-stats $P/ore_ont_$n.owl 2>/dev/null | awk '/concept_rules/{print $3}'
  printf "  %s ON =" $n; RUSTDL_DKEY_COLLAPSE_SPLIT=1 timeout 900 ./target/release/rustdl tbox-stats $P/ore_ont_$n.owl 2>/dev/null | awk '/concept_rules/{print $3}'
done
```

Expected — these are the predictions the report-only study made, so they are a real test of it:
- `7607`: OFF 5,419,609 → ON **≈9,515** (5,410,094 of its pairs are droppable, i.e. ~100%)
- `5368`: OFF 18,620,251 → ON **18,620,251 unchanged** (0 droppable — the negative control)

If `5368` moves at all, the split is dropping pairs it must keep: **stop and report**.

- [ ] **Step 4: The fixtures and canaries, flag ON**

```bash
RUSTDL_DKEY_COLLAPSE_SPLIT=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test dkey_collapse_broadcast > /tmp/t2.log 2>&1; echo "rc=$?"
grep -E "^test |test result" /tmp/t2.log
RUSTDL_DKEY_COLLAPSE_SPLIT=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test datatype_value_membership > /tmp/t3.log 2>&1; echo "rc=$?"
grep -E "test result" /tmp/t3.log
RUSTDL_DKEY_COLLAPSE_SPLIT=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test dkey_nominal_range_merge > /tmp/t4.log 2>&1; echo "rc=$?"
grep -E "test result" /tmp/t4.log
```

All must pass with the flag ON. The report-only study predicted `would_drop = 0` for all 11 fixtures,
so any failure here means the acting path diverges from the counting path — a bug in Step 2, not in
the design.

- [ ] **Step 5: Non-vacuity — prove the fixtures guard the emission**

Temporarily force `droppable` to ignore the classification (e.g. `let droppable = in_comp.is_some();`),
rebuild, and re-run the fixture test with the flag ON. **Expect failures.** Then revert with
`git checkout -- crates/owl-dl-core/src/convert.rs` and rebuild.

If the fixtures still pass under that sabotage, they are not guarding emission and Task 1 must be
fixed before proceeding. This step is mandatory: a gate in this exact area already passed while
guarding nothing.

- [ ] **Step 6: fmt, clippy, full suite, commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check > /dev/null 2>&1; echo "fmt rc=$?"
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --exclude owl-dl-py --all-targets --all-features -- -D warnings > /tmp/c2.log 2>&1; echo "clippy rc=$?"
RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py > /tmp/ws.log 2>&1; echo "tests rc=$?"
grep -cE "test result: ok" /tmp/ws.log
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): act on the collapse/broadcast split, default OFF

RUSTDL_DKEY_COLLAPSE_SPLIT=1 omits a DKey-disjointness pair when its component has no
COLLAPSE role and BOTH keys are value-only there. The classification was already
computed and validated in report-only mode (01e9925); this only lets it act.

Verified the flag switches on the discriminating cases: 7607 5,419,609 -> ~9,515,
5368 18,620,251 unchanged (its 15 FunctionalDataProperty are genuine collapse). The
11 fixtures, the 3 canaries and dkey_nominal_range_merge all pass with the flag ON,
and are non-vacuous (sabotaging the condition fails them)."
```

---

### Task 3: Gates, then flip the default

**Files:** `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` (results); `crates/owl-dl-core/src/convert.rs` (default flip, last step only).

- [ ] **Step 1: FP=0 net, flag ON**

```bash
RUSTDL_DKEY_COLLAPSE_SPLIT=1 ./scripts/run-soundness-diff.sh > /tmp/fp0.log 2>&1; echo "rc=$?"
grep -E '^\[fp0\]|test result:' /tmp/fp0.log | tail -25
```

Expected 22 passed / 0 failed with closures galen 27997, notgalen 32739, sio 8904, ore-10908 6001,
wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16. Record that this shows
**inertness** — the curated corpus has no consumed DKey disjointness.

- [ ] **Step 2: Flag-OFF byte-identity**

With the flag unset, `classify` output must be byte-identical to pre-change `main` on
`ontologies/real/{wine,family,pizza,ro}.ofn` and `ontologies/external/alehif-test.ofn`. Proves the
OFF path is untouched.

- [ ] **Step 3: Recovery on the beneficiaries, from PINNED binaries**

Build once, `cp target/release/rustdl` to a uniquely named path, and measure from that copy — do not
rebuild mid-measurement. Verify the pin distinguishes the configurations (`5368` must read
18,620,251) before trusting any row.

Measure `concept_rules`, wall and RSS, OFF vs ON, for the ≥100k beneficiaries:
`7607 1685 12182 4410 7345 8989 15288 13052 9899 6132 5548 443 12174 1672 4049`,
plus `5368`, `2504`, `4141` as negative controls (must be ~unchanged).

Report **per ontology**, and state explicitly which of the four DNFs (`7607`, `1685`, `4410`, `5548`)
now complete — that is this lever's headline claim and the only part that changes a user-visible
outcome.

- [ ] **Step 4: `ore_ont_5548` — the Bucket B probe**

Worth calling out separately. `5548` is label-cache-build-bound and recovered 0/5 across five weeks of
matcher/search work; 55% of its 530,605 pairs are droppable. Report whether it completes, and if it
still DNFs, whether its wall/RSS moved at all. **Either answer is valuable**: it tells us whether
Bucket B's cost is partly axiom volume, which no profiling has established.

- [ ] **Step 5: Full-pool ON-vs-OFF answer-identity sweep**

Beyond the curated corpus, sweep the ORE pool comparing classify output ON vs OFF and report **any**
ontology whose answers differ. The change should be answer-identical everywhere (it removes only
unusable axioms); a difference is either a lost clash or a recovered one, and both need explanation.
State the timeout used **and the exclusions** — a per-item timeout is not a neutral sampler, and that
artifact already invalidated one figure in this arc.

- [ ] **Step 6: Record results, then flip the default**

Append a `## Measured results` section to the spec: gate outcomes, per-ontology recovery, the `5548`
answer, and the sweep's diff count. Then flip `dkey_collapse_split_enabled` to default ON
(`map_or(true, |v| v != "0")`), re-run Steps 1–2, and commit.

```bash
git add docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): default the collapse/broadcast split ON, with measured results"
```

---

## Self-Review

**1. Spec coverage.**

| spec item | task |
|---|---|
| R1 per-pair drop | Task 2 Step 2 (`droppable` is per-pair; `global` never droppable) |
| R2 occurrence-position classification | already on `main` (`broadcast_in`), exercised by Task 1's `exists-plus-two-forall-dataoneof` |
| R3 union-find untouched | Task 2 Step 2's explicit "do not change" |
| R4 collapse closed downward | already on `main`, exercised by `functional-super-values-on-sub` / `functional-3-level` / `downward-closure-two-subs` |
| R5 pure-`DKey` filler test | already on `main` (`filler_is_pure_dkey`), exercised by `dkey_nominal_range_merge` |
| R6 `unanchored` pairing preserved | Task 2 Step 2 — `in_comp: None` ⇒ never droppable |
| gate: fixtures | Task 1 + Task 2 Step 4 |
| gate: non-vacuity | Task 2 Step 5 |
| gate: FP=0 | Task 3 Step 1 |
| gate: flag-OFF identity | Task 3 Step 2 |
| gate: recovery, pinned | Task 3 Step 3 |
| gate: population sweep | Task 3 Step 5 |

R6's deeper half — restoring `global × anchored` for keys the *shipped merging gate* drops from
`components` — is **not** in this plan. It is a defect in the predecessor, currently unreachable (no
lowering emits a top-level bare `DKey`), and folding it in would conflate two changes. It stays
recorded in the spec as its own item.

**2. Placeholder scan.** No TBDs. Task 1 Step 2 deliberately says to copy an existing fixture-loading
idiom rather than specifying one, because inventing a second loader is worse than matching the file's
neighbours — the `CASES` table it must drive is given in full. Task 2 Step 2 gives the exact code.

**3. Type consistency.** `dkey_collapse_split_enabled() -> bool` matches its one call site.
`droppable: bool` is computed once and used by both the counter and the gate, so the counting and
acting paths cannot diverge — which is what makes the report-only validation transfer. `in_comp:
Option<usize>`, `collapse_comps: HashSet<usize>`, `broadcast_in: HashMap<ClassId, Vec<usize>>` all
match the signatures merged in `01e9925`.

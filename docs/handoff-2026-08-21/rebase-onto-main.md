# DKey id-aliasing fix — rebase onto `main` + re-audit

Branch **`fix/dkey-id-aliasing-on-main`**, based on `origin/main` (`b796bec`).
Final tip **`60b2e22`**. All 4 commits replayed; one additional commit carries the
Part-2 findings.

```
60b2e22 fix(classify): finish the probe conversion; one NEW conflation in main's code   <- Part 2
6bd3904 docs: DKey aliasing resolution — corrected trigger, two defects, follow-ups
5af44c4 test(classify): corpus canary that actually runs in CI; retire unsatisfiable_bitset
ac721de fix(classify): unsat projection read a ClassId-indexed bitset with a report position
9cc380c fix(classify): report positions are not ClassIds — kills classify() false positives
b796bec (origin/main)
```

---

## Part 1 — conflict resolution

**6 conflict hunks across 2 commits, all mechanical. Nothing semantic; no restructuring
by main that the fix depends on.** Plus one silent non-conflict that the compiler caught
(below), which is the only part of Part 1 that needed judgement.

### Commit `132dd21` — 4 hunks, all in `classify.rs`

| # | site | main's side | our side | resolution |
|---|------|-------------|----------|------------|
| 1 | `classify_saturation_only_internal` | added `analyze_fragment(internal)` arg | `&classes` → `&reported` | took **both** |
| 2 | `classify_internal_with_timeout_impl` fast-path return | same new arg | same | took **both** |
| 3 | `classify_top_down_internal_impl` preamble | added `crate::rss_probe::probe("entry")` | `reportable_class_iris` → `ReportedClasses::collect` | took **both** |
| 4 | `classify_top_down_internal_impl` fast-path return | added `precheck_ms` / `saturate_ms` / `unattributed_wall_ms` stats accounting around the call | same arg change | kept main's whole stats block, changed only the argument inside it |

The merge had already resolved `classify_pure_el`'s **definition** cleanly, carrying both
`reported: &ReportedClasses` (ours) and `fragment: FragmentClassification` (main's) — which
is what makes "take both" the correct reading at every call site rather than a guess.

**The hunk git did NOT report.** Main added a **fifth** `classify_pure_el` caller,
`classify_prep_timeout` (the `RUSTDL_PREP_DEADLINE` degradation path, PRs #61/#62), in a
region our patch never touched — so it merged silently and then failed to compile. It took
`classes: &[String]`; threaded `reported: &ReportedClasses` through it and both of its call
sites in `classify_top_down_internal_impl`, plus `!classes.is_empty()` →
`!reported.iris().is_empty()`. **Mechanical, but worth flagging: a textually-clean merge is
not evidence that a signature change is fully propagated. Only the type checker found it.**

### Commit `954bc7e` — 2 hunks

| # | file | main's side | our side | resolution |
|---|------|-------------|----------|------------|
| 5 | `classify.rs`, `classify_pure_el` Pass 1 | added the `globally_inconsistent()`/`top_is_unsat()` inconsistency block above Pass 1 | replaced the bitset probe with `closure.is_unsatisfiable(reported.class_id(i))` | kept main's block verbatim; kept our Pass 1 + its comment; dropped only main's now-false comment describing the removed bitset |
| 6 | `realize.rs`, `realize_tableau_internal` | bounded the classify step by `pair_deadline_ms` | added the "full class-id space, no DKey filter here" comment | took **both** |

Commits `822a8d5` and `0cc64ac` applied clean.

---

## Part 2 — re-audit of main's 2,421 new lines

### THE SCANNER HAD GONE BLIND AND WAS PASSING VACUOUSLY

This is the headline finding, and it is why the two defects below survived.

`report_positions_are_never_cast_to_class_ids` isolated production code with
`src.split_once("\n#[cfg(test)]")`. That stops at the **first** test module. Main has an
inline `#[cfg(test)]` at **line 1039**, so the scan covered **1,038 of 6,800 lines** — 15%
of the file — and reported green. Every offender below sat in plain sight beneath the cut.

Rewritten to skip `#[cfg(test)]` modules by brace matching rather than truncating, scanning
the whole file. Three hardenings, because a guard that can fail silently is the actual bug:

* it now **asserts a coverage floor** (≥60% of lines scanned) — a future skip regression
  fails instead of passing empty;
* an **indented** `#[cfg(test)]` (which its top-level-`}` terminator cannot bound) is a
  loud failure telling the maintainer to teach it the nested case, not a silent miss;
* verified by sabotage: reintroducing the escalation-probe defect at line 3588 now fails
  it. **The old scanner could not see line 3588 at all.**

### Offender 1 — `probe_says_inconsistent`: the CONSUMER END of this branch's own producer conversion. Fixed.

`crates/owl-dl-reasoner/src/classify.rs`, layer (1) of the gated consistency probe:

```rust
&& unsatisfiable_idxs.contains(&(c.index() as usize))   // c: ClassId; the set is REPORT-indexed
```

> **RETRACTED (2026-08-21, verified):** this section previously read "LIVE AT DEFAULT, AND A
> FALSE POSITIVE" **on `main`**. That is wrong and is withdrawn. On stock `main` all three
> producers of `unsatisfiable_idxs` store RAW `ClassId`s — `classify.rs:1291` mints
> `ClassId::new(i)`, `:1507` reads the `ClassId`-indexed `unsatisfiable_bitset()`, `:3647`
> does the same — so main's `contains(&(c.index() as usize))` is **exact**, and the `< n` clip
> can only cause a miss, never a false positive. **`origin/main` has no live false positive
> from this site.** See `probe-fix-was-wrong.md` for the isolated-on-main experiment.

`RUSTDL_CLASSIFY_CONSISTENCY_PROBE` **defaults ON** (`is_none_or(|v| v != "0")`), so once the
producers move, this is not latent. And they do move: this branch's `9cc380c` and `ac721de`
convert all three producers to report positions (`reported.class_id(i)` over `0..n`). At
`5af44c4` — the parent of this commit — the producers are report-indexed while the probe still
compares `c.index()`, and *that* is the false positive. With a DKey below a user class, a
satisfiable asserted class reads a stranger's ⊥ bit; the probe returns `true`; classify returns
`classify_inconsistent`, which marks **every** class unsatisfiable; `Classification::entails`
then short-circuits `⊥ ⊑ *` for all of them. **Every pair in the ontology becomes an entailment
that does not hold.** Same severity and same mechanism as defect (b), reached by a third
spelling — but reached only *on this branch*, not on `main`.

**The two ends are therefore inseparable.** This hunk applied alone to `main` *introduces* the
defect it appears to fix; `9cc380c`/`ac721de` applied without it leave the defect live. Neither
half may be cherry-picked.

Reproduced at `5af44c4`, not inferred — new fixture `CONSISTENCY_PROBE_BODY`, id layout
`DKey=0, Aaa=1/pos0, Ccc=2/pos1, Fff=3/pos2, Eee=4/pos3(⊥), Bbb=5/pos4, Ddd=6/pos5`, so
`Fff`'s ClassId (3) collides with `Eee`'s report position (3):

* **before the fix:** `unsat = [Aaa, Bbb, Ccc, Ddd, Eee, Fff]` — all six
* **after:** `unsat = [Eee]` — correct

Fixed by threading `reported` into `probe_says_inconsistent` and going through
`ReportedClasses::report_pos`.

### Offender 2 — label-cache escalation probe. Latent. Fixed.

`RUSTDL_LABEL_CACHE_PROBE` (default OFF, added 2026-08-19). Its strided scan minted
`ClassId::new(i)` from report positions:

```rust
let i = (k * stride).min(n - 1);                 // n = reported.len(), i is a REPORT position
let id = owl_dl_core::ClassId::new(...i...);     // ...used as a ClassId
let r = prepared.classify_labels(id, ...);
probe_reuse.push((i, r));                        // and the verdict is REUSED at position i
```

Not merely a mis-sampled heuristic: `probe_reuse` feeds `reuse[i]`, which the build loop
returns **in place of** rebuilding — so report position `i` is handed the label oracle of a
different class. The label cache drives the unsat probe (`label_cache.get(i)` → a wrong
`Unsat` puts the wrong class in `unsatisfiable_idxs`, i.e. the defect-(b) blast radius
again) and the tier walk's pruning. Fixed to `reported.class_id(i)`.

The main build loop below it already used `reported.class_id(i)` — our own patch. The probe
was written after the branch point and did not follow it.

### Offenders 3 & 4 — diagnostics only. Corrected, no soundness impact.

* `tainted_classes(internal, n)` was called with the **report count** but sizes
  `vec![false; num_classes]` and writes it via `tainted.get_mut(c.index())` — a ClassId. The
  `get_mut` guard turned every id above the report count into a silent skip, and any DKey id
  in range marked the wrong slot. Only consumer is `dump_label_cache`. Now passed
  `reported.num_class_ids()`; the loop is genuinely id-space on both sides and is declared
  inside the conversion boundary.
* `dump_label_cache`'s `#closure` section minted `ClassId::new(report_pos)`. Now
  `reported.class_id(i)`. The dump legitimately mixes spaces (its `#labels`/`#times`/
  `#closure` row keys are report positions; the `#tainted` index and every subsumer id are
  ClassIds) — **documented in the function's doc comment** rather than silently changed, so
  nobody joins the sections on the bare number.

### Cleared — checked and genuinely correct

* **`pairs_per_sub`** (`stats.pairs_per_sub.entry(sub.index())`) — genuinely id space and
  never leaves it. The only consumer, `owl-dl-cli/src/main.rs:819`, reports quantiles of the
  **values** and never maps a key back to a class. Declared inside the boundary with that
  argument written down.
* **`DirectSubsumerIndex`** (`build_direct_index` / `direct_subsumers_fast` /
  `minimal_sat`, `RUSTDL_FAST_DIRECT_SUBSUMERS`, new on main) — entirely report space
  (`self.classes`, `self.unsatisfiable_idxs`, `self.entailed`). No ClassId enters. Clean.
* **`probe_says_inconsistent` layers 2–3** (`consistency_wedge`, bounded `decide(Top)`) — no
  indices at all.
* **`realize.rs`** — works in the **full class-id space** (`class_iris` covers
  `0..num_classes()` so `enumerate()` index *is* the ClassId) and its `unsat` set is keyed by
  **IRI string**, so the two spaces never meet. The documented DKey-leak caveat at
  `realize.rs:938–975` is pre-existing and unchanged.
* **`lib.rs`** `.index()` / `ClassId::new` sites — Tseitin/nominal encoding, pure id space,
  no report vector in scope.
* **`reportable_class_iris`** — no longer exists (replaced by `ReportedClasses::collect`), so
  main could not have added a caller. Confirmed by grep across `crates/`.
* **`Classification`'s public API** is IRI-keyed (`is_subclass`, `equivalent_classes`,
  `direct_subsumers`, `unsatisfiable_classes`, `undecided_pairs`). Only `classes()` exposes
  the report vector, and consumers index it through the paired `index` map.

### The "no cast at all" shape is now closed by the type system

Defect (b)'s spelling — a `usize` report position indexing an id-space structure — needed an
externally-supplied `usize`-indexed structure. Commit `822a8d5` **removed
`Subsumers::unsatisfiable_bitset()`**, and every remaining `Subsumers` method
(`contains`, `subsumers_count`, `subsumers_of`, `is_unsatisfiable`,
`unsatisfiable_classes`) is `ClassId`-typed. I found no other id-space `usize`-indexed
structure reachable from `classify.rs`. Every `vec![...; n]` in the file
(`satisfiable`, `direct_supers`, `direct_children`, `visited`, `visited_gen`, `reuse`,
`label_cache`, the `EntailmentMatrix` rows) is report-space sized **and** report-space
indexed — with `tainted` having been the sole exception, now fixed.

---

## Pre-existing tests whose verdict changed

**None.** No expectation file was edited. Full crate suite: **1001 passed, 0 failed,
74 ignored, 110 binaries** — the same set green as before the rebase.

---

## Verification

| check | result |
|---|---|
| `cargo test -p owl-dl-reasoner` (debug) | **1001 passed, 0 failed, 74 ignored** (110 binaries) |
| `cargo test -p owl-dl-reasoner --test dkey_id_aliasing` (debug) | **18 passed, 0 failed, 1 ignored** (16 + 2 new) |
| `cargo test -p owl-dl-reasoner --release --test dkey_id_aliasing` | **19 passed, 0 failed, 0 ignored** — including `inert_declarations_do_not_change_the_hierarchy_pizza`, which is debug-ignored |
| `cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

Not run (owned by you, per the brief): `cargo test --workspace`,
`clippy --all-targets --all-features`, `./scripts/run-soundness-diff.sh`.

### Sabotage checks (each fix proven load-bearing)

| sabotage | result |
|---|---|
| revert offender 1 | `consistency_probe_does_not_invent_an_inconsistency` **FAILS** (all 6 classes ⊥) *and* the hardened scanner **FAILS** — both nets fire |
| revert offender 2 | hardened scanner **FAILS** at `classify.rs:3588`; **the old scanner could not see that line** |
| — | the non-vacuity guard `consistency_probe_fixture_really_aliases_the_asserted_class` stays green under both, correctly (the layout is what it pins, not the fix) |

### New tests (2, both in `crates/owl-dl-reasoner/tests/dkey_id_aliasing.rs`)

* `consistency_probe_does_not_invent_an_inconsistency` — the soundness oracle, run against
  `classify`, `classify_n2` and `classify_top_down_with_timeout`.
* `consistency_probe_fixture_really_aliases_the_asserted_class` — the non-vacuity guard, kept
  **separate** so an interning-order drift is reported as a layout failure rather than
  letting the oracle pass while testing nothing. It asserts `Fff`'s ClassId equals `Eee`'s
  report position and that a DKey sits below a user class.

---

## Concerns

1. **The scanner's blindness is the most important thing here, above either defect.** Two
   soundness bugs of a class this repo had *already diagnosed, fixed and written a guard for*
   still landed on `main`, because the guard silently stopped covering the code. It had
   presumably been degraded ever since the inline test module at line 1039 was added. This is
   the codebase's own `[[sabotage-your-own-guard-tests]]` doctrine paying out: **the coverage
   floor I added matters more than the brace matching.**

2. **~~`probe_says_inconsistent` is default-ON and produces false positives on `main`
   today.~~ RETRACTED — see Offender 1.** `main` is sound at this site; the false positive
   exists only between `9cc380c`/`ac721de` and this commit, i.e. inside this branch. **No
   release note is owed**, and the earlier instruction to write one is withdrawn.

   What remains true is the *shape* of the hazard, which is why the canary in
   `crates/owl-dl-reasoner/tests/classify_dkey_alias_consistency.rs` is worth keeping: it needs
   (a) a DKey below a user class, (b) ≥1 unsatisfiable class, and (c) a `ClassAssertion` whose
   class's ClassId collides with an unsat class's report position — narrow enough that the
   curated corpus almost certainly does not hit it (the corpus is documented as inert for the
   DKey area), but the trigger is a **used-but-undeclared class**, which is legal OWL and common
   in the wild, and the blast radius is the entire hierarchy. That is the reason to fix the index
   spaces at both ends rather than to trust the narrowness.

3. **`classify_prep_timeout` argues for the sharper lesson from this rebase.** The riskiest
   thing was not any conflict — it was the caller git merged *cleanly*. The type system caught
   that one. It would not have caught it had main's new caller passed a `usize` report index
   into something expecting a class id, which is exactly the shape of both defects found.
   **`ReportedClasses` earns its keep by making the two spaces distinct TYPES**; the remaining
   exposure is wherever raw `usize` still crosses a function boundary.

4. **I did not add a behavioural oracle for offender 2** (the label-cache escalation probe).
   It is default-OFF, and reproducing it behaviourally needs a fixture that both aliases *and*
   makes the strided scan land on the aliased class at a budget where one class fails and
   another does not — brittle enough that a vacuously-passing test is the likely outcome, which
   this codebase rightly treats as worse than none. The hardened scanner covers its spelling.
   Flagging it as a known gap rather than shipping a fixture I cannot prove non-vacuous.

5. **Unrelated dirty files left untouched:** `crates/owl-dl-py/examples/nesy_loop/{llm,run}.py`
   were already modified in the working tree when I started. Not staged, not committed.

6. **Part-2 fixes are a separate commit** (`60b2e22`) on top of the four replayed ones, so they
   can be reviewed — or dropped — independently of the rebase itself.

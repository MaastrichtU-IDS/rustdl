# Label-Cache Back-Fold (close galen `TT ⊑ TICE`) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the last galen residual (`TibialTuberosity ⊑ TibialInterCondylarEminence`) by adding a sound, cheap **back-fold** derivation — EL `∃`-composition into defined classes, run over the wedge's already-built, branch-free, merge-enriched `sat(c)` graph — injected directly into the hierarchy with **zero** tableau calls (so it does NOT explode like `RUSTDL_CLASSIFY_DEFINED_SWEEP`).

**Architecture:** Design spec: `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md` (READ IT — it is the authoritative design; this plan sequences its §7 outline). Precompute the `∃`-bearing defined-class bodies once; after each per-class `sat(c)` completes branch-free, structurally recognize any defined `D ≡ ⊓Aᵢ ⊓ ∃rⱼ.Cⱼ` the root satisfies (`Aᵢ ∈ labels`, each `∃rⱼ.Cⱼ` has a resolved successor labelled `Cⱼ`) and inject `D` as an entailed direct super — mirroring the existing defined-SUB sweep, no `subsumes_via_tableau`.

**Tech Stack:** Rust (edition 2024). `RUSTUP_TOOLCHAIN=stable` on every cargo command. **Rebuild BOTH `-p owl-dl-cli` AND `-p owl-dl-bench` before any CLI/matrix run** (stale-bench trap bit us twice).

## Global Constraints

- **Soundness gate (hard, THE gate):** corpus-wide FP=0 on the `konclude_closure_diff` suite, unchanged. The back-fold's soundness is the entire risk surface. Any FP → STOP (unsound).
- **Load-bearing soundness invariant:** the back-fold derivation fires ONLY when the `sat(c)` run is branch-free (`branches_taken == 0`) — equivalently every label it reads has `deps_of == DepSet::EMPTY`. Branch-free ⟹ least Horn model ⟹ genuine entailment ⟹ FP-free. This gate MUST be unit-tested directly (a branched-sat canary where the precondition holds but the subsumption does not → must NOT derive).
- **Completeness gate:** galen `MISSED 1 → 0`; no *new* MISSED on any corpus ontology (byte-identical closures except galen's +1).
- **Perf gate:** zero tableau/wedge search calls in the back-fold; galen classify stays ~sub-second (was 0.85 s); no ontology regresses > ~10%. It must NOT behave like `DEFINED_SWEEP` (>6:40).
- **Gated behind `RUSTDL_CLASSIFY_BACKFOLD`** (default OFF) through Tasks 1–4; flag-OFF path byte-identical. Task 5 flips default ON only after all gates pass.
- Pattern to mirror for the direct hierarchy injection (no tableau): the **defined-SUB sweep** at `crates/owl-dl-reasoner/src/classify.rs:1833-1836` (`direct_supers[c].push(...)`).
- Reasoner-core code: READ each cited site and the design's §1–§4 before editing; do not blind-transcribe.

---

## Task 1: Precompute `∃`-bearing defined-class bodies (+ genus index)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` — `HyperCache` struct + `HyperCache::build` (~`:2016`).
- Test: inline `#[cfg(test)]` in `lib.rs` (or the nearest existing test module for `HyperCache`).

**Interfaces:**
- Consumes: `owl_dl_core::definitions::extract_definitions` (already called at `lib.rs:2020`), `Definitions::body_of(c)` (`definitions.rs:34`), `ConceptExpr` (`And`/`Some`/`Class`).
- Produces on `HyperCache`: `defined_exists_bodies: Vec<DefinedBody>` where `struct DefinedBody { name: ClassId, atoms: SmallVec<[ClassId; 4]>, exists: SmallVec<[(Role, ClassId); 2]> }` (only bodies with ≥1 `∃`-conjunct; purely-atomic bodies excluded — they already fire via Horn clauses). Plus `defined_body_by_genus: HashMap<ClassId, SmallVec<[usize; 4]>>` mapping each atomic conjunct (genus) → indices into `defined_exists_bodies`, so recognition only scans bodies whose genus is present.

- [ ] **Step 1: Write the failing test**

Add a test that builds a `HyperCache` from a tiny ontology with `D ≡ A ⊓ ∃r.C` and `E2 ≡ A2` (purely atomic) and asserts `defined_exists_bodies` contains `D` (atoms `[A]`, exists `[(r,C)]`) and NOT `E2`, and `defined_body_by_genus[A]` includes `D`'s index:

```rust
#[test]
fn defined_exists_bodies_extracted_and_genus_indexed() {
    let onto = /* build: Declaration D,E2,A,A2,C ; ObjectProperty r ;
        EquivalentClasses(D, ObjectIntersectionOf(A, ObjectSomeValuesFrom(r, C))) ;
        EquivalentClasses(E2, A2) */;
    let internal = /* convert_ontology(&onto) — mirror how other lib.rs tests build InternalOntology */;
    let hc = HyperCache::build(&internal);
    let d = /* ClassId of D */;
    assert!(hc.defined_exists_bodies.iter().any(|b| b.name == d), "D (∃-bearing) present");
    assert!(!hc.defined_exists_bodies.iter().any(|b| b.name == /*E2*/), "E2 (atomic) excluded");
    // genus index: D indexed under its atomic conjunct A
    assert!(hc.defined_body_by_genus.get(&/*A*/).is_some_and(|v| v.iter().any(|&i| hc.defined_exists_bodies[i].name == d)));
}
```
(Read an existing `HyperCache`/`convert_ontology` test in `lib.rs` first and copy its ontology-construction idiom for the `/* ... */` parts.)

- [ ] **Step 2: Run — expect FAIL** (`defined_exists_bodies`/`defined_body_by_genus` fields don't exist).
Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib defined_exists_bodies_extracted 2>&1 | head` → FAIL (unknown field).

- [ ] **Step 3: Implement**
Add the `DefinedBody` struct, the two fields to `HyperCache`, and populate them in `HyperCache::build` from the already-extracted `Definitions`: for each defined name `c`, walk `body_of(c)`; if it is an `And` (or a single `Some`) with ≥1 `ConceptExpr::Some(role, ConceptExpr::Class(filler))` conjunct, collect atomic `Class` conjuncts into `atoms` and `(role, filler)` into `exists`; skip if `exists` empty. Build `defined_body_by_genus` from each body's `atoms`. Keep this behind neither flag nor cost when there are no defined classes (empty vecs). (Flag-gating is applied at the USE site in Task 2/3, not here — precompute is cheap and inert if unused.)

- [ ] **Step 4: Run — expect PASS.** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib defined_exists_bodies_extracted`.

- [ ] **Step 5: Lints + commit**
`RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings` + `cargo fmt -p owl-dl-reasoner -- --check` → clean.
```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(backfold): precompute ∃-bearing defined-class bodies + genus index on HyperCache"
```

---

## Task 2: `backfold_derived` engine rule (branch-free gated) + carry it out of `classify_labels`

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — new `HyperEngine::backfold_derived` near `distinct_role_succ` (`:2456`); `SearchStats`/`branches_taken` (`:441-458`) is consumed.
- Modify: `crates/owl-dl-reasoner/src/lib.rs` — `LabelOracle::Sat` (`:1871`) gains `derived_sups`; `classify_labels` (`:2510-2631`, esp. the `HyperResult::Sat` return `~:2622-2628`) calls the rule.
- Test: inline `#[cfg(test)]` in `hyper.rs`.

**Interfaces:**
- Produces: `HyperEngine::backfold_derived(&self, root: HNode, bodies: &[DefinedBody], genus: &HashMap<ClassId,SmallVec<[usize;4]>>) -> Vec<ClassId>` — the entailed defined-`∃` names recognized at `resolve(root)`, ONLY if `self.stats().branches_taken == 0`; else returns empty. Reuses `resolve` + `distinct_role_succ(x, r, Some(C))`.
- `LabelOracle::Sat { labels, derived_sups: Vec<ClassId> }` — `derived_sups` are ENTAILED (not candidates).

- [ ] **Step 1: Write the failing tests (two — positive + the FP tripwire)**

In `hyper.rs` `#[cfg(test)]`:
```rust
#[test]
fn backfold_derives_defined_exists_over_forward_inverse_merge() {
    // Build the sat(TT)-shaped graph: root x:Eminence, x -g-> w, w gains TibialPlateau
    // via the ≤1 merge (reuse the funcmerge builder from the le1 tests). Body:
    // TICE ≡ Eminence ⊓ ∃g.TibialPlateau. Assert backfold_derived(root,...) contains TICE,
    // AND branches_taken==0 held.
}
#[test]
fn backfold_does_not_fire_when_branched() {
    // A graph where sat branched (branches_taken>0) and the structural precondition
    // holds in the chosen branch but D is NOT entailed → assert backfold_derived returns []
    // (the load-bearing soundness gate).
}
```
(Read the existing `le1_*` tests to reuse their graph-construction helpers.)

- [ ] **Step 2: Run — expect FAIL** (`backfold_derived` undefined).

- [ ] **Step 3: Implement `backfold_derived`**
Per design §1.2/§4.2: if `self.stats.branches_taken != 0` return `vec![]`. Else `let x = self.resolve(root);` and for each body reachable via the genus index whose every `atom ∈ self.nodes[x].labels` and every `(r,C)` has `!self.distinct_role_succ(x, r, Some(C)).is_empty()`, push `body.name`. (Optionally also require the witness label's `deps_of == EMPTY` for a finer gate; v1 may use the whole-run `branches_taken==0` gate.) Return the collected names. Zero search calls.

- [ ] **Step 4: Wire into `classify_labels`**
Extend `LabelOracle::Sat` with `derived_sups: Vec<ClassId>` (default empty). In `classify_labels`, on `HyperResult::Sat`, if `crate::classify_backfold_enabled()` (Task 3 adds the flag; for now reference it) call `engine.backfold_derived(root, &self.defined_exists_bodies, &self.defined_body_by_genus)` and return it in `derived_sups`; else `derived_sups: vec![]`. Update the two prune sites (`classify.rs:1678`, `:1976`) only to destructure the new field (they keep reading `labels` exactly as today — `derived_sups` is ignored there).

- [ ] **Step 5: Run tests — expect PASS** (`cargo test -p owl-dl-tableau backfold`), and `cargo test -p owl-dl-tableau` (no regression).

- [ ] **Step 6: Lints + commit**
clippy `-D warnings` + fmt clean (both crates).
```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(backfold): branch-free ∃-composition rule over sat graph + carry derived_sups from classify_labels"
```

---

## Task 3: Inject entailed `derived_sups` into the hierarchy + the flag

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` — after the label-cache build (`~:1303`), the injection; `stats.backfold_recovered` counter.
- Modify: `crates/owl-dl-reasoner/src/lib.rs` — `classify_backfold_enabled()` flag (alongside `classify_defined_sweep_enabled` `:1621`).
- Test: `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs` or a new `tests/backfold.rs`.

**Interfaces:**
- Consumes: `LabelOracle::Sat.derived_sups` (Task 2); `direct_supers`/`direct_children` (`classify.rs`), `closure.contains` (`:1969/1631`).
- Produces: `classify_backfold_enabled() -> bool` (default OFF: `std::env::var_os("RUSTDL_CLASSIFY_BACKFOLD").is_some_and(|v| v == "1")`).

- [ ] **Step 1: Write the failing integration test**
Add to `crates/owl-dl-reasoner/tests/backfold.rs`: the two minimal repros from the residual doc (told-filler and **merge-derived-filler** `TT≡E⊓∃g.Sub`, `TICE≡E⊓∃g.Sup`, `Sub⊑Sup` merge-derived), asserting via `classify(&onto)` (the classify path, NOT `is_subclass_of`) that `is_subclass("TT","TICE")` holds. Set `RUSTDL_CLASSIFY_BACKFOLD=1` in the test. Also assert that with the flag UNSET the classify path still succeeds on a normal ontology (flag-off sanity).

- [ ] **Step 2: Run — expect FAIL** (without the injection, the classify path prunes `TT⊑TICE`).

- [ ] **Step 3: Implement the flag + injection**
Add `classify_backfold_enabled()`. In `classify.rs` after the label cache is built, for each class `c` with `LabelOracle::Sat { derived_sups, .. }` and each `D ∈ derived_sups`: if `!closure.contains(c, D)` and `!direct_supers[c].contains(&D)`, do `direct_supers[c].push(D); direct_children[D].push(c); stats.backfold_recovered += 1;` — mirroring the defined-SUB sweep (`:1833-1836`), **no `subsumes_via_tableau`**. The existing transitive-closure BFS (`~:1866-1887`) propagates it.

- [ ] **Step 4: Run — expect PASS**; `cargo test -p owl-dl-reasoner --test backfold` + full `--test funcmerge_inverse --test funcmerge_scaling` still pass; `cargo test -p owl-dl-tableau` clean.

- [ ] **Step 5: Lints + commit**
clippy `-D warnings` + fmt clean.
```bash
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/backfold.rs
git commit -m "feat(backfold): inject entailed defined-∃ supers into hierarchy behind RUSTDL_CLASSIFY_BACKFOLD (default off)"
```

---

## Task 4: Gates — galen MISSED 1→0, corpus FP=0, wall sub-second (flag ON)

**Files:** verification only (uses `~/eval-tools` + the matrix + closure-diff).

- [ ] **Step 1: Soundness gate — closure-diff FP=0, flag ON**
`RUSTUP_TOOLCHAIN=stable RUSTDL_CLASSIFY_BACKFOLD=1 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture 2>&1 | grep -iE 'FP=|test result'` → every fixture FP=0, all pass. Record each fixture's FP/MISSED. **Any FP>0 → STOP (unsound; the branch-free gate is wrong — investigate before anything else).**

- [ ] **Step 2: galen completeness + wall gate**
Rebuild fresh cli+bench. `RUSTUP_TOOLCHAIN=stable RUSTDL_CLASSIFY_BACKFOLD=1 ./target/release/owl-dl-bench matrix --tier curated --out /tmp/matrix-bf --pair-timeout-ms 250 --global-timeout-s 60` (delete any stale `/tmp/matrix-bf/results.jsonl` first). Verify galen `rustdl FP 0 MISSED 0` (was 1), whole-tier rustdl FP=0, galen wall sub-second (NOT DEFINED_SWEEP-like), all onts finish. Report the galen cell + total wall.

- [ ] **Step 3: no-new-MISSED + wall sanity**
Confirm no other curated ont's rustdl MISSED increased; `time` galen/wine/sio/pizza classify flag-on stay comparable to flag-off (±10%). Record. If any explodes → the genus-index scan is the cause (design §6) — report.

No commit (verification). Record all gate numbers in the task report. **Proceed to Task 5 only if galen MISSED→0, corpus FP=0, no new MISSED, and wall is sub-second.**

---

## Task 5: Default-ON, regenerate matrix, docs → galen complete

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`classify_backfold_enabled` default ON, `=0` escape).
- Modify: `crates/owl-dl-reasoner/tests/backfold.rs` (drop the flag-set; default-on).
- Regenerate: `docs/benchmarks/2026-07-11-curated/`.
- Modify: `README.md`, `CLAUDE.md`, `docs/known-limitations/galen-defined-class-monotonicity-residual.md`.

- [ ] **Step 1: Flip default ON** (`is_none_or(|v| v != "0")`); update doc-comment; drop the test's flag-set; verify tests pass at default.
- [ ] **Step 2: Re-run gates at default** — closure-diff FP=0, galen MISSED 0, wall sub-second (Task 4 commands without the env).
- [ ] **Step 3: Regenerate the authoritative matrix at default** — galen `rustdl FP 0 MISSED 0`; whole-tier FP=0.
- [ ] **Step 4: Docs** — update `galen-defined-class-monotonicity-residual.md` to "RESOLVED by the back-fold rule (default on)"; README/CLAUDE.md galen now "sound AND complete on the curated corpus (MISSED=0)". Restore the completeness-contract statement (Horn⟹MISSED=0 now holds on the whole curated corpus). Keep the honest history.
- [ ] **Step 5: Full lints + commit**
`cargo clippy -p owl-dl-tableau -p owl-dl-reasoner -p owl-dl-bench --all-targets --all-features -- -D warnings` + `cargo fmt --all -- --check`.
```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/backfold.rs docs/benchmarks/2026-07-11-curated docs/known-limitations README.md CLAUDE.md
git commit -m "feat(backfold): default-on; galen now complete (MISSED 0); regen matrix + docs"
```

---

## Self-Review

**Spec coverage:** precompute bodies (design §7.1 → Task 1); `backfold_derived` branch-free rule (§7.2, §1.2, §2a → Task 2); carry `derived_sups` (§7.3-4 → Task 2); hierarchy injection + flag (§7.5-6 → Task 3); gates incl the load-bearing FP=0 closure-diff and the no-explosion wall (§6 → Task 4); default-flip + docs (§7 → Task 5). ✓

**Placeholder scan:** Test bodies in Tasks 1-2 intentionally reference "reuse the existing builder/le1 helpers" because the in-crate graph/ontology construction API must be read first (each step says so); the rule logic, gate, injection, flag, and all commands/gates are concrete. Reasoner-core edits are "read-the-site-and-mirror-(defined-SUB-sweep / le1-tests)" by necessity, with exact file:line anchors.

**Type consistency:** `DefinedBody{name,atoms,exists}`, `defined_exists_bodies`, `defined_body_by_genus`, `backfold_derived(HNode,&[DefinedBody],&HashMap)->Vec<ClassId>`, `LabelOracle::Sat{labels,derived_sups}`, `classify_backfold_enabled()->bool` used consistently across tasks and matched to the spec's anchors. The load-bearing invariant (branch-free/`EMPTY`-deps gate) and THE test (corpus FP=0 closure-diff) are called out in Global Constraints and Task 2/4.

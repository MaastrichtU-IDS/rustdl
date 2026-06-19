# Classify-completeness (SP1.1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make inverse/symmetric-domain-derived subsumptions appear in **default `classify`** — by giving the classify oracle the role hierarchy (Layer A) and broadening the existing same-tier sweep from defined-sups-only to **label-driven** (Layer B).

**Architecture:** Layer A cherry-picks the validated `wip/sp1.1-classify-oracle-reach` change (`HyperCache` carries the `RoleHierarchy`; classify-label engine uses `with_sub_roles_keep_index` to keep the amortized index). Layer B broadens the existing label-gated "defined-sup sweep" in `classify_top_down_internal` to also test sups that appear in any (now-complete) label set — closing the "equal-closure-count classes never compared" gap. FP-safe by construction (every recorded subsumption is oracle-confirmed).

**Tech Stack:** Rust (edition 2024); `owl-dl-reasoner` (`lib.rs`, `classify.rs`), `owl-dl-tableau` (`hyper.rs`); `konclude_closure_diff` corpus net; Konclude oracle.

**Spec:** `docs/superpowers/specs/2026-06-19-classify-completeness-sp1.1-design.md`

**Soundness law:** FP=0, and corpus closures **MISSED-reduced-or-unchanged** (no closure may *shrink* — that signals a bug). Layer B only *adds candidate pairs to test*; every recorded subsumption is confirmed by the oracle, and the label gate only prunes. Verify with the closure net before declaring done.

---

## Conventions

- Toolchain: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- Build: `cargo build --release -p owl-dl-cli`.
- Branch is already `feat/classify-completeness-sp1.1` (spec committed `0363edd`). Do NOT touch main.
- Layer A reference: `git show a2b3014` (on `wip/sp1.1-classify-oracle-reach`).

---

## Task 1: Layer A — carry the role hierarchy into the classify oracle

Cherry-pick the validated POC. Two files.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add `with_sub_roles_keep_index`)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`HyperCache`: `sub_roles` field + build + decide + classify_labels)

- [ ] **Step 1: Add `with_sub_roles_keep_index` to `HyperEngine`**

In `crates/owl-dl-tableau/src/hyper.rs`, right after the existing `with_sub_roles` method (~line 851):
```rust
/// Set the role hierarchy WITHOUT rebuilding `ClauseIndexes`. Use only when
/// the engine's index was already built hierarchy-aware
/// (`build_clause_indexes(.., Some(&h))`), e.g. the amortized classify-label
/// path that supplies a prebuilt index via `new_with_prebuilt`. Setting the
/// hierarchy enables `role_matches` symmetry + sub-role matching; the index
/// (trigger sets) must already reflect the same hierarchy.
#[must_use]
pub fn with_sub_roles_keep_index(mut self, hierarchy: RoleHierarchy) -> Self {
    self.sub_roles = Some(hierarchy);
    self
}
```

- [ ] **Step 2: Add the `sub_roles` field to `HyperCache`**

In `crates/owl-dl-reasoner/src/lib.rs`, in `pub(crate) struct HyperCache` (after `base_disjoint_pairs`):
```rust
    /// Role hierarchy for inverse + symmetric domain/range firing.
    /// Built once in `HyperCache::build` and passed into every engine so
    /// `domain(p⁻, C)` fires at the TARGET of `p`-edges on generated successors,
    /// not just ABox-seeded nodes. Without this, classify misses subsumptions
    /// derivable only via an inverse-domain triggered on a generated successor.
    sub_roles: RoleHierarchy,
```
(Confirm `RoleHierarchy` is in scope; `build_role_hierarchy` returns it in this file.)

- [ ] **Step 3: Build the hierarchy + hierarchy-aware base index in `HyperCache::build`**

After the `sup_neg = build_sup_neg_map(...)` call, add:
```rust
        // Build the role hierarchy from the clausified ontology so
        // `domain(p⁻, C)` and symmetric-role domain/range fire on generated
        // successors in the classify subsumption oracle. Built after
        // clausification so role ids match the role atoms in `clauses`.
        let sub_roles = build_role_hierarchy(&internal);
```
Change the base-index build from `None` to `Some(&sub_roles)`:
```rust
        let mut base_indexes_inner =
            owl_dl_tableau::hyper::build_clause_indexes(&clauses, Some(&sub_roles));
```
Add `sub_roles,` to the returned `Self { ... }` literal.

- [ ] **Step 4: Thread the hierarchy into `decide` + `classify_labels`**

In `HyperCache::decide`, change `let mut engine = HyperEngine::new(&clauses, self.fresh_q);` to:
```rust
        let mut engine =
            HyperEngine::new(&clauses, self.fresh_q).with_sub_roles(self.sub_roles.clone());
```
In `HyperCache::classify_labels`, chain `.with_sub_roles_keep_index(self.sub_roles.clone())` onto the `HyperEngine::new_with_prebuilt(...)` result (NOT `with_sub_roles` — the prebuilt index is already hierarchy-aware from Step 3; rebuilding would defeat the amortization).

- [ ] **Step 5: Build + existing tests green**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-reasoner --test inverse_symmetric_domain && cargo test -p owl-dl-tableau`
Expected: builds; existing SP1 motif (9) + wedge suite pass. Clippy `-p owl-dl-reasoner -p owl-dl-tableau --all-targets -- -D warnings` clean; fmt.

- [ ] **Step 6: Commit**

```sh
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(classify): SP1.1 Layer A — carry role hierarchy into the classify oracle

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: End-to-end driver test (RED — fails after Layer A alone, the tier gap)

**Files:**
- Test: `crates/owl-dl-reasoner/tests/classify_inverse_domain.rs` (the SP1.1-POC test file from `a2b3014`; if absent on this branch, create it — the implementer can copy the parse+classify helper from `konclude_closure_diff.rs` / `classify_top_down_with_timeout`)

- [ ] **Step 1: Add the default-`classify` driver test**

```rust
// SP1.1 end-to-end: C ⊑ D, derivable only via inverse-domain firing on a
// generated successor, must appear in DEFAULT classify (top-down tier walk),
// not just the N² path. RED after Layer A alone (same-tier gap), GREEN after
// Layer B.
#[test]
fn default_classify_finds_inverse_domain_subsumption() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;
    let src = r#"Prefix(:=<http://e#>)
Ontology(
Declaration(ObjectProperty(:p))
Declaration(Class(:C)) Declaration(Class(:G)) Declaration(Class(:H)) Declaration(Class(:K)) Declaration(Class(:D))
SubClassOf(:C ObjectSomeValuesFrom(:p :G))
ObjectPropertyDomain(ObjectInverseOf(:p) :H)
SubClassOf(ObjectIntersectionOf(:G :H) :K)
SubClassOf(ObjectSomeValuesFrom(:p :K) :D)
)
"#;
    let mut r = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    // The Classification must report C ⊑ D. Use whatever the public API exposes
    // (e.g. `c.is_subsumed("http://e#C", "http://e#D")` or scan the subsumption
    // pairs) — mirror how konclude_closure_diff.rs reads subsumptions.
    assert!(
        classification_has_subsumption(&c, "http://e#C", "http://e#D"),
        "default classify must report C ⊑ D (inverse-domain on generated successor)"
    );
}
```
(`classification_has_subsumption` — implement as a small helper using the same `Classification` accessor `konclude_closure_diff.rs` uses to enumerate pairs. If the API exposes direct `(sub,sup)` IRIs, scan for the pair.)

- [ ] **Step 2: Run; expect FAIL (tier gap)**

Run: `cargo test -p owl-dl-reasoner --test classify_inverse_domain default_classify_finds_inverse_domain_subsumption`
Expected: FAIL — `C` and `D` are same-tier (equal closure-subsumer count), the tier walk skips the pair, and the defined-sup sweep doesn't cover `D` (not an `EquivalentClasses`-defined class). This RED confirms Layer B is needed.

- [ ] **Step 3: Commit the failing test**

```sh
git add crates/owl-dl-reasoner/tests/classify_inverse_domain.rs
git commit -m "test(classify): SP1.1 end-to-end driver (default classify finds C⊑D) — RED

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Layer B — broaden the same-tier sweep to label-driven

The existing "defined-sup sweep" (`classify.rs` ~1473–1620) tests candidate subs against **defined** sups only (`EquivalentClasses(Name, ComplexExpr)`), label-gated + parallel. Broaden its sup-side to also include sups that appear in any (now-complete, via Layer A) label set, so engine-derived same-tier sups like `D` are tested. Reuses the entire existing sweep body.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (the `defined_sups` → sweep loop)

- [ ] **Step 1: Build the broadened sup set**

Immediately after the `defined_sups` binding completes (the `set.into_iter().collect()` at ~line 1510–1511) and before `let sweep_budget = ...`, add:
```rust
    // SP1.1 Layer B: broaden the sweep's sup-side from defined-classes-only to
    // LABEL-DRIVEN. With the classify oracle now hierarchy-aware (Layer A),
    // `labels(cand)` includes inverse/symmetric-domain-derived subsumers, so any
    // such sup appears in some label set. Union those in; the existing
    // label-gated sweep body then tests them (gated, so cost stays bounded by
    // the label heuristic — a sup that's in no label set adds zero oracle calls).
    // Sound: the sweep only ADDS candidate pairs; every recorded subsumption is
    // oracle-confirmed and the label gate only prunes.
    let sweep_sups: Vec<usize> = {
        let mut set: std::collections::HashSet<usize> = defined_sups.iter().copied().collect();
        for oracle in &label_cache {
            if let crate::LabelOracle::Sat(labels) = oracle {
                for &sup_id in labels {
                    let i = sup_id.index() as usize;
                    if i < n && !unsatisfiable_idxs.contains(&i) {
                        set.insert(i);
                    }
                }
            }
        }
        set.into_iter().collect()
    };
```

- [ ] **Step 2: Drive the sweep off `sweep_sups`**

Change the sweep loop header from `for &sup in &defined_sups {` to `for &sup in &sweep_sups {`. Nothing else in the loop body changes (the label-gate at ~1570 already prunes non-candidates; the `already_known` BFS + `closure.contains` filters already skip established pairs).

- [ ] **Step 3: Run the driver test; expect PASS**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-reasoner --test classify_inverse_domain`
Expected: `default_classify_finds_inverse_domain_subsumption` now PASSES (D ∈ labels(C) via Layer A → broadened sweep tests C⊑D → oracle confirms). The existing POC tests stay green.

- [ ] **Step 4: Same-tier FP control test**

Add to `classify_inverse_domain.rs`:
```rust
// FP control: two same-tier classes that are NOT subsumption-related must NOT
// be reported subsumed by the broadened sweep.
#[test]
fn default_classify_no_spurious_same_tier_subsumption() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;
    let src = r#"Prefix(:=<http://e#>)
Ontology(
Declaration(ObjectProperty(:p))
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))
SubClassOf(:A ObjectSomeValuesFrom(:p :E))
SubClassOf(:B ObjectSomeValuesFrom(:p :E))
)
"#;
    let mut r = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(!classification_has_subsumption(&c, "http://e#A", "http://e#B"));
    assert!(!classification_has_subsumption(&c, "http://e#B", "http://e#A"));
}
```
Run: `cargo test -p owl-dl-reasoner --test classify_inverse_domain` — both new tests pass.

- [ ] **Step 5: Clippy/fmt + commit**

Run: `cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings && cargo fmt --all`
```sh
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-reasoner/tests/classify_inverse_domain.rs
git commit -m "feat(classify): SP1.1 Layer B — label-driven same-tier sweep; driver GREEN

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Corpus closure net + perf gate (the sacred gate)

**Files:** none (measurement).

- [ ] **Step 1: Closure net — FP=0 + MISSED-reduced-or-unchanged**

Run: `cargo test --release -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored --nocapture 2>&1 | grep -iE 'rustdl_closure=|FP=|MISSED=|test result'`
Expected: every `*_closure_matches_*` fixture **FP=0**, and each `rustdl_closure` **≥ its pre-SP1.1 value** (galen 27997, notgalen 32739, sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16). A closure that **shrinks** → bug → STOP/revert. A closure that **grows with FP=0** = a recovered MISS (record it). `family_inconsistency_detected` failing under `--include-ignored` is expected.

- [ ] **Step 2: Perf gate — no wall blowup**

Run classify walls on galen, sio, ore-10908, **ore-15672** (the Layer-B perf-risk fixture), wine:
```sh
for f in external/galen external/ore-10908-sroiq external/ore-15672-shoin real/sio; do
  echo -n "$f: "; { /usr/bin/time -v ./target/release/rustdl classify ontologies/$f.ofn >/dev/null ; } 2>&1 | grep -oE 'Elapsed.*: [0-9:.]+' | grep -oE '[0-9:.]+$'
done
```
Expected: galen ≈0.2s, sio ≈2s, ore-10908 ≈0.2s within tolerance; **ore-15672 watched** — Layer B may surface same-tier hard pairs (deadline-capped). Record the wall; if it blows up materially beyond the 138s baseline, the ship criterion fails → gate Layer B behind a flag or revert. (wine is slow; spot-check with `--pair-timeout-ms 25`.)

- [ ] **Step 3: Record results in this plan's Results section + decide accept/revert**

Accept iff: driver GREEN, FP=0, no closure shrank, no material wall blowup. Else revert/flag-gate Layer B.

---

## Task 5: Full suite, docs, cleanup

- [ ] **Step 1: Full workspace gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: green; clippy clean; fmt clean.

- [ ] **Step 2: CLAUDE.md + Results**

Add an SP1.1 entry to CLAUDE.md (`owl-dl-reasoner` section): classify oracle now hierarchy-aware (Layer A) + the same-tier sweep is label-driven (Layer B), so inverse/symmetric-domain subsumptions surface in default classify; reference the spec + plan; note any recovered corpus MISSes. Fill the Results section below.

- [ ] **Step 3: Commit + clean up the superseded wip branch**

```sh
git add CLAUDE.md docs/superpowers/plans/2026-06-19-classify-completeness-sp1.1.md
git commit -m "docs(classify): SP1.1 results + CLAUDE.md (classify-completeness shipped)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git branch -D wip/sp1.1-classify-oracle-reach   # superseded by this branch's Layer A
```

---

## Results

(Filled during execution. Record: driver test verdict; corpus FP=0 + any closure that grew (recovered MISS) or shrank (bug); ore-15672/wine walls vs baseline; accept/revert decision.)

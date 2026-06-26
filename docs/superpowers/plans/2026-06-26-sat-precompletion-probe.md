# SP3 Phase-1 precompletion-graph viability probe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Probe whether seeding the saturation's derived ∃-facts (`Zinfandel ⊑ ∃hasColor.{Red}`) collapses a hard non-collapsing wine class soundly — gating the precompletion-graph production build.

**Architecture:** Expose the saturator's derived ∃-facts + its NomKey→individual reverse map; a `precompletion_probe` fn (mirroring `seed_probe`) seeds the wedge with named subsumers PLUS translated ∃-facts (`Q → ∃R.target`, NomKey targets bridged to the wedge's clausal nominal class via the shared `IndividualId`); a controller-run gate measures branch collapse + verdict-preservation on the worst hard wine class, with a garbage control.

**Tech Stack:** Rust (edition 2024), `owl-dl-saturation` (engine `facts`/`seen_facts`/`nominal_to_ind`), `owl-dl-reasoner` (`HyperCache`, the `seed_probe` harness), `owl-dl-core` clausal nominal repr (`nominal_base = num_classes`).

## Global Constraints

- Probe-only / throwaway code; the durable deliverable is the verdict doc `docs/sat-precompletion-probe-results-2026-06-26.md`. The saturator accessor is the keep-on-GO piece.
- Soundness check for Phase-1 = **verdict preserved (Sat)** on the probed class (wine classes are satisfiable); full-corpus FP=0/MISSED=0 is Phase-2's gate, NOT claimed here.
- Branch `feat/sat-precompletion-probe` (already created, off `feat/sat-seed-sp2`); `main` untouched.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (pedantic) clean; `cargo test --workspace` green.
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the controller says so; trailers:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

## Grounding (read before editing)

- Saturator engine (`crates/owl-dl-saturation/src/lib.rs`): derived ∃-facts live in `facts: Vec<ExistentialFact{sub,role,target}>` + `facts_by_sub[c]` (indices) + `seen_facts: HashSet<(ClassId,RoleId,ClassId)>`. NomKey→individual reverse map: `nominal_to_ind: HashMap<ClassId, IndividualId>` (line ~2029). `saturate(internal) -> Subsumers` (~107) runs the engine and drops it; you add a sibling that also returns the facts + map.
- Wedge clausal nominal repr (`crates/owl-dl-core/src/clause.rs`): `Nominal(i)` → `ClassId::new(nominal_base + i.index())`, `nominal_base = num_classes` (~794). So the wedge's `{a}` = `ClassId::new(num_classes + a.index())` — the SAME id the seed must target. This shared `IndividualId` is the sound bridge.
- The seed mechanism + harness to mirror: `seed_probe` (`crates/owl-dl-reasoner/src/lib.rs` ~1094) + `tests/seed_probe_gate.rs`. `Atom::Exists(role, filler_class, var)` is the ∃ head-atom form (see `lookahead_live_disjuncts` / clause construction for how `Role` is built from a `RoleId`).
- Wine hard classes: `SweetWine`/`Zinfandel` collapse under the named seed; the probe needs one that does NOT (still hundreds-of-k branches under named-only) — Task 2 finds it.

---

### Task 0: Branch (already done)

`feat/sat-precompletion-probe` exists at the spec commit (`580c26b`). No action.

---

### Task 1: Expose derived ∃-facts + `precompletion_probe` fn

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (new pub fn exposing derived ∃-facts + `nominal_to_ind`).
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`precompletion_probe` fn).
- Test: inline `#[cfg(test)]` in reasoner `lib.rs` (translation unit test — needs `pub(crate)` visibility, like the SP2 wiring test).

**Interfaces:**
- Consumes: the saturator engine internals (`facts`, `nominal_to_ind`), `HyperCache`, `seed_probe`'s structure, `clause` nominal repr.
- Produces:
  - `owl_dl_saturation::saturate_with_exists_facts(internal: &InternalOntology) -> (Subsumers, Vec<(ClassId, RoleId, ClassId)>, std::collections::HashMap<ClassId, IndividualId>)` — `Subsumers` as today, plus every derived ∃-fact `(sub, role, target)` (from `seen_facts`, sorted for determinism), plus the `nominal_to_ind` map.
  - `owl_dl_reasoner::precompletion_probe(ont, class_iri: &str, mode: u8, depth, timeout) -> Result<Option<(HyperResult, SearchStats, f64, usize)>, ReasonError>` — `mode`: 0 none, 1 named-only (= SP2 seed), 2 named+∃ (named seed + translated derived ∃-facts of the class), 3 garbage-∃ (named seed + the same count of RANDOM `(R, target)` ∃-clauses, control). Returns `(verdict, stats, wall_ms, n_exists_seeded)`.

- [ ] **Step 1: Expose derived ∃-facts from the saturator**

In `lib.rs`, add beside `saturate`:

```rust
/// Like [`saturate`] but also returns every derived existential fact
/// `(sub, role, target)` and the `NomKey → individual` reverse map, so a
/// caller can seed a tableau with the saturation's deterministic ∃-structure.
#[must_use]
pub fn saturate_with_exists_facts(
    internal: &InternalOntology,
) -> (
    Subsumers,
    Vec<(ClassId, RoleId, ClassId)>,
    std::collections::HashMap<ClassId, IndividualId>,
) {
    let n = internal.vocabulary.num_classes();
    let role_super_map = build_role_super(internal);
    let (rules, tseitin, num_total_classes, _) =
        collect_el_rules_with_provenance(internal, &role_super_map, false);
    let role_super = freeze_role_super(&role_super_map);
    let mut engine =
        WorklistEngine::new(n, num_total_classes, rules, tseitin, role_super, false, None);
    engine.seed(internal);
    engine.run();
    let facts: Vec<(ClassId, RoleId, ClassId)> = {
        let mut v: Vec<_> = engine.seen_facts.iter().copied().collect();
        v.sort_unstable_by_key(|&(s, r, t)| (s.index(), r.index(), t.index()));
        v
    };
    let nom = engine.nominal_to_ind.clone();
    (engine.subsumers, facts, nom)
}
```
Confirm field names (`seen_facts`, `nominal_to_ind`, `subsumers`) and `WorklistEngine::new` arity against the live code; adjust the construction to match `saturate_with_config`'s body exactly (it is the template).

- [ ] **Step 2: Write the translation unit test (failing)**

Inline `#[cfg(test)]` in reasoner `lib.rs` (mirror the SP2 `sat_seed_wiring_tests` env-guard pattern):

```rust
// Fixture: C ⊑ ∃r.{a} (an ObjectHasValue, lowered to ∃r.Nominal(a)). After
// saturation, the derived ∃-fact (C, r, NomKey(a)) must translate to the wedge
// nominal class id = num_classes + a.index().
#[test]
fn precompletion_translates_nomkey_to_wedge_nominal() {
    let (internal, ids) = build_c_exists_nominal_a(); // C, r, individual a
    let (_subs, facts, nom_to_ind) =
        owl_dl_saturation::saturate_with_exists_facts(&internal);
    let n_named = internal.vocabulary.num_classes() as u32;
    // find the (C, r, NomKey) fact, translate the NomKey target:
    let (_, _, tgt) = facts.iter().copied()
        .find(|&(s, r, _)| s == ids.c && r == ids.r).expect("derived ∃-fact present");
    let ind = nom_to_ind.get(&tgt).copied().expect("target is a NomKey");
    let wedge_nominal = owl_dl_core::ir::ClassId::new(n_named + ind.index());
    assert_eq!(ind, ids.a, "NomKey maps back to individual a");
    // wedge_nominal is the id the clausifier uses for {a}; assert it is the
    // nominal_base-relative id (sanity: ≥ n_named).
    assert!(wedge_nominal.index() >= n_named);
}
```
Build `build_c_exists_nominal_a` via the crate's InternalOntology test pattern (a class `C` with `SubClassOf(C, ObjectHasValue(r, a))`). If the in-crate builder can't express `ObjectHasValue`, parse a tiny `.ofn` fixture.

- [ ] **Step 3: Run — confirm fail.** `cargo test -p owl-dl-reasoner precompletion_translates` → FAIL (fn absent).

- [ ] **Step 4: Implement `precompletion_probe`**

Mirror `seed_probe`'s structure (convert → `HyperCache::build` → clone clauses → push `Q → c`). Then:

```rust
let (subs, facts, nom_to_ind) = owl_dl_saturation::saturate_with_exists_facts(&internal);
let n_named = internal.vocabulary.num_classes();
// named seed (modes 1,2,3) — same as SP2:
if mode != 0 {
    for d in subs.subsumers_of(c) {
        if d != c && (d.index() as usize) < n_named {
            clauses.push(DlClause { body: vec![Atom::Class(cache.fresh_q, X)],
                                    head: vec![Atom::Class(d, X)] });
        }
    }
}
let mut n_exists = 0usize;
if mode == 2 {
    // ∃-seed: translate each derived ∃-fact of c.
    for &(s, r, tgt) in &facts {
        if s != c { continue; }
        let translated = if (tgt.index() as usize) < n_named {
            Some(tgt) // named target
        } else if let Some(&ind) = nom_to_ind.get(&tgt) {
            Some(ClassId::new(n_named as u32 + ind.index())) // NomKey → wedge nominal {a}
        } else {
            None // Tseitin/DKey — drop (sound under-approx)
        };
        if let Some(t) = translated {
            clauses.push(DlClause {
                body: vec![Atom::Class(cache.fresh_q, X)],
                head: vec![Atom::Exists(Role::named(r), t, X)], // confirm Role ctor
            });
            n_exists += 1;
        }
    }
} else if mode == 3 {
    // garbage control: same count of random (r, named-target) ∃-clauses.
    // (pick deterministic "random": first n_exists_real (R,D) pairs not in facts.)
    // n_exists here mirrors the mode-2 count computed on a dry run; for the probe,
    // seed min(real_count, available) arbitrary in-vocabulary ∃-clauses.
}
// build engine EXACTLY like seed_probe (new + double_block + precise_card + mrv), run.
```
Use `HyperEngine::new(&clauses, cache.fresh_q)` (full index rebuild — the seed/∃ clauses must be indexed, the SP2.1 lesson). Confirm the `Role` constructor for a named role from a `RoleId` (grep `Role::` in `owl-dl-core`). Return `(result, stats, wall_ms, n_exists)`.

- [ ] **Step 5: Run the translation test — pass.** `cargo test -p owl-dl-reasoner precompletion_translates` → PASS.

- [ ] **Step 6: fmt/clippy/build green**

```sh
cargo test -p owl-dl-reasoner --lib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- [ ] **Step 7: Commit**

```sh
git add crates/owl-dl-saturation/src/lib.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(sat-precompletion): expose derived ∃-facts + precompletion_probe

<trailers>"
```

---

### Task 2: Gate — find the hard class, measure ∃-collapse, verdict (controller-run)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/precompletion_probe_gate.rs` (`#[ignore]`; mirrors `seed_probe_gate.rs`).
- Create (durable): `docs/sat-precompletion-probe-results-2026-06-26.md`.

- [ ] **Step 1: Find a hard non-collapsing wine class**

Harness over wine: for a set of candidate hard classes (the named-seed-collapsing set excludes SweetWine/Zinfandel; sweep e.g. `WhiteWine`, `Wine`, `RedWine`, `ItalianWine`, `Beaujolais`, `CabernetSauvignon`, … — resolve real IRIs first), run `precompletion_probe(mode=1, named-only)` at depth 256, adaptive OFF, 60 s. Pick the class with the **highest** branch count (the one named-seed leaves slow). Print the table.

```sh
RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release \
  --test precompletion_probe_gate find_hard_class -- --ignored --nocapture
```

- [ ] **Step 2: ∃-collapse measurement + controls on the chosen class**

For the chosen hard class, run modes 0 (none), 1 (named-only), 2 (named+∃), 3 (garbage-∃), each depth 256, adaptive OFF, 60 s. Record branches (disj/merge), `n_exists_seeded`, wall, verdict.

```sh
RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release \
  --test precompletion_probe_gate exists_collapse -- --ignored --nocapture
```

- [ ] **Step 3: Interpret + verdict**

`docs/sat-precompletion-probe-results-2026-06-26.md`: the hard-class table, the mode-0/1/2/3 numbers, and the **VERDICT**:
- **GO** iff mode-2 (named+∃) collapses the class order-of-magnitude below mode-1 (named-only) AND verdict stays `Sat` AND mode-3 (garbage-∃) does NOT collapse-to-correct-Sat. → spec Phase-2 (wire ∃-seeding into `classify_labels` + full-corpus FP=0/MISSED=0 gate + wine wall).
- **NO-GO** iff ∃-seed doesn't collapse, or flips the verdict (translation unsound). → the named-seed ~7.5% is the coupled-saturation wine ceiling; record it.

- [ ] **Step 4: Commit verdict + report to controller.** Do NOT merge / flip defaults.

---

## Notes for the implementer

- The translation `NomKey → individual → wedge nominal (num_classes + ind.index())` is the sound bridge (both engines key `{a}` by the same `IndividualId`); verify the wedge clausifier's `nominal_base == num_classes` so the id matches.
- Drop untranslatable targets (Tseitin/DKey) — sound under-approximation (fewer ∃-seeds, never a wrong one). Phase-1 soundness check = verdict preserved on the probed class.
- Use `HyperEngine::new` (full index rebuild) so the seed/∃ clauses fire — the SP2.1 lesson (amortized index leaves appended clauses inert).

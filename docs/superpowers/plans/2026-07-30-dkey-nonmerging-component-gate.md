# DKey Non-Merging Component Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `convert_ontology` from materialising O(k²) `DisjointClasses(DKey,DKey)` axioms for role components that contain no merge-inducing role, where the pairs can never be consumed.

**Architecture:** One gate inside `dkey_components` in `crates/owl-dl-core/src/convert.rs`: compute the set of union-find components containing at least one merge-inducing role (`m_star`), and have the step-(e) `anchor` closure skip components not in that set. DKeys left with no component fall into the skip branch `seed_disjoint_bucket` already has for keys that "can never reach a node label". Behind `RUSTDL_DKEY_MERGING_GATE`, default ON.

**Tech Stack:** Rust (edition 2024), `cargo test` / `clippy` / `fmt`.

**Spec:** `docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md`

**Starting point:** the gate is ALREADY prototyped and measured on this branch at commit `3aae033`
(`perf/dkey-nonmerging-component-gate`), but **unflagged and untested**. Do not re-derive it; add the
flag and the tests around it.

## Global Constraints

- **Toolchain.** `cargo` is NOT on `PATH` and `rust-toolchain.toml` pins a `cargo`-less 1.95.0. Every
  cargo command needs
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and
  `RUSTUP_TOOLCHAIN=stable cargo …`.
- **Warnings are errors.** CI sets `RUSTFLAGS: -D warnings`; clippy `pedantic` workspace-wide;
  `unwrap_used`/`dbg_macro` warn-level. `clippy::doc_markdown` is ON — a bare `ABox`, `TBox`,
  `DKey`, `HyperCache` in a `///` or `//!` comment is an **error**; backtick them.
- **Check real exit codes.** Never `cargo … | tail` then read `$?` — that reads `tail`'s status.
  Redirect to a file, then `echo $?`.
- **`owl-dl-py` does not build in this sandbox** (missing Python headers, pre-existing). Use
  `--exclude owl-dl-py` for workspace runs.
- **FP=0 is the repo's absolute invariant.** This change only REMOVES axioms, so it cannot create a
  false positive; the risk is a lost clash (a MISS). Treat any canary failure as a real completeness
  regression, never as a test to adjust.
- **The curated corpus cannot validate this area** — `datatype_value_membership.rs` says so itself.
  The FP=0 net shows inertness; the three canaries are the real gate.
- **Never rebuild a binary while a measurement is running against it.** That invalidated one scan in
  this session. Pin binaries to fixed paths and measure from those.

---

## File Structure

| file | responsibility |
|---|---|
| `crates/owl-dl-core/src/convert.rs` | `merging_comps` computation + the `anchor` gate + `dkey_merging_gate_enabled()` |
| `crates/owl-dl-core/src/convert.rs` (`#[cfg(test)] mod`) | the two boundary tests (count seeded axioms directly) |

---

### Task 1: Put the prototyped gate behind a flag

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` — `dkey_components` (the `merging_comps` binding added by `3aae033`, and the `anchor` closure), plus a new `dkey_merging_gate_enabled()` beside `bounded_dkey_disjoint_enabled()`.

**Interfaces:**
- Consumes: `m_star: Vec<bool>`, `uf: UnionFind`, both already in scope in `dkey_components`.
- Produces: nothing public. `dkey_merging_gate_enabled()` is private to the module.

- [ ] **Step 1: Add the flag reader next to the existing one**

Find `fn bounded_dkey_disjoint_enabled()` and add directly below it:

```rust
/// Non-merging-component gate (2026-07-30). **Default ON** — set
/// `RUSTDL_DKEY_MERGING_GATE=0` to seed disjointness for every role component,
/// including those that contain no merge-inducing role (the pre-2026-07-30
/// behaviour). Read per call so tests can toggle it.
///
/// A component with no merge-inducing role can never force two `DKey`s into one
/// node label, so its pairwise disjointness is unusable — see
/// `docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md`.
fn dkey_merging_gate_enabled() -> bool {
    std::env::var("RUSTDL_DKEY_MERGING_GATE").map_or(true, |v| v != "0")
}
```

- [ ] **Step 2: Make `merging_comps` an `Option` so the flag can disable it**

`3aae033` added an unconditional `merging_comps`. Change it to:

```rust
    // Components containing at least one merge-inducing role. A component with
    // none can never force two `DKey`s into ONE node label (`∃p.A ⊓ ∃p.B` has two
    // distinct successors), so seeding its pairs is dead weight. `None` ⟹ gate
    // off ⟹ every component is treated as merging (pre-2026-07-30 behaviour).
    let merging_comps: Option<HashSet<usize>> = dkey_merging_gate_enabled().then(|| {
        (0..num_roles)
            .filter(|&r| m_star[r])
            .map(|r| uf.find(r))
            .collect()
    });
```

Note `uf.find` takes `&mut self` in this codebase's `UnionFind` (path compression). If the borrow
checker objects to calling it inside the closure while `uf` is later borrowed mutably, build the set
with an explicit loop before the `anchor` closure is defined:

```rust
    let merging_comps: Option<HashSet<usize>> = if dkey_merging_gate_enabled() {
        let mut s = HashSet::new();
        for r in 0..num_roles {
            if m_star[r] {
                s.insert(uf.find(r));
            }
        }
        Some(s)
    } else {
        None
    };
```

Use whichever compiles; prefer the explicit loop if there is any doubt.

- [ ] **Step 3: Gate the `anchor` closure**

The closure body currently starts by computing `comp`. Make it:

```rust
        let comp = uf.find(role.role_id().index() as usize);
        if merging_comps.as_ref().is_some_and(|m| !m.contains(&comp)) {
            // Gate ON and this component has no merge-inducing role: the keys
            // under it can never be co-labelled, so leave them unanchored-and-
            // uncomponented. `seed_disjoint_bucket` already skips such keys
            // ("can never reach a node label"); this extends that skip to
            // "can never be CO-labelled".
            return;
        }
```

`is_some_and` gives the right semantics for both states: gate off (`None`) ⇒ never skip; gate on ⇒
skip iff the component is absent from the set.

- [ ] **Step 4: Build, fmt, clippy**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-core > /tmp/b.log 2>&1; echo "build rc=$?"
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check > /dev/null 2>&1; echo "fmt rc=$?"
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-core --all-targets --all-features -- -D warnings > /tmp/c.log 2>&1; echo "clippy rc=$?"
```

All three must be 0.

- [ ] **Step 5: Confirm the flag actually switches behaviour**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli > /dev/null 2>&1
P=/data/dumontier/ore-run/pool_sample/files
echo -n "gate ON  : "; ./target/release/rustdl tbox-stats $P/ore_ont_9347.owl | awk '/concept_rules/{print $3}'
echo -n "gate OFF : "; RUSTDL_DKEY_MERGING_GATE=0 timeout 300 ./target/release/rustdl tbox-stats $P/ore_ont_9347.owl | awk '/concept_rules/{print $3}'
```

Expected: ON → `113`; OFF → `49571087`. If OFF does not reproduce 49,571,087 the flag is not
restoring the old path — stop and report.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gate DKey disjointness seeding on merge-inducing components

RUSTDL_DKEY_MERGING_GATE, default ON. A role component with no merge-inducing role
can never force two DKeys into one node label, so its pairwise disjointness is
unusable; skip anchoring into it and let the existing 'can never reach a node label'
skip drop the keys. =0 restores the pre-2026-07-30 all-components behaviour.

Verified the flag switches: ore_ont_9347 concept_rules 113 (ON) vs 49,571,087 (OFF)."
```

---

### Task 2: Boundary tests that pin the gate directly

The three existing canaries prove the gate keeps the pairs that matter, but they test through a
*reasoning outcome*. These two test the gate's boundary at the conversion level, which is where the
decision is made — so a future regression is attributed immediately instead of surfacing as a
mysterious lost clash.

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` — add to the existing `#[cfg(test)] mod tests`.

**Interfaces:**
- Consumes: `convert_ontology`, and whatever OFN/`SetOntology` test helper the module's existing
  tests use — **read them first and reuse that helper** rather than inventing one.

- [ ] **Step 1: Read the module's existing test conventions**

```bash
grep -n "mod tests" crates/owl-dl-core/src/convert.rs
grep -n "fn parse\|SetOntology\|read_ofn\|fn onto(" crates/owl-dl-core/src/convert.rs | head -20
```

Match the established pattern. Also find how other tests count emitted axioms — there may already be
a helper that filters `Axiom::DisjointClasses`.

- [ ] **Step 2: Write the pair of tests**

The two fixtures must differ in **exactly one axiom** — the `FunctionalDataProperty` — so the test
isolates the gate rather than any other property of the input. Use ≥3 distinct data values so a
non-zero count is unambiguous (3 values ⇒ 3 pairs when seeded).

```rust
    /// Count `DisjointClasses` axioms whose operands are both synthetic `DKey`
    /// classes — i.e. the ones `seed_disjoint_bucket` mints.
    fn dkey_disjoint_count(onto: &InternalOntology) -> usize {
        onto.axioms
            .iter()
            .filter(|ax| matches!(ax, Axiom::DisjointClasses(cs) if cs.len() == 2))
            .filter(|ax| {
                let Axiom::DisjointClasses(cs) = ax else { return false };
                cs.iter().all(|&c| match onto.concepts.get(c) {
                    ConceptExpr::Atomic(cls) => onto
                        .vocabulary
                        .class_iri(*cls)
                        .is_some_and(|iri| is_dkey_iri(iri)),
                    _ => false,
                })
            })
            .count()
    }

    /// GATE BOUNDARY, negative side: three data values on a data property with NO
    /// merge-inducing characteristic. Nothing can put two `DKey`s in one node
    /// label, so ZERO disjointness pairs must be seeded.
    #[test]
    fn non_merging_data_property_seeds_no_dkey_disjointness() {
        let onto = convert_test_ontology(
            "    Declaration(DataProperty(:p))
     Declaration(NamedIndividual(:a))
     Declaration(NamedIndividual(:b))
     Declaration(NamedIndividual(:c))
     DataPropertyAssertion(:p :a \"1\"^^xsd:integer)
     DataPropertyAssertion(:p :b \"2\"^^xsd:integer)
     DataPropertyAssertion(:p :c \"3\"^^xsd:integer)",
        );
        assert_eq!(
            dkey_disjoint_count(&onto),
            0,
            "a non-merge-inducing data property must seed no `DKey` disjointness"
        );
    }

    /// GATE BOUNDARY, positive side: the SAME fixture plus one
    /// `FunctionalDataProperty` axiom. `p` is now merge-inducing, so the three
    /// values CAN be forced onto one node and all 3 pairs must be seeded.
    #[test]
    fn functional_data_property_still_seeds_dkey_disjointness() {
        let onto = convert_test_ontology(
            "    Declaration(DataProperty(:p))
     FunctionalDataProperty(:p)
     Declaration(NamedIndividual(:a))
     Declaration(NamedIndividual(:b))
     Declaration(NamedIndividual(:c))
     DataPropertyAssertion(:p :a \"1\"^^xsd:integer)
     DataPropertyAssertion(:p :b \"2\"^^xsd:integer)
     DataPropertyAssertion(:p :c \"3\"^^xsd:integer)",
        );
        assert_eq!(
            dkey_disjoint_count(&onto),
            3,
            "a functional data property is merge-inducing: all 3 pairs must survive"
        );
    }
```

`convert_test_ontology` is a placeholder for the module's real helper — substitute the one Step 1
found, and adjust the prefix/`xsd:` handling to match it. If no helper exists, write one following
the closest existing test.

If `is_dkey_iri` or `class_iri` are not visible from the test module, use whatever the surrounding
tests use to inspect the vocabulary; do NOT widen visibility to `pub` for a test.

- [ ] **Step 3: Run them**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core dkey_disjoint 2>&1 | grep -E "^test |test result"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core non_merging_data_property functional_data_property 2>&1 | grep -E "^test |test result"
```

Both must pass. If `functional_data_property_still_seeds_dkey_disjointness` returns 0, the
`FunctionalDataProperty` lowering is not reaching `m_star` — investigate rather than lowering the
expectation, because that would mean the gate is dropping consumable pairs.

- [ ] **Step 4: Prove the tests are non-vacuous — flip the flag**

```bash
RUSTDL_DKEY_MERGING_GATE=0 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core non_merging_data_property 2>&1 | grep -E "^test |test result"
```

Expected: `non_merging_data_property_seeds_no_dkey_disjointness` **FAILS** with the gate off (3 pairs
seeded, not 0). That is the proof the test measures the gate. If it passes with the gate off, the
test is vacuous — fix it before continuing.

- [ ] **Step 5: fmt, clippy, commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check > /dev/null 2>&1; echo "fmt rc=$?"
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-core --all-targets --all-features -- -D warnings > /tmp/c2.log 2>&1; echo "clippy rc=$?"
git add crates/owl-dl-core/src/convert.rs
git commit -m "test(convert): pin the DKey merging-gate boundary at conversion level

Two fixtures differing in EXACTLY one axiom (FunctionalDataProperty), so the test
isolates the gate: 3 integer values on a non-merge-inducing data property seed 0
DKey disjointness pairs; adding FunctionalDataProperty(:p) makes p merge-inducing
and all 3 pairs survive.

Non-vacuity verified: the negative test FAILS under RUSTDL_DKEY_MERGING_GATE=0."
```

---

### Task 3: Gates

**Files:** none (measurement). Results appended to the spec.

- [ ] **Step 1: Full workspace suite**

```bash
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py > /tmp/ws.log 2>&1; echo "rc=$?"
grep -E "test result: FAILED|^failures:" /tmp/ws.log | head
```

Expected rc=0, no FAILED.

- [ ] **Step 2: The three canaries, and the sabotage re-run (spec gate 2)**

```bash
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test datatype_value_membership 2>&1 | grep -E "^test forall.*clash|test result"
```

Expected: all pass, including `forall_value_outside_range_clashes`,
`forall_float_value_outside_clashes`, `forall_string_value_outside_enum_clashes`.

Then re-confirm the net still bites, by temporarily forcing the gate to skip EVERY component
(edit the `anchor` gate to `if true {` … `return; }`), rebuilding, and running the same command.
Expected: those **three** FAIL. Revert the edit immediately (`git checkout -- crates/owl-dl-core/src/convert.rs`).
If they do not fail, the evidence for this whole change is void — stop and report.

- [ ] **Step 3: FP=0 net (spec gate 3)**

```bash
./scripts/run-soundness-diff.sh > /tmp/fp0.log 2>&1; echo "rc=$?"
grep -E '^\[fp0\]|test result:' /tmp/fp0.log | tail -25
```

Expected: 22 passed / 0 failed, closures galen 27997, notgalen 32739, sio 8904, ore-10908 6001,
wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16, all FP=0/MISSED=0.
Record that this shows **inertness**, not correctness (see spec § Evidence).

- [ ] **Step 4: Flag-OFF byte-identity (spec gate 1)**

```bash
S=/tmp/claude-1007/-data-dumontier-rustdl/8e753f2f-e24e-4be2-8c66-c6e13e322bae/scratchpad
for f in ontologies/real/wine.ofn ontologies/real/family.ofn ontologies/real/pizza.ofn \
         ontologies/real/ro.ofn ontologies/external/alehif-test.ofn; do
  n=$(basename "$f" .ofn)
  RUSTDL_DKEY_MERGING_GATE=0 ./target/release/rustdl classify "$f" 2>/dev/null | grep -v '^#' | sort > "$S/$n.gateoff"
  cmp -s "$S/$n.pre.classify" "$S/$n.gateoff" && echo "IDENTICAL $n" || echo "***DIFF*** $n"
done
```

`$S/*.pre.classify` are pre-change `main` baselines already captured this session. All five must be
IDENTICAL — that proves the gate is the only behavioural delta.

- [ ] **Step 5: Per-ontology recovery from PINNED binaries (spec gate 4)**

Pinned copies already exist at `$S/rustdl-before` and `$S/rustdl-after`. **Do not rebuild while
measuring.**

```bash
P=/data/dumontier/ore-run/pool_sample/files
for n in 9347 5368; do
  for side in before after; do
    printf "  %s %-6s " "$n" "$side"
    /usr/bin/time -f "wall=%es rss=%MkB" timeout 700 env RAYON_NUM_THREADS=1 \
      "$S/rustdl-$side" classify "$P/ore_ont_$n.owl" >/dev/null
  done
done
```

Expected `9347`: before DNF ~600 s / ~70.7 GB, after ~11 s / ~0.23 GB. Expected `5368`: essentially
unchanged (it is genuinely merging).

- [ ] **Step 6: Record results in the spec and commit**

Append a `## Measured results (2026-07-30)` section to
`docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md`: the suite/canary/FP=0
outcomes, the flag-OFF byte-identity results, per-ontology recovery, the population before/after
counts (from `$S/pop.log`), and how much residual remains ≥1M after the gate — that residual is the
only justification for Lever 2, so state it precisely.

```bash
git add docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md
git commit -m "docs: measured results for the DKey non-merging component gate"
```

---

## Self-Review

**1. Spec coverage.**

| spec item | task |
|---|---|
| `merging_comps` gate | Task 1 Steps 2–3 |
| `RUSTDL_DKEY_MERGING_GATE` flag, default ON | Task 1 Step 1 |
| dedicated non-merging / merging tests (gate 6) | Task 2 |
| gate 1 flag-OFF byte-identity | Task 3 Step 4 |
| gate 2 three canaries + sabotage re-run | Task 3 Step 2 |
| gate 3 FP=0 net | Task 3 Step 3 |
| gate 4 per-ontology recovery, pinned binaries | Task 3 Step 5 |
| gate 5 population before/after | Task 3 Step 6 (scan already running) |
| out-of-scope: Lever 2 oracle, `definitely_disjoint`, `unanchored` | not touched by any task |

**2. Placeholder scan.** One deliberate placeholder remains: `convert_test_ontology` in Task 2 Step 2,
which Step 1 exists specifically to resolve against the module's real helper. It is flagged inline
rather than guessed, because inventing a parse helper that duplicates an existing one is worse than
looking. Everything else is literal code or a literal command with an expected value.

**3. Type consistency.** `merging_comps: Option<HashSet<usize>>` is used consistently in Steps 2 and 3
(`as_ref().is_some_and(...)` matches the `Option`). `dkey_merging_gate_enabled() -> bool` matches its
one call site. `dkey_disjoint_count(&InternalOntology) -> usize` matches both test call sites.
Task 3's `$S/rustdl-{before,after}` names match the pinned files created earlier this session.

Task 1's Step 2 deliberately offers two spellings for the same value because `UnionFind::find` may
take `&mut self`; the plan says which to prefer if the first does not compile, rather than leaving
the implementer to guess.

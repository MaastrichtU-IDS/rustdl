# R4 — `crates/owl-dl-reasoner` review findings

Binary: rebuilt from HEAD `509efc8` with
`PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin RUSTUP_TOOLCHAIN=stable cargo build --release`
(there is **no `cargo` on `$PATH` and none in `~/.cargo/bin`** on this host — the toolchain
binary must be addressed directly, otherwise the build silently does not happen).
All temporary instrumentation was reverted (`git checkout -- crates/owl-dl-reasoner/src/classify.rs`)
and the clean binary rebuilt + verified inert (`RUSTDL_GATE_PROBE=1` emits 0 lines).

Population: the 257 genuine-DNF stems in
`/data/dumontier/owl-reasoner-harness/baselines/2026-08-01-dnf257-list.txt`
over `/data/dumontier/ore-run/pool_sample/files/`.

Two instruments were used, both proved to fire before being trusted:
* **timestamped `RUSTDL_TRACE_RSS=1` phase markers** — `entry` / `after_saturate` /
  `before_prepared` / `after_prepared` / `after_label_cache`, piped through
  `perl -MTime::HiRes=time`. Fires on every run; gives a phase breakdown even on a DNF.
* **a temporary `RUSTDL_GATE_PROBE` histogram** inserted after `saturate()` in
  `classify_top_down_internal`, tallying which axiom *kinds* `is_el_axiom` /
  `is_saturator_axiom` reject, TBox-only and overall. Proved correct on three fixtures
  with known answers (`t1.ofn` pure-EL → `{}`; `topbot.ofn` with one `∀` → `{SubClassOf:1}`;
  `ore_ont_10019` → `{EquivalentClasses:19, Symmetric:5}`) before the 257-ontology sweep.
  **This is a gate probe on the converted IR, not a grep on the source.**

---

## 1. CONFIRMED — inefficient/incorrect — the classify budget bounds only the *search*; the *build* is unbudgeted

`crates/owl-dl-reasoner/src/classify.rs:1588` (`saturate(internal)`) and
`crates/owl-dl-reasoner/src/classify.rs:1635`
(`PreparedOntology::from_internal(internal.clone())`) both run **before any deadline is
consulted** and neither takes a deadline argument. The first `global_deadline` test in the
whole function is inside the label-cache loop (`classify.rs:1699`). Same shape on the
naive path (`classify.rs:758` / `classify.rs:810`).

**Measured** (single-thread, `ulimit -v 16 GB`):

| ontology | `--global-timeout-ms` | conversion | `saturate` | `from_internal` | label cache | total |
|---|---|---|---|---|---|---|
| `ore_ont_1028` | **3000** | 0.14 s | 2.30 s | 4.66 s | 0.01 s | **7.17 s** |
| `ore_ont_10140` | **3000** | 1.65 s | 2.45 s | 5.63 s | 0.12 s | **10.99 s** |
| `ore_ont_10926` | **1** | 23.8 s | 14.1 s | 34.7 s | – | **84.9 s** |
| `ore_ont_11270` | **1** | 1.0 s | 24.3 s | (never finished) | – | **93.5 s** |

**Population** (all 257, `--global-timeout-ms 1`, i.e. a 1 ms promise; 252 produced data):

* **77 / 252 (31 %)** still burn **≥ 10 s**
* **34 (13 %)** ≥ 30 s
* **11 (4.3 %)** ≥ 60 s
* **26 (10 %)** never reach the `after_prepared` marker within 125 s — `from_internal`
  *alone* is the DNF.

**Accounting bug that hides it:** `stats.tier_walk_wall_ms` is computed at
`classify.rs:2380` as a *residual* —
`total_wall − label_cache − snapshot_build − snapshot_replay`. Everything unmeasured
(conversion-to-`saturate`, `from_internal`, the unsat probes, both sweeps, the
entailment-matrix BFS) is silently attributed to "tier walk". That is the reported
"~94 % of the overrun unattributed": it is not unattributed, it is *mis*-attributed by
construction. On `ore_ont_1028` the banner reports `tier_walk=7198` for a run whose actual
tier walk was 0.08 s.

**Falsifiable prediction:** for every one of the 26 "never reached `after_prepared`"
ontologies, *any* `--global-timeout-ms` value — including 1 — still DNFs at 120 s. A
deadline check threaded into `saturate` / `from_internal` (or, minimally, an
`Instant::now() >= gd` bail between `classify.rs:1588` and `:1635` and again before
`:1635`) converts them from DNF to a sound partial answer.

---

## 2. CONFIRMED — inefficient — hybrid classify runs the **same EL saturation three times**; two are removable, one is provably dead

Three full saturations of the *identical, pre-mutation* ontology per hybrid classify:

1. `classify.rs:1588` — `saturate(internal)`, the closure the tier walk uses.
2. `lib.rs:4662` — `owl_dl_saturation::saturate(&internal)` inside
   `PreparedOntology::from_internal`.
3. `lib.rs:2329` — `owl_dl_saturation::saturate_with_exists_facts(&internal)` inside
   `HyperCache::build` (`RUSTDL_SAT_SEED`, **default ON**), which returns a *superset* of
   what (1) and (2) compute.

`#2 is dead work on an ABox-free ontology.` `PreparedOntology.closure` is read at exactly
one site — `lib.rs:4799`, building `AboxCheckInputs` inside `abox_verdict()` — which is
`get_or_init`-lazy *and* early-returns `Unknown` when there are no individuals. The
in-code comment at `lib.rs:4658` claims "on ABox-free ontologies, abox_check exits early
before querying the closure, so the cost is amortised" — but the closure is computed
**eagerly**, so the early exit saves nothing. **122 of the 257 DNF ontologies are
ABox-free** (gate probe, `abox=false`).

**Measured, intervention (not arithmetic):** `ore_ont_1028`, `RUSTDL_SAT_SEED=1` vs `=0`
— `from_internal` 4.60 s → 2.28 s, i.e. **−2.32 s against a one-saturation cost of
2.30 s**: `saturate_with_exists_facts` is exactly one extra full saturation. The
residual 2.28 s is (2) plus absorb/clausify/index-build.

**Measured, population:** across the 115 DNF ontologies whose saturation took > 0.5 s,
`from_internal / one-saturation` has **mean ratio 2.25** (median ≈ 2.0) — i.e.
`from_internal` ≈ two saturations plus ~0.25 of other work, exactly as the three call
sites predict.

**Falsifiable prediction:** computing `saturate_with_exists_facts` once and threading its
`subs` into both other consumers removes ~2/3.25 ≈ **62 % of the entire pre-classify
build** on all 257 DNF ontologies and on every hybrid-path classify. Specifically
`ore_ont_1028` 7.17 s → ~2.6 s, `ore_ont_10926` 84.9 s → ~40 s, and several of the 26
`from_internal`-DNFs should complete their build.

---

## 3. CONFIRMED — incorrect (D10 class, NEW) — `is_el_concept` admits `⊥` under an `∃` filler, which the saturator drops

`crates/owl-dl-reasoner/src/classify.rs:1191` gives `is_el_concept` a `ConceptExpr::Bot`
arm; `classify.rs:1193` recurses through `Some(role, body)`. So
`X ⊑ ∃r.⊥` is admitted by `is_pure_el`. `∃r.⊥ ≡ ⊥`, so `X` is unsatisfiable — the EL
saturator has no rule for it. Note `is_saturator_concept` (`classify.rs:1417`) has **no**
`Bot` arm, so the two sibling gates disagree; only the `is_pure_el` arm is exposed.

The Lever-1b justification comment at `classify.rs:1185–1190` only argues the *top-level*
positions (`X ⊑ ⊥`, `A⊓B ⊑ ⊥`); it does not cover the nested `Some`-filler position the
recursion reaches.

**Reproduced** (fixture `scratchpad/t1.ofn`):

```
SubClassOf(:X ObjectSomeValuesFrom(:r owl:Nothing))
```
→ `# fragment: pure-EL (trust_sat sound by construction; saturator alone is complete)`,
`"unsatisfiable": []`, **`"incomplete": false`**.
Adding one unrelated `ObjectAllValuesFrom` axiom forces the hybrid path, which
correctly reports `unsat http://e.org/X`. Same miss for
`∃r.(A ⊓ ⊥)` and for the `EquivalentClasses` form (which additionally loses `Y ⊑ ⊥` for
`Y ⊑ X`). The control `X ⊑ A ⊓ ⊥` *is* caught, isolating the hole to the `Some` filler.
`rustdl sat t1.ofn X` also answers `sat` (`lib.rs:3877`, same `is_pure_el` gate).

Other conversion routes into this shape: empty `ObjectUnionOf` (`convert.rs:869`) and
empty `ObjectOneOf` (`convert.rs:885`) both lower to `pool.bot()` and can sit under an
`∃`. (`DataSomeValuesFrom` with an empty range is **safe** — `convert.rs:964` returns
`pool.bot()` for the whole expression, not `∃p.⊥`.)

**Prevalence — honest:** a grep of the ORE pool finds 8 files with `Nothing` under a
`someValuesFrom`; running all 8 shows **0 currently take the pure-EL path** (each has
other out-of-fragment axioms). The hole is therefore **latent, not firing on ORE today**.
It goes live for any otherwise-EL ontology containing the pattern. Given the project's
stated ordering (correctness > performance, and this is the fourth instance of the D10
class), it is reported first among correctness items.

**Falsifiable prediction:** removing `ConceptExpr::Bot` from the `Some`-filler recursion
in `is_el_concept` changes no ORE or curated closure (0/8 candidates reach the gate) while
making `t1/t2/t3.ofn` report `X` unsatisfiable.

---

## 4. CONFIRMED — incorrect — `classify --json` reports `"consistent": true` on an inconsistent KB, on both non-fast paths, including the `family.ofn` corpus fixture

`stats.inconsistent` is set in exactly two places: `classify.rs:982`
(`closure.globally_inconsistent() || closure.top_is_unsat()` — **inside
`classify_pure_el` only**) and `classify_inconsistent` (`classify.rs:1056`, reached only
from the A1 `abox_check` verdict). Neither `classify_top_down_internal` nor
`classify_internal_with_timeout` applies the `top_is_unsat` test, although the `closure`
is in scope at `classify.rs:1588` / `:758`. Additionally, **`abox_saturation` does not
appear anywhere in `classify.rs`** — the default-ON `saturate_abox_consistency`
pre-check (the fix that made `family` detectable, wired into `is_consistent`, `realize`,
`materialize_*`, `justify`) is not wired into `classify`.

Two reproductions:

* Synthetic (`scratchpad/topbot.ofn`): `⊤ ⊑ E`, `E ⊑ ⊥`, plus one `∀` axiom to force the
  hybrid path → `"consistent": true`, `"incomplete": false`, `unsatisfiable` = all four
  classes. Delete the `∀` axiom (pure-EL path) → `"consistent": false`. Same file:
  `rustdl consistent topbot.ofn` → **`inconsistent`**.
* Corpus: `rustdl consistent ontologies/real/family.ofn` → **`inconsistent`**;
  `rustdl classify --json ontologies/real/family.ofn` → `"consistent": true`,
  `"incomplete": false`, `"unsatisfiable": []`, plus a full hierarchy. On an inconsistent
  KB every class is unsatisfiable and every subsumption holds.

This is the SP1.1 shape — an inference reachable from one oracle (`is_consistent`) and not
from another (`classify`) — *and* a case where the `incomplete` signal fails to fire on a
wrong answer. Not an FP-subsumption (the hierarchy under-reports), but a false
consistency claim on the machine-readable contract.

**Falsifiable prediction:** applying `closure.globally_inconsistent() || closure.top_is_unsat()`
at `classify.rs:1588`, and running the `has_abox_axioms`-guarded
`abox_saturation::saturate_abox_consistency` pre-check before `classify.rs:1635`, flips
`family.ofn` and `topbot.ofn` to `"consistent": false` and is inert (byte-identical) on
every consistent fixture. Cost on family is ~0.7 s (post-2026-07-23 indexing) and zero on
ABox-free inputs.

---

## 5. CONFIRMED — inefficient (dispatch, Lever-1 class) — `SymmetricRole` / `InverseObjectProperties` **declarations** alone push 71 of the 257 DNF ontologies off the saturation fast path

`is_el_axiom` (`classify.rs:1132`) and `is_saturator_axiom` (`classify.rs:1312`) both fall
to `_ => false` for `Axiom::SymmetricRole` and `Axiom::InverseObjectProperties`.

**Gate probe over all 257** (245 produced a verdict; 12 DNF before reaching the gate).
Ontologies whose *entire* TBox rejection set is these two kinds:

| sole rejecting kind(s) | count |
|---|---|
| `Symmetric` only | **36** |
| `InverseObjProps` only | **24** |
| both, nothing else | **11** |
| **total** | **71 / 257 (28 %)** |

**Rescue confirmed by intervention** — deleting the 5 `SymmetricObjectProperty` lines
(nothing else) from three of the 36:

| ontology | as-shipped | symmetric declarations removed |
|---|---|---|
| `ore_ont_8470` | DNF @ 120 s (hybrid) | **0.53 s**, `mode: pure EL` |
| `ore_ont_11500` | DNF @ 120 s (hybrid) | **0.86 s**, `mode: pure EL` |
| `ore_ont_16283` | DNF @ 120 s (hybrid) | **0.75 s**, `mode: pure EL` |

**Inertness verified on one:** `ore_ont_8470` run as-shipped with
`--global-timeout-ms 60000` produces 19 578 `direct`/`equiv`/`unsat` lines that are
**byte-identical** to the 0.53 s stripped run (`comm -23` → 0 rows). The symmetric
declarations contribute exactly zero subsumptions; rustdl spends >120 s failing to produce
what it can produce in half a second.

**This corroborates the project's own prior measurement** (memory
`backward-prop-symmetric-nogo`): of 235 real ORE symmetric ontologies, **211 are
"trivially inert" — the symmetric role is never an antecedent `∃R` trigger — and zero
confirmed real symmetric classify misses exist anywhere in the corpus.** That NO-GO was
about building a backward-propagation *engine*; it is silent on the *gate*, and it supplies
exactly the inertness criterion the gate would need.

**Soundness caveat — must not be skipped.** Admitting these kinds *unconditionally* would
be a new D10 instance: `Symmetric(r) + Domain(r,A) ⟹ Range(r,A)`, and
`InverseObjectProperties(p,q) + Domain(p,A) ⟹ Range(q,A)`; the saturator derives neither.
The sound form is a per-role side condition (role never used in an `∃`-antecedent trigger
position and carrying no domain/range axiom), matching the "trivially inert" test. The
71 is therefore an **upper bound**; the deliverable set is 71 × (inert fraction), which
the prior measurement puts near 90 % for the symmetric half.

---

## 6. CONFIRMED — inefficient — the label-cache build has **no aggregate bound**, and its per-class budget is 30× the user's `--pair-timeout-ms`

`classify.rs:1685` calls `adaptive_label_cache_ms(n, per_pair_timeout, override)`
(`lib.rs:1913`) = `clamp(n × per_pair, 1 s, 30 s)`. On the CLI default
(`--pair-timeout-ms 1000`, `--global-timeout-ms 0`) that is **30 s per class** for any
`n ≥ 30`, and `global_deadline` is `None`, so the `(0..n).into_par_iter()` loop at
`classify.rs:1691` has **no aggregate cap at all** — worst case 30 s × n.

**Measured** — `ore_ont_10125` wait, `ore_ont_10517` (prep = 2.4 s):

| config | time inside the label-cache loop |
|---|---|
| default | ≥ 298 s (killed at 300 s) |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=1000` (per class = the pair budget) | ≥ 298 s (killed at 300 s) |

The per-class bound is honoured; the *aggregate* is not. This is the plausible root of the
reported "10–27× per-class budget overrun": the user-visible knob is
`--pair-timeout-ms 1000` and the label cache is allowed 30 s per class on top of an
unbounded total.

**Falsifiable prediction:** ontologies whose phase markers show a small `after_prepared`
but never reach `after_label_cache` (`ore_ont_10517`, `ore_ont_10109` — both prep < 0.03 s,
both 300 s DNF) complete under an aggregate label-cache cap; `ore_ont_1028`-class
ontologies (prep-bound) are unaffected.

---

## 7. CONFIRMED — inefficient — 8+ of the 257 "DNF" ontologies are **not reasoning DNFs at all**: classify returns and `Classification::direct_subsumers` DNFs

`ore_ont_10125`, stdout line-timestamped with `stdbuf -oL`:

```
 15.231  # classes: 73449
 15.231  # mode: pure EL (saturation-only)
 15.231  # wall breakdown ms: label_cache_build=0 ... tier_walk=0
 15.374  line 20000
 15.777  line 100000        <- ~200k lines/s
222.330  line 120000        <- ~100 lines/s
400.035  END n=155463 (still not finished)
```

Classify finishes in **15.2 s**; the remaining ≥385 s is the CLI's per-class
`h.direct_subsumers(c)` loop (`crates/owl-dl-cli/src/main.rs:818–823` calling
`crates/owl-dl-reasoner/src/classify.rs:469`). The stall is *inside* the `direct` loop
(100 k `direct` lines already emitted), and `writeln!` is not the cost (the first 100 k
lines took 0.5 s), so it is `direct_subsumers` itself. Only 6 classes are unsatisfiable,
so the documented `(0..n)` degenerate arm is not involved; the only super-linear term left
is the Hasse prune `strict_supers.iter().filter(|j| !strict_supers.iter().any(...))` —
**O(k²) `entails` calls per class, recomputing a transitive reduction independently for
every class** (`RUSTDL_ABOX_CHECK=0` changes nothing; forcing the dense arm with
`RUSTDL_CLASSIFY_DENSE_MAX=100000` gets 33 % further in the same 400 s and still DNFs,
consistent with removing only the sparse `binary_search` log factor).

Of the 9 DNF ontologies whose TBox is already in the saturator fragment
(`sat_tbox_reject={}`), **all 9 print `# mode: pure EL (saturation-only)` and then DNF at
150 s**: `10125`, `11315`, `11899`, `12351`, `12567`, `14861`, `4598`, `7706`, `7868`.
9/9 — the dispatch is already correct for this whole group; the DNF is downstream of it.

**Falsifiable prediction:** a single global transitive-reduction pass (topological order
over the entailment matrix, O(Σk·δ) instead of O(n·k²)) converts these 8–9 from DNF to
completing, with byte-identical output. They will *not* respond to any engine, gate, or
budget change.

---

## 8. SUSPECTED — inefficient — `classify_labels` unconditionally discards the v0.3.39 clause-index amortization

`lib.rs:2958`: `let mut engine = if self.sat_seed.is_some() || self.exists_seed.is_some()
|| self.value_disjoint.is_some() { HyperEngine::new(&clauses, …) } else {
HyperEngine::new_with_prebuilt(…) }`. `HyperEngine::new` (`hyper.rs:1241`) rebuilds
`build_disjoint_pairs(clauses)` **and** `build_clause_indexes(clauses, None)` from
scratch — O(#clauses), **once per class**. Because `RUSTDL_SAT_SEED` is **default ON**
(`lib.rs:1608`), `sat_seed.is_some()` is always true, so the amortized branch is
**dead code in production**. Its per-*pair* sibling `decide_with_stats` (`lib.rs:2653`)
solves exactly this with `build_clause_index_delta` + `new_with_prebuilt_extras`
(`lib.rs:2740–2755`); `classify_labels` simply does not use it.

**Measured**, `ore_ont_10080` (n = 3533), `RUSTDL_LABEL_CACHE_TIMEOUT_MS=1` so each
per-class probe returns `Stalled` immediately and the number measures *setup only*:

| | `label_cache_build` |
|---|---|
| `RUSTDL_SAT_SEED=1` (full-rebuild branch, the default) | **19 987 ms** (5.66 ms/class) |
| `RUSTDL_SAT_SEED=0` (`new_with_prebuilt` branch) | **6 414 ms** (1.82 ms/class) |

**Confound, stated:** `SAT_SEED=0` also removes the seed clauses themselves, so the
13.6 s delta is index-rebuild *plus* seed-clause push/clone, not index-rebuild alone. The
6.4 s baseline is the `self.clauses.clone()` the prompt already measured at 0.55–6.3 % of
CPU; the extra 13.6 s is new. Marked SUSPECTED because the split was not isolated — but
the structural claim (the amortized path is unreachable at default settings) is certain
from the code, and a delta path already exists three hundred lines away.

**Falsifiable prediction:** routing `classify_labels` through
`build_clause_index_delta` + `new_with_prebuilt_extras` (identical to `decide_with_stats`)
cuts `label_cache_build` by ~2–3× on every wedge-classified ontology with a large clause
set; `ore_ont_10080` 20.0 s → ~7 s. Verdict-preserving by the same argument v0.3.39 used.

---

## 9. CONFIRMED — missing — the `saturator_complete_fragment` / Lever-1 fast paths are wired to `classify` and `realize` but **not** to `sat` / `subclass` / `consistent`

* `is_class_satisfiable_internal_full` (`lib.rs:3877`) gates only on
  `classify::is_pure_el`.
* `is_subclass_of_internal_full` (`lib.rs:4155`) gates only on `classify::is_pure_el`
  (and then builds a whole `HyperCache` per call).
* `is_consistent_internal_full` (`lib.rs:3959`) has no saturation gate at all and never
  consults `closure.top_is_unsat()`.

`classify` (`classify.rs:1601`) and `realize` (`realize.rs:737`) all three use
`is_pure_el || saturator_complete_fragment || tbox_only_saturator_eligible`. So GALEN-class
(EL + functional) and Lever-1-class (EL TBox + big nominal-free ABox) ontologies get the
proven-complete saturation answer from `classify` but the full tableau from `sat` /
`subclass`.

**Measured**, `ore_ont_10068` (19.8 MB, `mode: pure EL` via the non-`is_pure_el` arm):
`classify` **2.52 s** vs `sat <one class>` **3.50 s** — the single-class query is slower
than classifying every class. Modest here because the tableau happens to terminate; the
exposure is unbounded on any ontology where it does not.

Minor sibling: `realize_saturation_eligible` (`realize.rs:737`) calls
`tbox_only_saturator_eligible` **without** the `crate::classify_tbox_fragment_enabled()`
guard that both classify sites use, so `RUSTDL_CLASSIFY_TBOX_FRAGMENT=0` does not disable
Lever 1 for realize — an A/B-isolation hole, not a correctness one.

---

## Explicitly NOT re-reported

* `let mut clauses = self.clauses.clone()` at `lib.rs:2896` as a DNF lever — measured
  above at 1.82 ms/class / 6.4 s of a 120 s run, matching the 0.55–6.3 % already known.
  Finding 8 is about the *index rebuild* on the following line, not the clone.
* `RUSTDL_CLASSIFY_SAME_TIER` — the tier walk's same-tier blind spot is real but is
  already compensated for the *closure-derivable* cases by the closure seed at
  `classify.rs:2340` ("catches same-tier equivalences … the walk only looks at placed
  classes"), and SP1.1 already characterised the residual as corpus-invisible at ~2× wall.
  No new evidence found; not worth a finding line.

## Reproduction artefacts

`scratchpad/t1.ofn`, `t2.ofn`, `t3.ofn`, `t4.ofn` (finding 3);
`scratchpad/topbot.ofn`, `topbot_el.ofn` (finding 4);
`scratchpad/nosym_ore_ont_{8470,11500,16283}.ofn`, `orig_partial.txt`, `strip_full.txt`
(finding 5); `scratchpad/probe/*.prep`, `all_prep.txt` (finding 1/2);
`scratchpad/probe/gates.txt` (findings 2/5/7).

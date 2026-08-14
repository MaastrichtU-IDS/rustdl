# The `unsat_probe` DNF cluster: two mechanisms, both search-bound

**Date:** 2026-08-14 · Follow-on from the four-way peer comparison, which found Konclude
solves **100%** of the `unsat_probe` and `saturate` buckets while 32-way parallelism recovers
**0**.

## Shared signature: classification fails before the classifier starts

All four members show the same three facts, each independently measured (banner forced with
`--global-timeout-ms`, since none terminates):

| | `934` | `8273` | `7828` | `10517` |
|---|---|---|---|---|
| classes | 108 | 316 | 831 | 904 |
| `label_cache_build` | 16.6 s | 23.4 s | 1.9 s | 2.1 s |
| classes the label cache decides | **0** | **0** | **0** | **0** |
| `unsat_probe` | fills budget | fills budget | 198 s / 200 s | 198 s / 200 s |
| `tier_walk` | **never runs** | **never runs** | **never runs** | **never runs** |
| Konclude | 0.46 s | 0.29 s | 0.10 s | 0.41 s |

1. **The label cache decides zero classes** (`satisfiability probes: saturation=0
   tableau=N`). On `934`, `RUSTDL_LABEL_HEURISTIC=0` leaves the wall unchanged and moves its
   16.6 s into `unsat_probe` — on these ontologies the cache is pure overhead.
2. **`unsat_probe` expands to fill whatever budget exists.** Each per-class probe receives the
   *global* deadline via `effective_deadline`, so at the default (1000 ms per pair) every class
   burns its full second: `934` ⇒ ~108 s, matching the census's 103,541 ms exactly.
3. **`tier_walk` never starts** — `subsumption: saturation=0 tableau=0`. Not one pair is ever
   compared.

`--pair-timeout-ms` does not help: at 50 ms the cost relocates to `tier_walk` (831 classes ⇒
up to ~345k pairs), so this is not a budget-tuning problem.

## Mechanism A — ∀-disjunctive (`934`, `8273`)

Covered in `docs/2026-08-14-ore934-in-fragment-alch-blowup.md`. A **604-line pure-ALCH core**
of `ore_ont_934` still DNFs at 180 s where Konclude takes 0.09 s; six single-construct
ablations all still DNF, so it is the ∀/∃/⊔ *combination*. Profile ~33% in `apply_role_rules`.

## Mechanism B — no `∀`, no disjointness, still unbounded (`7828`, `10517`)

These carry **zero `ObjectAllValuesFrom`** and **one `DisjointClasses`** each. With essentially
nothing to clash against, a *satisfiability* probe should be trivial. It is not.

**Refuted hypothesis:** unbounded ∃-generation defeating ancestor-scoped pair-blocking.
`RUSTDL_ANYWHERE_BLOCKING=1` changes nothing (both still DNF at 120 s). Recorded because it is
the intuitive first guess and it is wrong.

**What `RUSTDL_TRACE=1` shows** (20 s window inside `unsat_probe` on `7828`):

```
# trace search depth=122 disj node=67 options=2 graph_nodes=415
# trace branch depth=121 my_id=135 pick=1/2 disj=462
```

* **7,178 search + 7,156 branch events in 20 s ⇒ ~358 branches/s**
* **`graph_nodes` STABLE**: mean 447, range 298–607, and the mean is 441/451/450/446 across
  the four quartiles of the window. The graph oscillates in a band; it does **not** grow.
* depth ≈ 122, `options=2`, always `pick=1/2`
* **per-branch cost ≈ 2.79 ms on a ~450-node graph**

At a 1000 ms budget that window is ~20 class probes, i.e. **~357 branches per class before the
budget expires, and none concludes.**

So this is **search-bound, not model-bound** — the same "thrashing through a tiny state set at
stable node count" signature the adaptive-budget work documented, here on the MAIN TABLEAU
rather than the wedge. Two costs compound:

* **Per-branch cost is ~2.79 ms**, which is 1–2 orders of magnitude above what a 450-node graph
  should need. The profile is consistent with per-branch work proportional to graph size
  (`apply_role_rules` 19.6%, `apply_deferred_concept_or_rules` 5.6%, `apply_concept_rules`
  4.0%, `SmallVec::clone` 4.2%, `hash_one` 5.5%, `_int_free` 4.0%).
* **The search does not converge** in ~357 branches at depth 122. With no ∀ and no
  disjointness the first disjunct should always succeed, so descending to depth 122 without
  concluding suggests each ∃-generated successor introduces fresh disjunctions — the decision
  count grows with the model rather than being bounded by the TBox.

### A real but INSUFFICIENT lever found here

`concrete_domain_clash` is the **top frame at 14.78%** on `7828` (7.6% on `934`). It already
early-outs on `dkey_ranges.is_empty()` — but these ontologies have 85/153
`DataPropertyAssertion`, which lower to `∃dp.DKey`, so `dkey_ranges` is non-empty. And they have
**zero** `DataSomeValuesFrom`, `DataAllValuesFrom`, `FunctionalDataProperty` and
`DataPropertyRange`, so **nothing can consume a DKey**: every accumulator is necessarily empty
and `card_sat` trivially succeeds. It is doing 14.78% of the work for a provably empty result.

The sound early-out is "no DKey **consumer** exists", one layer up from the shipped
`RUSTDL_DKEY_MERGING_GATE`, which applies exactly this reasoning to disjointness seeding.

**But 14.78% is not a rescue.** `7828` DNFs at 400 s and needs >4×; deleting the frame entirely
cannot complete it. Recorded as a genuine inefficiency, not a fix.

Relevant history, stated so it is not mis-read: a `concrete_domain_clash` fast path was built on
2026-08-13, **measured zero, and reverted** — but that was measured on `tier_walk`, and that
experiment **skipped its own fires-check**, so it is a weak null that does not transfer to
`unsat_probe`. Any retry must prove the guard fires by a criterion declared in advance.

## What this changes about the tail's characterisation

The 2026-08-13 peer triage concluded the residual is "algorithmic, not constant-factor",
citing a **flat** `tier_walk` profile (largest area 14%, five ~10% slices). That still holds
for `tier_walk`. It does **not** describe this cluster: here there is a top frame at 14.78%
that is provably dead work, a per-branch cost 1–2 orders of magnitude too high, and a
stable-node-count search signature. Different phase, different profile shape, different
levers.

## Honest status

* Mechanism A has a committed minimal reproducer (`docs/ore934-pure-alch-core.ofn`) and meets
  the CB arc's documented reopen criterion — but the construct census found **zero** strictly-ALCH
  members in the tail, so a pure-ALCH engine has no direct market. See that document.
* Mechanism B is **characterised but not root-caused**. I know it is search-bound at stable
  node count with a ~2.79 ms branch cost, and that blocking is not the answer. I do not know
  why the search fails to converge with no ∀ and no disjointness present, and that is the
  question to answer next.
* Neither mechanism is fixed. Nothing here is a shipped change.

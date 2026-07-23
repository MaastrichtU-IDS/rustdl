# Plan: amortize the per-pair ClauseIndexes rebuild in classify's subsumption oracle

**Status:** proposed (scoped 2026-07-23), for advisor review then delegation to Fable.
**Author:** Claude, 2026-07-23. Session: issue-#35 perf arc — the wedge-classify throughput frontier.
**Branch (to create):** `perf/classify-clauseindex-amortize`.

## 1. Problem (measured this session)

`sample`-profiling two **converging, wedge-heavy** classify runs
(`ore_ont_1508` 12.9 s / 13,950 subs; `ore_ont_12698` 5.6 s) — `build_clause_indexes`
is **11–15 % of self-time**, plus `ClauseIndexes` drop (~2–4 %) and a large
malloc/free/memmove tail (the index + clause-vector build/drop churn).

**Direct instrumentation (call counter):** `build_clause_indexes` runs
**13,772 times** during one `ore_ont_1508` classify (≈ once per decided pair),
each over **~34,593 clauses**. It is **not amortized** on the subsumption path.

Root cause, code-grounded in `HyperCache::decide_with_stats`
(`crates/owl-dl-reasoner/src/lib.rs:2571–2636`), per decided pair:
1. `let mut clauses = self.clauses.clone();` (2573) — clones the full ~34k-clause
   base vector, O(#clauses)/pair.
2. appends a few per-pair clauses: `Q→sub` (2574), optional `sat_seed`/`exists_seed`
   /`value_disjoint`, and the `¬sup` clause (2619–2629).
3. `HyperEngine::new(&clauses, self.fresh_q)` (2636) → **rebuilds the full
   ClauseIndexes over all ~34k clauses**, O(#clauses)/pair.

The amortization machinery already exists and the **label-oracle** path uses it:
`HyperCache::build` builds `base_indexes` **once** (Arc, with the single Q-clause
delta pre-applied, 2477–2487); `classify_labels` builds engines via
`HyperEngine::new_with_prebuilt` + `with_sub_roles_keep_index` (2797–2824),
reusing the shared index with **zero per-probe index work**. The **subsumption
`decide` path never got this** — because it appends *several* per-pair clauses,
whereas `new_with_prebuilt`'s `extra_clause` mechanism handles exactly **one**
(the Q-clause). So `decide` falls back to clone-all + rebuild-all.

`with_sub_roles` (2644) is SP1.1-gated (`classify_same_tier_enabled`, default
OFF), so in the default config the rebuild is `HyperEngine::new` itself, every
pair — not the sub-roles rebuild.

## 2. Goal & non-goals

**Goal:** reuse the shared `base_indexes` (Arc, O(1) clone) on the subsumption
`decide` path and apply only a **small per-pair index delta** for the handful of
appended clauses (O(#extra), not O(#clauses)); and avoid the full base-clause
`Vec` clone. Verdict-/closure-identical. Target: remove the 11–15 %
`build_clause_indexes` self-time + its drop/malloc tail → realistically 15–25 %
off wedge-heavy converging classify walls (1508/12698 and similar), **broadly**
(every wedge-classified ontology).

**Non-goals:**
- NOT the fire loop (`enumerate_matches`/`match_body`, ~25 %) — the maintainer's
  in-flight `match_body`/`fire_clause` plans; disjoint from this.
- Do NOT change clausification, the Q-clause semantics, or verdicts.
- SP1.1 `same_tier` path (`with_sub_roles`, default OFF) — keep it correct
  (fall back to the current rebuild when on, or extend `with_sub_roles_keep_index`),
  do not silently break it.

## 3. Phase 0 — measurement GATE (largely done; formalize)

- **GO (held):** 13,772 rebuilds × ~34k clauses measured on 1508;
  `build_clause_indexes` 11–15 % self-time on two converging wedge-heavy runs.
- **P0 deliverable:** an A/B spike (escape-hatch `RUSTDL_CLASSIFY_AMORTIZE_IDX=0`
  to force the old clone+rebuild) showing the classify wall delta on 1508 / 12698
  / GALEN / sio. **GO if** ≥ ~10 % wall improvement on ≥2 of them with zero
  closure diff. **NO-GO** if the delta is under noise (e.g. the clause-clone /
  drop dominate and index reuse alone doesn't move the wall — then re-aim at the
  clone).
- Confirm the clause-vector **clone** cost separately (it may be a comparable
  chunk of the malloc tail) — the fix should remove both the clone and the
  rebuild, but measure their split so the P0 attributes the win correctly.

## 4. Phase 1 — base + per-pair-delta index on the decide path

The per-pair extra clauses (all appended after the base) and their **exact index
contributions** (this enumeration is the correctness crux — a missed entry ⇒ the
clause never fires ⇒ a MISSED subsumption / wrong verdict):

| appended clause (decide) | body | index delta |
|---|---|---|
| `Q→sub` (2574) | `{Class(q,X)}` | `x_trigger[q] += ci` |
| `Q→D` sat_seed (2588) | `{Class(q,X)}` | `x_trigger[q] += ci` |
| `Q→∃R.t` exists_seed (2603) | `{Class(q,X)}` | `x_trigger[q] += ci` (+ any Exists-head bookkeeping `build_clause_indexes` does for `∃`-heads) |
| value_disjoint (2613) | `{Class(a,X),Class(b,X)}` | `x_trigger[a] += ci`, `x_trigger[b] += ci` (2-atom body: match the base builder's chosen-trigger rule — **first atom? both?** — MUST mirror `build_clause_indexes` exactly) |
| `¬sup` head-only (2620) | `{Class(q,X)}` | `x_trigger[q] += ci` |
| `¬sup` empty-head clash (2625) | `{Class(q,X),Class(sup,X)}` | mirror the base builder's 2-atom-body trigger rule for `(q,sup)` |

Approach:
1. **Clause storage:** stop cloning `self.clauses` per pair. Give the engine the
   shared base clause slice + a small per-pair `extra_clauses: Vec<DlClause>`
   (a few entries). Extend `new_with_prebuilt` (or add a sibling) to take
   `extra_clauses: &[DlClause]` (a slice, generalizing today's single
   `extra_clause`), with `get_clause(ci)` routing `ci >= base.len()` into the
   extra slice by offset.
2. **Index delta:** `Arc::clone` the shared `base_indexes`, then build a **small
   delta** ClauseIndexes-fragment for the extra clauses using the **same
   trigger-selection logic as `build_clause_indexes`** (factor that per-clause
   logic into a reusable `index_one_clause(&mut ix, ci, clause, sym)` so base and
   delta are provably consistent — do NOT duplicate the rule). Consult the delta
   as an overlay, or `Arc::make_mut`-clone-and-extend only the touched trigger
   buckets (measure which is cheaper; overlay avoids any per-pair clone).
3. **disjoint_pairs:** `build_disjoint_pairs` is likewise base-once + per-pair
   delta only if an extra clause is an empty-head 2-atom clash (`¬sup`,
   value_disjoint). Same base+delta treatment; share the Arc.

## 5. Correctness gates (mandatory)

1. **Closure byte-identity, corpus-wide** — the load-bearing gate. classify
   closures FP=0/MISSED=0 and **byte-identical** OFF (`=0`, old rebuild) vs ON on
   ro/sio/sulo/pizza/wine/galen/notgalen. A missed delta index entry surfaces
   here as a changed/lost subsumption.
2. **Per-pair verdict A/B** on the wedge-heavy onts (1508/12698 + a couple more):
   `decide` verdict identical old-vs-new for every probed pair (debug harness).
3. **The `index_one_clause` equivalence** — a unit test asserting that, for each
   of the 6 extra-clause shapes above, the delta entry equals what a full
   `build_clause_indexes` over base+that-clause produces (directly guards the
   crux).
4. **Full suite** green; **fmt + clippy**.
5. **Perf** — the §3 P0 A/B: ≥10 % wall on ≥2 wedge-heavy onts, no regression on
   EL onts (GALEN classify should be flat-or-better).

## 6. Risks

- **Missed / mismatched delta entry (HIGH, the crux).** The per-pair delta must
  reproduce `build_clause_indexes`'s trigger selection **exactly** for every
  clause shape — especially the **2-atom-body** clauses (value_disjoint, `¬sup`
  clash): which atom(s) the base builder picks as the trigger key is the subtle
  part. Mitigation: the shared `index_one_clause` (§4.2) + the §5.3 equivalence
  unit test + the §5.1 byte-identity gate. This is the same "missed-insert-site"
  risk class as the (rejected) label_sig prefilter — but here it is guarded by a
  *direct closure-diff*, which is strong.
- **`∃`-head / Exists bookkeeping.** exists_seed clauses have `∃R.t` heads;
  confirm `build_clause_indexes` does head-side indexing for those and mirror it.
- **SP1.1 `same_tier` (with_sub_roles).** Default OFF; when on, the current path
  rebuilds with the hierarchy. Either keep the rebuild in that (rare) mode or
  extend `with_sub_roles_keep_index` to the delta — do not break it.
- **Overlay vs make_mut.** An overlay lookup adds a branch to the hot
  `x_trigger`/`succ_trigger` reads; measure it doesn't eat the win.

## 7. Delegation notes (Fable)

- Files: `crates/owl-dl-tableau/src/hyper.rs` (`new_with_prebuilt` → extra-slice;
  factor `index_one_clause` out of `build_clause_indexes`) +
  `crates/owl-dl-reasoner/src/lib.rs` (`HyperCache::decide_with_stats` — use the
  shared index + delta, drop the clause clone).
- Land P0 spike + escape hatch first (bisectable), gate, then remove the old path.
- The `index_one_clause` factoring is the whole correctness story — base and
  delta MUST go through the identical per-clause routine.

## 8. Follow-ups (out of scope)

- The fire loop (`enumerate_matches`/`match_body`) — maintainer's arc.
- `ore_ont_9899`-class consistency convergence — separate (completeness tradeoff
  / Konclude-style).

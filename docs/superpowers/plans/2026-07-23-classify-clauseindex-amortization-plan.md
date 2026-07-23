# Plan: amortize the per-pair ClauseIndexes rebuild in classify's subsumption oracle

**Status:** advisor-reviewed 2026-07-23 — **APPROVE-WITH-CHANGES**; level-1 (full base+delta) green-lit contingent on B1–B4 below. Strongest of the four perf candidates.
**Author:** Claude, 2026-07-23. Session: issue-#35 perf arc — the wedge-classify throughput frontier.
**Branch (to create):** `perf/classify-clauseindex-amortize`.

> **Advisor outcome (folded in — supersedes the original §1/§3/§4/§6 where they conflict).**
>
> **Bounding insight (shrinks the whole crux):** all six appended clause shapes
> have **class-only bodies** (`{Class(q,X)}`, `{Class(a,X),Class(b,X)}`,
> `{Class(q,X),Class(sup,X)}`) — **no role-body atoms, no empty bodies**.
> `build_clause_indexes` indexes only *body* atoms, so the per-pair delta touches
> **only `x_trigger`, `match_plans`, and `disjoint_pairs`** — ZERO
> `role_trigger`/`inverse_first_trigger`/`empty_body` deltas. The §6
> inverse/∃-head worries are moot; delete them.
>
> **Delta table verified** (against `build_clause_indexes`, hyper.rs:920–978, which
> loops over *every* body atom): value_disjoint `{a,b}` → `x_trigger[a]+=ci` AND
> `x_trigger[b]+=ci` (**both**); ¬sup clash `{q,sup}` → `x_trigger[q]+=ci` AND
> `x_trigger[sup]+=ci`; exists_seed `Q→∃R.t` → **only** `x_trigger[q]+=ci` (heads
> are never indexed — drop the "∃-head bookkeeping" uncertainty).
>
> **B1 (HIGH — the critical omission):** the delta must also produce
> **`match_plans[ci]`**. `match_body` reads `self.indexes.match_plans[ci]` as a
> **direct index** (hyper.rs:3582) — an un-built entry for `ci ≥ base_len`
> **panics (OOB)**, or if merely `None`-padded, **silently drops the clause →
> missed subsumption**. `index_one_clause` MUST build the match_plan
> (`build_clause_match_plan(clause)`) and storage must route `match_plans[ci]`
> for `ci ≥ base_len` into the extras. All six shapes yield `Some(plan)`.
> Extend the §5.3 equivalence test to assert the `match_plans` entry too.
>
> **B2 (HIGH — framing was wrong):** the label-oracle path does **NOT** amortize
> in the default config. `RUSTDL_SAT_SEED` and `RUSTDL_VALUE_TYPE_DISJOINT`
> default **ON** (lib.rs:1585 / 2100), so `classify_labels` takes the
> **full-rebuild `HyperEngine::new` branch (2814–2822), not `new_with_prebuilt`**.
> So this is **not "porting working code"** — it is building the machinery the
> label path *punted on* (2818). Also the per-pair appended set is **not "a
> handful"**: `Q→sub` + **all** `sat_seed[sub]` (can be tens) + **all**
> `exists_seed[sub]` + **all** `value_disjoint` + `¬sup`. `value_disjoint` is
> **pair-invariant** → fold into `base_indexes`/`base_disjoint_pairs` **once**;
> `Q→sub`, seeds, `¬sup` are pair-varying (the O(#extra) delta).
>
> **B3 (MEDIUM — API doesn't exist):** there is **no `extra_clause`/`get_clause`**
> (the lib.rs:2464 comment is stale); `new_with_prebuilt` takes a single
> `&[DlClause]` and ~10 sites index `self.clauses[ci]` directly (hyper.rs:2361,
> 2384, 2457, 2493, 2572, 2591, 2744, 2849, 3554, 3677). The real change:
> introduce a **base-slice + extra-slice `clause(ci)` accessor** and convert
> those ~10 sites (and `match_plans[ci]`) to branch-route
> `if ci < base_len { base } else { extra[ci-base_len] }`. More invasive than
> "generalize extra_clause to a slice."
>
> **B4 (MEDIUM):** don't clone the `disjoint_pairs` HashSet per pair
> (O(#pairs)/pair defeats the point). Read disjointness as
> `base.contains(p) || per_pair_extra.contains(p)` with the 1 `¬sup` pair (+ any
> non-folded value_disjoint) in a tiny per-pair set.
>
> **Non-blocking:** use **branch-routing, not a HashMap overlay**, for the
> hot-path `match_plans[ci]` and `clause(ci)` reads (predictable
> almost-always-true branch); for `x_trigger`, a tiny per-pair `Vec<(key,ci)>`
> consulted only at the few extra keys beats overlaying every `x_trigger.get`.
> SP1.1 `with_sub_roles` (default OFF): extras have no role atoms ⇒ hierarchy
> irrelevant to the delta; `with_sub_roles_keep_index` on the shared base suffices.
>
> **Strategic ranking (decisive):** only **level 1 (full base+delta with
> branch-routed `match_plans`/`x_trigger` + disjoint overlay)** removes the
> measured cost — it alone is O(#extra)/pair. **Level 2 (`Arc::make_mut`-extend)
> REJECTED** — deep-clones the whole `ClauseIndexes` every pair → still
> O(#clauses)/pair. **Level 3 (clone-removal only) is not actually cheaper** —
> rebuilding without cloning already needs the B3 base+extra routing, i.e. most
> of the plumbing for a fraction of the win; keep only as a P0 fallback if the
> index delta proves too risky. **Commit to level 1 with B1–B4, or defer.**

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

**All deltas verified against `build_clause_indexes` (advisor).** Every shape has
a **class-only body**, so each contributes `x_trigger` entries (one per body
atom) **and** a `match_plans[ci] = Some(build_clause_match_plan(clause))` entry
(B1 — mandatory), plus `disjoint_pairs` for the empty-head 2-atom clauses.

| appended clause (decide) | body | `x_trigger` delta | `match_plans[ci]` | `disjoint_pairs` |
|---|---|---|---|---|
| `Q→sub` (2574) | `{Class(q,X)}` | `[q]+=ci` | `Some(plan)` | — |
| `Q→D` sat_seed (2588) | `{Class(q,X)}` | `[q]+=ci` | `Some(plan)` | — |
| `Q→∃R.t` exists_seed (2603) | `{Class(q,X)}` | `[q]+=ci` (heads NOT indexed) | `Some(plan)` | — |
| value_disjoint (2613) | `{Class(a,X),Class(b,X)}` | `[a]+=ci` **and** `[b]+=ci` | `Some(plan)` | `(a,b)` — **pair-invariant, fold into base once (B2)** |
| `¬sup` head-only (2620) | `{Class(q,X)}` | `[q]+=ci` | `Some(plan)` | — |
| `¬sup` empty-head clash (2625) | `{Class(q,X),Class(sup,X)}` | `[q]+=ci` **and** `[sup]+=ci` | `Some(plan)` | `(q,sup)` — per-pair overlay (B4) |

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

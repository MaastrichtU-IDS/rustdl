# Plan: index the ABox-saturation role-chain closure (semi-naive conditional)

**Status:** advisor-reviewed 2026-07-23 (APPROVE-WITH-CHANGES; all blocking items folded in below) — ready for delegation to Fable.
**Author:** Claude, 2026-07-23. Session: issue-#35 realize/inconsistency perf arc.
**Branch (to create):** `perf/abox-chain-index`.

> **Advisor outcome (summary).** The E²→E×fan-out collapse from **indexing the
> inner leg alone** is the whole win (~21 s → ~1–3 s projected); the 5× fixpoint
> re-scan that **semi-naive** would remove is second-order and very likely
> already under the 3 s bar. Semi-naive reorders derivations — breaking the
> `chain2_fires`-unchanged gate (B2), introducing an easy-to-botch `R1⋈ΔR2`
> r1-leg index-direction bug that MISSES clashes = completeness-unsoundness (B4),
> and depending on exact delta bookkeeping across the interleaved queue drain
> (B5). **Index-only preserves the derivation schedule, so the final edge set is
> byte-identical *by construction* with zero completeness risk.** Therefore
> Phase 1 is split: **1a = index-only (primary, ship this)**, **1b = semi-naive
> (conditional, only if 1a misses ≤3 s, and only with the B2/B4/B5 fixes)**. My
> expectation, shared by the advisor: 1a suffices and 1b never ships.

## 1. Problem (measured this session, not theorized)

`is_consistent` / `realize` / `materialize_*` on `ontologies/real/family.ofn`
(the inconsistency torture fixture, 508 named individuals) take **~21.6 s** to
return `inconsistent`. Reference reasoners on the same ontology, same machine:

| reasoner | verdict | wall |
|---|---|---|
| Konclude v0.7.0 (native) | inconsistent | **0.53 s** |
| HermiT via ROBOT 1.9.10 (Docker) | inconsistent | ~6–8 s (incl. OWLAPI load) |
| rustdl (this arc) | inconsistent | **21.6 s** |

Env-gated per-phase profiling of `saturate_abox_consistency`
(`abox_saturation.rs`) on family isolates the cost to **one phase**:

```
[abox-prof] iters=5 individuals=508 edges=267118 chains2=38 chains3=0
            functional=9 disjoint_pairs=15822
            | chain2=21.40s chain3=0.00s func=0.02s disj=0.13s
            | fires: chain2=228149 chain3=0 type_add=1455
```

**21.40 s of 21.6 s is Rule 4, 2-hop role chains (`chain2`).** Everything else
is noise. Two compounding facts:

1. **The edge set closes to 267,118 edges** (from 508 individuals; 228,149 are
   chain-derived). `267118 ≈ 508² (258064)` — a near-complete transitive
   closure of a family composition/transitive role. The closure size is
   **semantics, not a bug** — the derived edges are genuinely entailed and are
   consumed by `materialize_object_property_assertions`.

2. **`chain2` computes that closure with a brute-force all-pairs edge scan, no
   index, rebuilt every fixpoint iteration.** Code-grounded (`abox_saturation.rs`
   ~773–835):

   ```rust
   let edge_vec: Vec<RawEdge> = edges.iter().copied().collect();   // rebuilt each iter
   for &(ea_id, ea, eb) in &edge_vec {          // outer: all E edges
       if ea_id != r1_id { continue; }
       for &(eb2_id, eb2, ec) in &edge_vec {    // inner: all E edges AGAIN
           if eb2_id != r2_id || eb2 != b { continue; }   // linear scan for "r2-edge from b"
           edges.insert((sup_id, na, nc)); ...
       }
   }
   ```

   The inner loop is an **O(E) linear scan** to find the r2-edges leaving node
   `b` — that should be an O(1) indexed lookup. Cost ≈
   `iters × Σ_rules (|edges matching r1| × E)` — quadratic in E. With E=267 118
   and 38 rules over 5 iterations, that is the 21 s. `chain3` (3-hop) has the
   same shape one nesting deeper (O(E³)); it is 0 on family but the same fix
   must apply so it does not become the next wall.

## 2. Goal & non-goals

**Goal:** compute the *same* role-chain closure with a `(role, node)`-indexed
inner lookup, removing the O(E) inner scan (the quadratic factor). Target family
`saturate_abox_consistency` wall **21.4 s → ≤ 3 s**. Indexing alone (Phase 1a)
is expected to hit this; semi-naive (1b) is a conditional follow-on, not the
plan of record. The ~267 k closure size stays, so we will not match Konclude's
on-demand 0.5 s — that needs a different, non-materializing strategy (out of
scope, §8).

**Non-goals (do NOT do these):**
- Do **not** change closure *semantics*. The final `edges` set (and thus
  `result.edges`, `type_additions`, `.clash`) must be **byte-identical** to
  today's output on every fixture. This is a pure algorithmic reorganization.
- Do **not** switch to on-demand / tableau-style inconsistency detection (the
  Konclude strategy). Separate, much larger redesign.
- Do **not** touch the functional-merge (`func=0.02 s`) or disjoint-clash
  (`disj=0.13 s`) phases beyond what indexing trivially enables; they are not
  the bottleneck. (Note them as follow-ups only — §8.)
- Do **not** change the seeding passes or the type/edge worklist queues (already
  worklist-based and cheap).

## 3. Phase 0 — measurement GATE (do FIRST) — largely DONE, formalize

The profiling harness (env `RUSTDL_ABOX_PROFILE`, per-phase `Instant` timers +
iteration/collection-size counters, emitted after the fixpoint) is written and
was run above. **GO/NO-GO:**

- **GO criteria (all held on family):** `chain2` ≥ 80 % of `saturate` wall
  (measured 21.40/21.6 ≈ 99 %); edge count ≫ individuals (267 k ≫ 508, i.e. the
  closure is the driver); chain-fires ≫ 0 (228 k).
- **Committed second fixture (REQUIRED before Phase 1) — one fixture, two jobs.**
  There is no `family-stripped` in the tree. Author and commit a **consistent**
  transitive-role ABox fixture (a few hundred individuals) that also carries an
  inverse-in-chain and a symmetric role (per §5.2 (a)/(b)/(c)). Use it for BOTH
  (i) the P0 dominance re-measure (confirm `chain2` dominance is not
  family-specific — if not dominant there, still GO on family's strength, but
  note it) AND (ii) the §5.2 byte-identity gate (family can't serve it — it
  clashes and clears `edges`). This removes the earlier "REQUIRED but
  maybe-synthesized" ambiguity.
- **NO-GO if:** on re-measure the dominant cost is actually the `edge_queue`
  drain or functional-merge (it is not, per the profile) — then re-aim.

Deliverable: commit the profiler behind `RUSTDL_ABOX_PROFILE` (it is cheap and
useful for validating the fix); it is NOT the diagnostic eprintln'd version —
keep it env-gated and off-path-free.

## 4. Phase 1a — inner-leg edge index, keep full rescans (PRIMARY — ship this)

This is a mechanical transformation that performs the **identical sequence of
derivation attempts** as today: same outer edges each iteration, same matched
inner edges (the lookup returns exactly what the linear scan matched), same
queue-drain-then-chain ordering. So the final edge set **and** `chain2_fires`
are unchanged **by construction** — no new source of variation, zero
completeness risk.

### 4.1 Indexes

Alongside `edges: HashSet<RawEdge>`, maintain two indexes, rebuilt once per outer
iteration from `edges` at the top of the chain phase (or incrementally via the
choke-point below — either is byte-identical here since the outer schedule is
unchanged):

```rust
by_src: HashMap<(RoleId, IndividualId), Vec<IndividualId>>,  // Named(r)(src,·)
by_dst: HashMap<(RoleId, IndividualId), Vec<IndividualId>>,  // Named(r)(·,dst)
```

Route **every** `edges.insert` site through one choke-point
`insert_edge(&mut edges, &mut by_src, &mut by_dst, e) -> bool` so the indexes
never drift. Edges are canonical `Named(r)(a,b) = (r,a,b)`. The chain2 **inner
leg** (find r2-edges leaving `b`):
- forward (`!r2_inv`): `Named(r2)(b,·)` = `by_src[(r2, b)]`;
- inverse (`r2_inv`, `Inverse(r2)(b,·) = Named(r2)(·,b)`): `by_dst[(r2, b)]`.

This replaces the inner O(E) scan (lines 811–823) with an O(fan-out) lookup —
the E²→E×fan-out collapse that is the entire win. The **outer** loop keeps the
current `for edge_vec { if ea_id != r1_id { continue } … }` scan unchanged
(38×5×E ≈ 5e7 ops — noise; do NOT index it — advisor non-blocking).

### 4.2 chain3

Apply the identical **inner-leg indexing** to chain3's two inner scans (the
middle leg needs the same forward/inverse `by_src`/`by_dst` care). chain3 is
0-fire on family so this is trivially byte-identical there; do it in the same
change so both phases share `insert_edge`. Drop the dead `let _ = b2;` binding
(line 868) while there.

### 4.3 Phase 1b — semi-naive (CONDITIONAL — only if 1a misses ≤3 s)

Do NOT implement unless the 1a perf gate (§6) shows family `chain2` still > 3 s.
Semi-naive removes only the 5× outer-iteration re-scan (second-order). If it
ships, it MUST include:

- **Full `insert_edge` coverage of all 8 edge-insert sites** (§4.1) —
  seed OPA, seed `ObjectHasValue`, type-driven `ObjectHasValue`, inverse
  materialization, role hierarchy, **`SameIndividual` edge propagation (line
  765)**, chain2, chain3 — feeding a single continuously-accumulating
  `pending_delta`, `take()`-n and cleared at chain-phase entry (NOT "seeds only
  on iter 1" — that under-fills; the accumulator must include iteration-1 drain
  edges). Chain output still `edge_queue.push_back`s (keeps feeding Rules
  2/3/5/9b).
- **Both join legs** `ΔR1 ⋈ R2 ∪ R1 ⋈ ΔR2`, with the **r1 outer-leg index
  direction spelled out** (it is the MIRROR of the r2 leg — the B4 botch site):
  given a new r2-edge `(b,c)`, find r1-edges *ending at* `b`:
  - forward r1 (`!r1_inv`): `Named(r1)(·,b)` = `by_dst[(r1, b)]`, chain-`a` = src;
  - inverse r1 (`r1_inv`): `Named(r1)(b,·)` = `by_src[(r1, b)]`, chain-`a` = dst.
  Getting this wrong under-derives → MISSED clash → **completeness-unsound**
  (reports `consistent` when `inconsistent`). The §5 fixture MUST put an inverse
  role as the *first* chain leg to exercise this.
- **§5.6 `chain2_fires` gate demoted to diagnostic** (see §5) — semi-naive
  reorders which rule first-inserts an edge, so the count legitimately shifts.

## 5. Correctness gates (mandatory, in order)

The **load-bearing gate is closure edge-set identity (§5.2)** — not the clash
verdict, not the fires count.

1. **Verdict-identity, ABox onts:** `family` still `inconsistent`; any ORE ont
   with an ABox — `is_consistent` verdict unchanged. (Family can validate only
   the *verdict* here — see §5.2 on why the byte-identity gate can't run on it.)
2. **Closure edge-set identity — the strong gate.** `family` is INCONSISTENT and
   the code clears `edges` on clash (line ~1041, `if !result.clash`), so
   `materialize_object_property_assertions` returns `Err(Inconsistent)` on it —
   the closure never materializes. The confirmed **sole** consumer of the closure
   is `materialize_object_property_assertions` (lib.rs:110–111), which `sort()`s +
   `dedup()`s, so the invariant is the derived edge **set** (order-independent).
   Therefore this gate needs a **committed, CONSISTENT ABox fixture** (Phase-0
   deliverable, §3) that exercises: (a) a transitive role with a large closure,
   (b) an **inverse role used inside a chain** (first leg, for the B4 r1-leg
   path), and (c) a **symmetric / self-inverse** role. Assert OFF-vs-ON:
   - `materialize_object_property_assertions` output byte-identical, AND
   - via a debug hook, the **full `edges` `HashSet` identical** (covers
     anon/TOP/BOT-filtered edges the materialize output drops).
3. **Corpus consistency regression:** `is_consistent` on ro/sio/sulo/pizza/wine/
   galen — verdicts unchanged (mostly ABox-free ⇒ phase inert; confirm anyway).
4. **Full suite:** `cargo test -p owl-dl-reasoner -p owl-dl-tableau` green;
   `abox_saturation` unit tests unchanged; add a **brute-vs-new edge-set identity
   unit test** on the committed fixture, diffed **through the
   `RUSTDL_ABOX_CHAIN_BRUTE=1` escape hatch** (§6) so CI compares both paths, not
   just a hand run.
5. **fmt + clippy -D warnings** (module already has a broad `#![allow]` header).
6. **Perf:** re-run `RUSTDL_ABOX_PROFILE=1 consistent family.ofn`; assert
   `chain2` wall drops ~21 s → ≤ 3 s. **`chain2_fires`:** for **1a** it MUST stay
   228 149 (schedule preserved — a real check); for **1b** it is **diagnostic
   only** (informational, same order of magnitude) — semi-naive reorders which
   rule first-inserts an edge, and family `break`s on clash mid-fixpoint so the
   count is a partial snapshot that legitimately shifts. Do NOT hard-gate on it
   under 1b. (Also verify the count is reproducible run-to-run before trusting it
   at all: `edge_vec` derives from a `HashSet` with randomized `RandomState`.)

## 6. Risks

- **Inverse-polarity indexing bug (applies to 1a and 1b).** The
  `r1_inv`/`r2_inv`/`sup_inv` direction logic is subtle. For **1a** it is only
  the inner leg (forward r2 → `by_src`, inverse r2 → `by_dst`) — small surface.
  For **1b** it also adds the mirror r1 outer leg (§4.3) — the higher-risk site.
  Mitigation: closure-edge-set-identity gate (§5.2) on the committed fixture with
  an inverse-in-chain role; `RUSTDL_ABOX_CHAIN_BRUTE=1` escape hatch diffed in CI
  (§5.4).
- **Semi-naive under-derivation (1b ONLY).** If the delta misses an edge source
  (inverse, hierarchy, and `SameIndividual` (line 765) all add edges — domain/
  range add *types* not edges), a derivation is lost → MISSED clash →
  *completeness* regression (`consistent` where it should be `inconsistent`).
  Mitigation: the `ΔR1⋈R2 ∪ R1⋈ΔR2` union + full 8-site `insert_edge` coverage
  (§4.3) + the §5.2 edge-set-identity gate. **1a has none of this risk** (the
  derivation schedule is unchanged), which is why 1a ships first.
- **Memory.** Two extra `HashMap<(RoleId,IndividualId), Vec<..>>` over a 267 k
  edge set ≈ tens of MB — acceptable; note peak RSS in the perf gate.
- **Functional-merge rebuilds `fillers_by_subj` from all edges each iter** (§8)
  is a *separate* latent cost; it is 0.02 s on family so out of scope, but the
  `by_src` index would let it be indexed too — leave a TODO, do not do it here.

## 7. Delegation notes (Fable)

- Single file: `crates/owl-dl-reasoner/src/abox_saturation.rs`. No public API
  change (`SaturationResult` unchanged).
- Commit order (bisectable): (1) profiler + committed consistent fixture
  (Phase 0), (2) **Phase 1a index-only** — expected to be the whole fix.
  **Stop and re-measure after 1a.** Only if it misses ≤3 s, (3) Phase 1b
  semi-naive with the full B4/B5 fixes.
- Keep the `RUSTDL_ABOX_CHAIN_BRUTE=1` escape hatch for one release for A/B.
- Do NOT refactor the seeding passes or rename fields — keep the diff to the
  chain phase + the `insert_edge` choke-point.
- **Highest-risk part is inverse-polarity index direction** (§4.1 inner leg for
  1a; §4.3 r1 outer leg for 1b). The committed fixture's inverse-in-chain role
  is the guard — do not skip it.

## 8. Follow-ups (explicitly out of scope)

- Index the functional-merge `fillers_by_subj` build (reuse `by_src`).
- On-demand / non-materializing inconsistency (the Konclude strategy) — the only
  path to sub-second on family, but a major redesign; separate spec.

(Dropped per advisor: "per-role outer edge lists" — the post-index outer
`continue`-scan is ~5e7 ops, noise; not worth the extra state.)

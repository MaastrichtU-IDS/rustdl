# R2 — code review sweep: `crates/owl-dl-saturation`

Agent R2. Method: `superpowers:systematic-debugging`. All numbers below are from
**direct counts** by temporary instrumentation (since reverted, `git status` clean)
or from oracle runs, not from arithmetic plausibility.

Binary hygiene: the instrumented builds were pinned to
`scratchpad/rustdl-R2-instrumented` and `scratchpad/rustdl-R2-instr2`.
**One stale-binary hazard was caught and corrected mid-investigation** — a
background rebuild left `target/release/rustdl` still containing the `rrr`
instrumentation symbols; it was rebuilt in the foreground and verified with
`strings target/release/rustdl | grep -c rrr` → `0` before any oracle probe.
All measurement runs used `( ulimit -v N; timeout M … )`, one at a time.

---

## PART 1 — `ore_ont_11085`: ROOT CAUSE, CONFIRMED

### The container

**`todo_subsumer: VecDeque<(ClassId, ClassId)>`**
— declared `crates/owl-dl-saturation/src/lib.rs:373`
— pushed `crates/owl-dl-saturation/src/lib.rs:958` (in `enqueue_subsumer`, `:956`)
and `:757` (the seed broadcast).

Element size is exactly 8 bytes, so its doubling produces **exactly power-of-two
GB** totals — which is why the reported `VmPeak` curve was clean
`1.18 / 2.18 / 4.18 / 8.18 / 16.18 GB`: the container is `2^k` GB and the constant
**+0.18 GB is the entire rest of the engine**.

### The measurement that identifies it

Instrumented dump of every candidate container, `--saturation-only`,
`RAYON_NUM_THREADS=1`, `ulimit -v 12 GB`:

| sample | `todo_subsumer` cap | every other container |
|---|---|---|
| run-entry | 16,777,216 (128 MB) | facts cap 16, seen_facts 14, facts_by_sub 1 MB, facts_by_target 1 MB, **subsumers 62 MB**, **subsumed_by 62 MB**, conj_by_body 1 MB, exist_trig 1 MB, disj_by_class 1 MB, atomic_content 0, merged_atom_sets 0, b2 0 |
| +2 M pops | 67,108,864 (512 MB) | *all unchanged* |
| +4 M pops | 268,435,456 (2 048 MB) | *all unchanged* |
| +6 M pops | 1,073,741,824 (**8 192 MB**) | *all unchanged* |
| death | — | `memory allocation of 17179869184 bytes failed` |

The fatal allocation is **17,179,869,184 B = 16 GiB = 2^31 × 8 B** — the VecDeque
doubling from 2^30 to 2^31 entries. Nothing else moves at all: the two `IdMatrix`
bitmatrices are 62 MB each and **flat**, `facts` never exceeds 512 slots.

### The driver (exact, reconciled to the entry)

`top_subsumers.len() == 732`, and the file contains exactly **732**
`SubClassOf(owl:Thing, …)` axioms (direct grep = 732, matching `tbox-stats`
`residual_atomic: 732`).

The `⊤ ⊑ C` broadcast seed at **`lib.rs:752-760`** pushes
`num_user_classes × |top_subsumers|` pairs eagerly. Reconciliation of the
run-entry queue length, exact to the entry:

```
22,642  reflexive C⊑C            (lib.rs:708-711)
    13  synthetic reflexive       (lib.rs:717-720)
20,741  told atomic subsumptions  (lib.rs:744-747)
22,642 × 732 = 16,573,944  ⊤⊑C broadcast (lib.rs:752-760)
------------------------------------------------
16,617,340  =  observed todo_subsumer.len() at run-entry  ✅ EXACT
```

### The amplifier (why 16.6 M becomes >2.1 G)

`enqueue_subsumer` (`lib.rs:956-960`) dedups **only** against
`self.subsumers.contains(c, d)`, and `subsumers` is written by `record_subsumer`
at **POP** time (`process_subsumer`, `lib.rs:1146-1149`). So there is **no
in-queue membership test**: while a 16.6 M-deep FIFO backlog drains, the same
`(c, d)` is re-pushed by every derivation path. Forward transitivity
(`lib.rs:1098`, `for e in supers_of_class(d)`) and backward transitivity
(`lib.rs:1115`, `for x in subs_of_class(c)`) each re-derive a pair once per
intermediate class, and because all 732 tops subsume everything, **every class has
≥733 subsumers ⟹ ~733 derivation paths per pair.**

Direct counters at 7 M pops:

```
pops        =   7,000,000
pops_dup    =         442     ← pop-time dedup is USELESS: the duplicates are still queued
enq_calls   = 3,090,503,108   (441 per pop)
enq_pushed  =   927,520,091   (132 per pop → queue grows 132× faster than it drains)
```

`enq_pushed = 927,520,091` already **exceeds the total number of distinct ordered
pairs, 22,642² = 512,660,164**. That is a *proof* (not an inference) that
**≥414,859,927 pushes were duplicates of pairs already sitting in the queue.**

### Decisive counterfactual

Delete only the 732 `SubClassOf(owl:Thing, …)` lines; everything else identical:

| input | wall | peak RSS | outcome |
|---|---|---|---|
| `ore_ont_11085.owl` | DNF | ≥21.7 GB (cap-dependent) | killed |
| same, 732 `⊤⊑C` axioms removed | **0.72 s** | **166 MB** | completes, 20,428 rows |

≈130× RSS, DNF → 0.72 s. The driver is unambiguous.

### Why the two prior hypotheses were right to be refuted

Independently re-confirmed by the same dump: the dense-matrix hypothesis is dead
(both `IdMatrix`es are 62 MB and flat, because `num_total_classes = 22,655 <
DENSE_MAX = 50,000`), and runtime Tseitin minting is dead (`next_id` never moves
off 22,655; `merged_atom_sets = 0`, `b2_disjunctions = 0`).

### Population size of this mechanism

Amplification ≈ `|top_subsumers|`, so what matters is `|tops|`, not just the
product. Ranked over the ORE pool by `|tops| × classes`:
`11085` (732 × 22,642 = 16.6 M) ≫ `8475` (124 × 74,258 = 9.2 M) >
`5840` (150 × 5,086) > `2397` (59 × 9,860) > everything else ≤ 0.36 M.
67 pool ontologies have any `⊤⊑C` axiom; only the first three or four have a
large enough `|tops|` for the ~`|tops|`-fold duplication to matter.

**Cross-check: this mechanism does NOT explain the other large-RSS cases.**
`ore_ont_1833`, `15655`, `3080`, `3914`, `9347` all have **zero**
`SubClassOf(owl:Thing, …)`. Probing `1833` directly: its saturation **completes**,
`todo_subsumer` peaks at 16,384 entries, and total saturator memory is ~130 MB.
So `1833`'s 7.94 GB is **not in this subsystem** — it is downstream
(classify/tableau). Do not attribute it here.

### Fix direction (NOT implemented — recorded for the owner)

Record-at-enqueue instead of record-at-pop (this is what ELK does: the "is new"
test happens on insertion into the context, not on dequeue). `process_subsumer`
already early-returns when `record_subsumer` is false, so the closure content is
unchanged; the queue becomes bounded by the distinct-pair count. A cheaper
alternative is an explicit in-queue bitset. Either way the required gate is
byte-identical closures on the curated corpus, because this is a
*storage/scheduling* change, not a rule change.

---

## PART 2 — findings

Categories as requested. **CONFIRMED** = measured or oracle-checked in this
session. **SUSPECTED** = read from code, mechanism clear, magnitude not measured.

### 2.1 incorrect — the D10 class (gate certifies complete, engine drops the axiom)

#### F1. CONFIRMED — `⊥` as an `∃`-filler is admitted by `is_pure_el` and silently dropped

- **Gate**: `crates/owl-dl-reasoner/src/classify.rs:1191` — `is_el_concept` returns
  `true` for `ConceptExpr::Bot` in *any* position, and `:1193` recurses into
  `Some(role, body)` without restriction. So `X ⊑ ∃r.⊥` is certified pure-EL.
- **Engine drop**: `crates/owl-dl-saturation/src/lib.rs:4197` — the
  `_ => return None` in `atomic_or_tseitin_body_with_extras` (`:4163`) has no `Bot`
  arm, so the RHS existential lowering returns `None` and the caller drops the
  whole axiom. Same for a `Bot` inside a conjunctive filler:
  `lib.rs:4250` in `atomic_classes_with_existential_markers`.
- **Repro + oracle** (fixtures in `scratchpad/d10/`):

  | fixture | axiom | rustdl | Konclude |
  |---|---|---|---|
  | `p1.ofn` | `SubClassOf(:X ∃r.owl:Nothing)` | **satisfiable**, `fragment: pure-EL (… saturator alone is complete)` | `X ≡ owl:Nothing` |
  | `p4.ofn` | `SubClassOf(:X ∃r.(A ⊓ owl:Nothing))` | **satisfiable**, same banner | `X ≡ owl:Nothing` |
  | `q1.ofn` | `EquivalentClasses(:A ∃r.owl:Nothing)` | **satisfiable**, same banner | `A ≡ owl:Nothing` |

- **Soundness**: this is a MISS (under-approximation), **not** an FP — FP=0 is not
  threatened. Its severity is the false *completeness certificate*, exactly the
  D10 class.
- **Scope is narrow and was verified, not assumed.** The *dynamic* equivalents all
  work: `X ⊑ ∃r.C` + `C ⊑ ⊥` (p5), `X ⊑ ∃r.C` + `C ⊑ D,E` + `Disjoint(D,E)` (p7),
  `X ⊑ ∃r.C` + `Domain(r, ⊥)` (p8) all correctly yield `X` unsat. `A ⊑ B ⊓ ⊥` (p2)
  and `Equiv(A, ⊥)` (p3) also work. Only the **syntactic `⊥` under an `∃`** leaks.
- **Corpus impact today: inert — measured on the gate, not on grep.** 8 pool
  ontologies contain a syntactic `∃R.owl:Nothing`; all 8 were classified and all 8
  route to `mode: hybrid` (other constructs kick them off the fast path), one
  (`1194`) DNFs. So this is a **latent** hole that fires the moment a pure-EL
  ontology carries `∃R.⊥`.
- **Falsifiable prediction**: give the lowering a `Bot` arm (subject → `directly_unsat`),
  or narrow `is_el_concept`'s `Bot` arm so it is not admitted under a `Some`/`And`
  filler. Then `p1`/`p4`/`q1` flip to `unsat X` matching Konclude, and every
  curated-corpus closure stays byte-identical (no fixture has `∃R.⊥`).

#### F2. CONFIRMED-still-open — role-chain-induced poison (the known residual), and its boundary is narrower than documented

`CLAUDE.md` records `Chain(t,u) ⊑ r` + `Domain(r,⊥)` + `C ⊑ ∃t.∃u.A` as MISSED
while `is_pure_el` certifies complete. Re-tested and refined: with a **named**
intermediate (`C ⊑ ∃t.B`, `B ⊑ ∃u.A`, `Chain(t,u) ⊑ r`, `Domain(r,D)`) rustdl
**does** derive `C ⊑ D`, agreeing with Konclude (`q2.ofn`). So the residual is
specifically the *nested/marker* filler form, not chains in general — worth
recording, because it narrows the fix surface.

#### F3. SUSPECTED-unadjudicated — chain-induced *range* not propagated

`q3.ofn` (`Chain(t,u) ⊑ r`, `Range(r,E)`, `C ⊑ ∃t.B`, `B ⊑ ∃u.A`, `∃r.E ⊑ D`)
entails `C ⊑ D` by my reading of the OWL 2 semantics, and rustdl misses it under a
pure-EL certificate. **But Konclude also misses it**, so I am not asserting a bug:
it needs a HermiT adjudication before it is called anything. Recorded so it is not
lost. Same gate/engine shape as F2 if it is real.

### 2.2 inefficient

#### F4. CONFIRMED — unbounded duplicate `todo_subsumer` (Part 1). `lib.rs:373` / `:958` / `:1146`.
Predicted effect of record-at-enqueue: `11085` queue capped at ≤512 M entries
(realistically ~16.6 M, i.e. 128 MB) instead of ≥16 GB; `11085` completes.

#### F5. CONFIRMED — the `⊤ ⊑ C` broadcast is materialised eagerly, `O(classes × |tops|)`
`lib.rs:752-760`. 16,573,944 queue entries pushed before a single rule fires. This
is a *representation* choice: 732 top-subsumers could be held as a set applied
lazily (every named class trivially has all of them) rather than as 16.6 M queue
entries and 16.6 M matrix bits. Predicted effect: seed cost and the resulting
backlog drop to ~`|tops|`; `11085` seed goes from 128 MB of queue to kilobytes.
Falsifiable: with F4 fixed but F5 unfixed, `11085` should still show a 128 MB queue
plateau and a 16.6 M-entry drain.

#### F6. CONFIRMED (code) / magnitude measured — full-row `Vec` allocated per popped pair
`supers_of_class` (`lib.rs:574`) and `subs_of_class` (`lib.rs:585`) each
materialise an **owned `Vec<ClassId>` of the entire matrix row**. They are called
on **every** popped pair at `lib.rs:1098` and `lib.rs:1115`. On `11085` rows are
733–22,642 wide, and the counters show **441 elements iterated per pop** across the
two legs, i.e. two heap allocations of ~220 elements per pop, 7 M+ times. Predicted
effect of iterating the row in place (or returning an iterator / reusing a scratch
buffer): eliminates ~2 allocations + ~441 copies per pop with no verdict change —
the same shape as the shipped `HyperEngine::is_blocked` in-place fix.

#### F7. CONFIRMED (code) — `subs_of_class(fact.sub)` allocated *inside* a nested loop
`lib.rs:1307` allocates a fresh full-row `Vec` for **every (fact, trigger) pair**,
inside `for fidx in fact_idxs { for tidx in &trigger_idxs { … } }`. It is
loop-invariant with respect to `tidx` and should be hoisted at minimum. Same
pattern at `lib.rs:1631` and `:1726`. Predicted effect: allocation count in the
existential-trigger path drops by a factor of `|trigger_idxs|`, verdict-identical.

#### F8. CONFIRMED (code) — index rows cloned per popped pair
`lib.rs:1150` (`conjunctive_by_body[d].clone()`), `:1186`
(`conjunctive_unsat_by_body[d].clone()`), `:1086` (`facts_by_sub[c].clone()`),
`:1293` (`facts_by_target[c].clone()`), `:1295`, `:1359`, `:1365`, `:1448`, `:1729`.
Each is a borrow-checker workaround (`if let Some(x) = Some(expr.clone())` at
`:1150`/`:1293` is literally a clone wrapped in a no-op `Some`). Predicted effect:
index-swap or split-borrow removes them; hot-loop allocation traffic drops,
verdicts unchanged.

#### F9. CONFIRMED (measured) — `disjoints_by_class` is a dense all-pairs expansion
`lib.rs:496-500` expands `rules.disjoint_pairs` into
`Vec<Vec<ClassId>>` over all classes. Measured **117 MB on `ore_ont_1833`** (7,944
classes) — one large `DisjointClasses(…)` becomes `O(n²)` pairs stored twice. This
is the same `O(k²)` shape as the DKey-disjointness conversion blow-up that was
fixed in v0.3.29, in a different container. Predicted effect: a
disjointness-set/component representation cuts this to `O(n)`; on `1833` saturator
RSS should fall from ~130 MB to ~15 MB. Note `1833`'s headline 7.94 GB is
elsewhere, so this is a real but non-headline win.

#### F10. SUSPECTED — `IdMatrix` never degrades `Dense → Sparse` once grown
`IdMatrix::with_capacity` (`lib.rs:2168`) picks the representation from the
*initial* `n` against `DENSE_MAX = 50_000`, but `grow_to` (`lib.rs:2223`, called
from `introduce_runtime_synthetic`, `lib.rs:655-657`) only widens the existing
representation. An ontology starting at e.g. 49,000 classes and minting runtime
synthetics past 50,000 keeps a dense `n²`-bit matrix with **no upper bound** — at
n = 370,000 that is ~16 GB per matrix, i.e. the D4 failure mode re-entering
through the back door the D4 fix did not cover. Additionally `grow_to`'s Dense arm
re-`grow`s **every existing row** on **each single** new synthetic, so k synthetics
cost `O(k · n)` reallocations. Not triggered by `11085` (13 synthetics, stays at
22,655), hence SUSPECTED. **Falsifiable prediction**: construct an ontology with
~49,900 named classes plus a `disjunctions_by_class`-bearing shape that mints
>200 runtime synthetics; peak RSS should scale as `(49,900 + k)² / 4` bytes and
`IdMatrix` should report `Dense`. If instead it reports `Sparse` or stays flat,
this finding is wrong.

#### F11. SUSPECTED — SP-B1 forced-disjunct block costs a third full-row `Vec` per pop
`lib.rs:1258-1289` runs on every newly recorded subsumption whenever *any* atomic
disjunction exists, and its first act is `for g in self.supers_of_class(c)` — a
full-row owned `Vec`. Measured **inert on `11085`** (`b2_disjunctions = 0`,
`disjunctions_by_class` empty despite `residual_or = 1341` — the saturator's
`Atomic ⊑ Or(all-atomic)` shape is stricter than the tableau's residual count, a
useful reminder that `residual_or` is not a proxy for this). Predicted effect on a
genuinely disjunction-bearing EL ontology: ~50% more per-pop allocation than F6
alone; falsified if a disjunction-bearing fixture shows no allocation delta when
the block is hoisted behind a cheaper `disjunctions_by_class` lookup.

#### F12. SUSPECTED — Phase-2d fact inheritance is `|facts| × |subclasses|`
`push_fact_impl` (`lib.rs:1055`) recursively copies every existential fact to every
subclass of its subject, each recursion allocating a full-row `Vec`. Bounded by
distinct `(sub, role, target)` triples, so it terminates, but the bound is
`classes × distinct(role,target)`. Inert on `11085` (442 facts, 429 inherited).
Predicted effect: on an ontology with many `∃` axioms over a deep hierarchy,
`facts` / `seen_facts` / `facts_by_sub` / `facts_by_target` / `todo_fact` grow
together as `O(classes × ∃-count)`; that is the container set to instrument first
on a large-RSS ontology that is **not** `⊤⊑C`-driven. The comment at `lib.rs:1049`
correctly warns that gating it on `has_functional_roles` is unsound — any
mitigation must be representational, not a gate.

### 2.3 missing

#### F13. CONFIRMED — no `Bot`-filler rule in the existential lowering
The mechanism behind F1: there is no rule for `∃r.⊥ ⊑ ⊥` on the *filler* side,
even though the sibling cases all exist and are individually commented
(`∃r.A ⊑ ⊥` → `directly_unsat` marker, `lib.rs:3882`; `∃r.⊤ ⊑ ⊥` →
`poisoned_roles`, `lib.rs:3857`; `Atomic ⊑ ⊥`, `lib.rs:3560`; `And(…) ⊑ ⊥`,
`lib.rs:3868`; `⊤ ⊑ ⊥`, `lib.rs:3961`). The gap is that all five handle `⊥` on the
**consequent**; none handles `⊥` appearing as a **filler** in the antecedent's
lowering. Predicted effect: adding it closes F1 with no other behaviour change.

#### F14. SUSPECTED — chain-induced range (F3), pending HermiT adjudication.

---

## PART 3 — FP=0 review (the absolute invariant)

I looked specifically for anything that could add an unentailed subsumption and
**found nothing new**. Checked:

- `record_subsumer` (`lib.rs:940`) — writes both matrices symmetrically; the
  proposed record-at-enqueue change would move *when* a pair is recorded, and any
  such change must be gated on byte-identical closures for exactly this reason.
- SP-B1 disjunction ingestion (`lib.rs:3528-3549`) — correctly guarded on
  `Atomic(sub)` **and** `Or(sup)` with *all* disjuncts atomic, so only genuine
  `C ⊑ D₁⊔…⊔Dₙ` coverings are registered; the `EquivalentClasses` reverse direction
  cannot enter (its `sub` is the `Or`).
- SP-B1/B2 forcing (`lib.rs:1258-1289`) — "all but one disjunct excluded ⟹ force
  the survivor / none ⟹ unsat" is sound given only *soundness* of the disjointness
  and unsat derivations, which is the existing invariant.
- SP-B2a synthetic sharing (`lib.rs:627-651`) — `Sᵢ = C⊓Dᵢ` synthetics are
  deduped by sorted body with statically-introduced Tseitin classes; sharing is
  semantically safe because both denote `C⊓Dᵢ`, and the two-way (`F ⊑ Bᵢ` plus
  `{Bᵢ} ⊑ F`) emission is what B2's `Sᵢ` unsat ⟺ `C⊓Dᵢ` unsat argument needs.
- The `union_existential_marker` vs `existential_marker` distinction
  (`lib.rs:3700-3720`) is already documented as FP-critical and is respected.

Every completeness finding above (F1–F3, F13–F14) is a **MISS** direction.
Nothing in this sweep threatens FP=0.

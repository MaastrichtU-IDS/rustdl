# How KM classifies, and how it differs from rustdl's decision procedure

**Date:** 2026-07-28
**Subjects:** Kobayashi-MaRust (`/data/dumontier/kobayashi-marust/engine`, `git 7aed676`)
vs rustdl (`/data/dumontier/rustdl`, `main`).
**Method:** code reading of both engines. KM's CB core (`engine.rs` disjunctive-head
construction, one-saturation subsumption read-off) was verified directly against
source; the routing/race framework and the `konclude_ht` port status are mapped from
the code + `STATUS.md`/`PORT.md` and are flagged where port-in-progress.

The one-sentence contrast: **KM is a *consequence-based* classifier — one connected
saturation over per-class "contexts" whose clauses carry *disjunctive heads*, from
which every subsumption is read off directly, with a hypertableau kept as a racing
competitor. rustdl is a *refutation* classifier — an EL forward-saturation fast path
for the complete fragment, and otherwise O(n²)-shrunk-to-top-down *per-pair* `C ⊓ ¬D`
unsatisfiability probes decided by a hypertableau "wedge" (+ tableau).** KM resolves
disjunction *by consequence* (no case split); rustdl resolves it *by tableau
branching* (case split + backjumping).

---

## Part 1 — KM's classify

### 1.1 Top-level pipeline

`km classify [--route R] <ont>` (`src/bin/km.rs:24,400-488`) →
`orchestrate::classify` (`src/orchestrate/mod.rs:464`):

```
1. FRONTEND    ofn/owl → normalized DL-clause set (+ Meta: iri_map, named classes,
               el_rbox_safe, detected route).  run_ofn_split (mod.rs:478)
2. ROUTE       pick ONE route from the ontology's features (§1.2).  routing.rs:397
3. CONSISTENCY ABox-inconsistency / DL-safe-rules pre-check first.   mod.rs:508,533
4. ENGINE      dispatch to the route's mechanism (elc | cb | ht | portfolio),
               possibly as a RACE of several arms.                  mod.rs:589-745
5. ASSEMBLE    read subsumptions off the winning engine, expand IRIs, drop
               self-loops, collect owl:Nothing as unsatisfiable, emit.  mod.rs:757-805
```

Classification is **one saturation that yields all subsumptions** (not per-pair) on
the CB path; the racing HT arm is per-pair (§1.4). Incremental
(`incremental.rs`) is a separate command — `classify` always does a fresh run.

### 1.2 The routing decision (measured routing, not one universal algorithm)

`routing::select(profile)` (`src/routing.rs:397-436`) is a **two-stage** decision:

```
select(profile):
  match semantic_fragment(profile):                       # hard, soundness gate
    UnsupportedRules | Rules            -> HtRules         # DL-safe rules → HT
    NativeBridgeAbox                    -> CertifiedNominals
    Nominal if inv_card_separable       -> CertifiedCardNominals
    Nominal                             -> Nominals
    PositiveAbox | SriqCore:
      if inv_card_separable             -> ProductionAll   # portfolio
      else:
        r = learned_decision_tree(profile)                 # perf tuning (ML tree)
        if sriq_policy_eligible(r): r  else CbPlain16
```

- The **semantic-fragment gate** is a soundness partition over ontology features
  (rules, ABox shape, nominals, inverse+cardinality separability). The **learned
  decision tree** (`routing/routing_tree_generated.rs`) only picks *performance*
  variants within the SRIQ-core fragment.
- ~35 named routes each bundle a *mechanism* (`cb` | `elc` | `ht` | `portfolio` |
  `tableau`) + env settings (absorption on/off, thread count, HT sub-driver).
  Features come from the normalized clause shape (`orchestrate/features.rs`,
  surfaced by `km features`): disjunction counts, body/head sizes, DL constructs
  (inverse, cardinality, nominals, datatypes, role chains).

**Key decision:** KM does *not* commit to one algorithm. It classifies the ontology
first, then routes to the engine empirically best for that class — and for the
performance-sensitive core, to an engine chosen by a trained tree.

### 1.3 The core: the disjunctive consequence-based (CB) calculus

This is the primary engine (`engine.rs`, `calc.rs`, `clause.rs`), a Rust port of the
Cuenca Grau / Horrocks / Tena-Cucala **disjunctive consequence-based calculus for
ALCHOIQ** (the Sequoia family).

**Data model.**
- A **clause** is `body → head`: body = conjunction of predicates, head =
  **disjunction** of literals (`clause.rs:46-73`). Literals: concept `C(t)`, role
  `R(s,t)`, equality `s≈t`, disequality `s≉t`; empty head = ⊥.
- Terms are integer-ordered (`calc.rs:30-140`): neighbour `z_i < y < x < o <
  f_i(x) < f_i(o)`. Function (Skolem) terms are **maximal**, orienting
  paramodulation downward.
- A **context** is a local saturation scope keyed to a "core":
  - one **root context per named class** to classify — seeded `⊤ → A(x)` (the
    *Core* rule), so the context computes "everything entailed for an A"
    (`engine.rs:2504-2516`);
  - **successor contexts** per Skolem function term (one per `f`, reused —
    `successor_for`, `engine.rs:4045-4075`) — this reuse is what bounds generation.

**The rules** (README table; `engine.rs`):

| Rule | Meaning | Site |
|---|---|---|
| **Core** | seed `⊤ → p` for each core atom `p` | `2504` |
| **Hyper** | hyperresolution: resolve an ontology clause's body against context clause heads; conclusion keeps **all** unmatched head literals ⇒ *disjunctive* head | `3078-3251`, resolvent build `3455-3466` |
| **Succ** | an existential `C(f(x))`/`R(x,f(x))` in a head pushes a hypothesis clause into `f`'s successor context | `4716-4738` |
| **Pred** | a successor's derived clause is **back-substituted** (`y↦x, x↦f(x)`) and resolved into the *predecessor* — backward propagation | `3473-3565` (local), `4743-4815` (inter-ctx) |
| **Eq** | paramodulation with a derived equality (from functionality/`≤1`) | `2926-2936` |
| **Ineq** | eagerly drop `t≉t`; empty head ⇒ clash | `2681-2695`, `clause.rs:223` |
| **Elim** | forward+backward subsumption via a redundancy trie (keep an antichain of maximal clauses) | `1604-1687,1745-1763` |

**Saturation loop** (per context, `engine.rs:2787-3030`) + **inter-context fixpoint**
(`4996-5103`):

```
SATURATE_CONTEXT(ctx):
  while (c = ctx.todo.pop()):
    if forward_subsumed(c): continue           # Elim
    for max in c.maximal_head_literals():
      Hyper(ctx, c, max)  -> add resolvents     # disjunctive heads stay intact
      if max carries a function term:
        enqueue Succ(ctx, f, max, succ_ctx(f))  # push existential to successor
        Pred_local(ctx, c, max) -> add          # resolve back-pushed clauses
      if max is an equality: Eq(...) ; if ≉ : Ineq(...)
    ctx.worked_off.push(c)

CLASSIFY(O, classes):
  seed a root context per class (Core), compute a shared TBox closure once
  repeat:                                        # message fixpoint
    saturate every dirty context
    for each successor: propagate()  -> enqueue Pred messages back to predecessors
    drain Succ/Pred messages -> add hypothesis / back-substituted clauses
  until no messages and all contexts saturated
```

**Disjunction *without* branching — the crux** (`engine.rs:3455-3466`, verified). When
Hyper resolves, the conclusion's head is the union of *all* candidate head literals
minus the matched one:

```rust
for l in &clause.head { if *l != Lit::P(matched) { head.push(*l); } }  // ALL disjuncts kept
```

So `A ⊔ B` stays a single clause `⊤ → A ⊔ B`. If later `A → C` and `B → C`, Hyper
derives `⊤ → C ⊔ B` then `⊤ → C ⊔ C = ⊤ → C` — **by forward consequence, with no
case-split search and no backtracking.** (There is an optional `branch_ordered`
guard, `3442-3453`, that refuses to combine ≥2 disjunctive premises to bound
derived×derived blow-up; off by default.)

**Subsumption read-off — one saturation, all subsumptions** (`engine.rs:5592-5653`,
verified). After the fixpoint, for each root context of class `A`, every worked-off
**unit** clause `⊤ → B(x)` (empty body, one concept literal on the central variable)
means `A ⊑ B`; an empty clause `⊤ → ⊥` means `A` unsatisfiable:

```rust
for ci in ctx.worked_off:
  if c.body.empty() && c.head == [Concept(B, x)] && is_central(x): supers(A).push(B)
```

No per-pair queries: the connected fixpoint computes each class's full subsumer set.

**Termination:** one reused successor context per function term (`4045-4075`), core
reuse by content hash (`4081-4118`), a shared TBox closure precomputed once
(`4120-4141`), and a redundancy trie keeping only a maximal antichain of clauses
(`1745-1763`).

### 1.4 The hypertableau — a racing competitor, not the core

`hypertableau.rs` (~15.8k LOC) is KM's own tableau: **hyperresolution + disjunctive
*branching* + blocking** (ancestor-subset + pairwise signature blocking for SHIQ). Its
verdict is `Option<bool>` (`consistent`, `9973-10068`), and it classifies by
**per-pair probes**: Phase 1 per-class SAT (`A` consistent?), Phase 2 per-pair
`A ⊓ ¬B` unsat to confirm `A ⊑ B` (`10729-10900`). It runs as a **concurrent race
arm against CB, with CB-preference** — first sound+complete finisher wins, the loser
is killed; the HT answer is taken only when it beats CB (`orchestrate/race.rs:574-595,
1175-1250`). So on hard disjunctive/SROIQ inputs KM effectively runs *both* paradigms
and keeps whichever converges first.

### 1.5 `konclude_ht` — a port in progress

`konclude_ht/` is a faithful port of Konclude's C++ hypertableau (saturation →
completion → KPSet classifier). Per `STATUS.md`/`PORT.md` it is **~72-75% live by
function surface**: core completion loop + first expansion rules live, ~25-28% of
expansion rules are `W*-DEFER` stubs, the full taxonomy scheduler is deferred. It is
reachable via `KM_HT_BRIDGE` / the `cb_to_ht` reverse-Skolemization bridge
(`orchestrate/cb_to_ht.rs`) on a certified production fragment, and defers to CB
otherwise. Treat it as *not yet the general classify engine*.

---

## Part 2 — rustdl's classify

### 2.1 Pipeline

`classify(onto)` → `convert_ontology` (NNF via `normalize.rs`, GCIs absorbed into
lazily-fired triggers via `absorb.rs`, told tables, locality) → a **fragment gate**
that chooses one of two paths:

```
if pure-EL  OR  saturator_complete_fragment (EL + role hier/chains/transitivity/
               functional + inverse-functional witness-merge + domain/range):
   SATURATION-ONLY FAST PATH  (Horn-shortcircuit, classify.rs)  -> complete
else (∀, ≤n, ⊔, nominals, inverse *use*, DisjointClasses, ABox, …):
   HYBRID PER-PAIR PATH
```

### 2.2 EL forward-saturation fast path (complete on its fragment)

`owl-dl-saturation` is a single-file **consequence-based EL engine** (ELK-style,
Kazakov et al.): one forward fixpoint over atomic classes — told subsumption,
conjunction triggers, CR5 ∃-propagation, CR9 role hierarchy, length-≤2 chains +
transitivity, domain/range, functional witness-merge, Bot detection. Like KM's CB
core it computes **all subsumptions in one saturation** and reads them off. It is
**sound and complete only on that EL/Horn fragment**, and is **forward-only**: no
∀-rule, no general disjunction, and **no backward (Pred-style) propagation**.

### 2.3 Hybrid per-pair path (off the complete fragment)

`PreparedOntology::from_internal` snapshots the post-absorb state once; the classifier
is **top-down** (`classify_top_down_internal` / `find_direct_parents_top_down`) to
avoid the full O(n²) sweep. Each remaining subsumption `C ⊑ D` reduces to
**unsatisfiability of `C ⊓ ¬D`**, decided by:

- the **hypertableau "wedge"** (`hyper.rs`) as the default accelerator: Horn
  hyperresolution + disjunctive **branching** + double-blocking, with **`trust_sat`**
  — a `Sat` verdict is taken as "not subsumed" *without* consulting the full tableau
  (sound iff the wedge is complete on the workload; can MISS, never FP);
- the full **SROIQ tableau** (`graph.rs`/`trail.rs`/`search.rs`) for the rest:
  completion graph + log-and-undo trail + `⊔`-search with **dependency-directed
  backjumping** (`branch_id` + `DepSet` clash dependencies);
- a **per-class label heuristic** (built once from per-class wedge SAT) that prunes
  `subsumes_via_tableau` when `D ∉ labels(C)` — a counter-model short-circuit.

Disjunction is thus handled by **tableau case-split branching + backjumping**, not by
disjunctive clauses.

### 2.4 Soundness/completeness posture

FP=0 corpus-wide (no false subsumption on any measured ontology). Complete *by
construction* on EL/Horn (the fast path); off-fragment the default `trust_sat`
classifier is a **sound, empirically-near-complete-but-not-guaranteed-complete**
approximation (set `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` for the slower fall-through). The
saturator's missing ∀/backward-propagation/disjunctive-clause reasoning is exactly
what forces the off-fragment work onto the per-pair tableau.

---

## Part 3 — Head-to-head

| Axis | KM | rustdl |
|---|---|---|
| **Paradigm** | consequence-based (forward) throughout; HT as a race competitor | EL consequence-based *fast path* + refutation (per-pair tableau/wedge) for the rest |
| **Classification granularity** | ONE connected saturation over N per-class contexts → read off all subsumers | EL fast path: one saturation. Else: top-down **per-pair `C⊓¬D` probes** |
| **Disjunction** | kept in clause **heads**, resolved **by consequence** — no case split, no backtracking | tableau **branching** (⊔-rule) + dependency-directed **backjumping** |
| **Backward reasoning** | explicit **Pred rule** (successor ⇒ predecessor back-substitution) | none in the saturator (forward-only); off-fragment falls to the tableau |
| **Engine selection** | feature-based **router** + learned tree + **portfolio race** of CB/ELC/HT | single orchestrator: fragment gate → fast path or hybrid; `trust_sat` accelerator |
| **Hypertableau role** | racing competitor (CB-preference), per-pair probes | the *default* accelerator (wedge), per-pair probes, trust_sat |
| **Completeness** | sound+complete on the CB fragment (calculus-complete), +HT race for the hard tail | complete on EL/Horn; near-complete (trust_sat) elsewhere |

### 3.1 Why the granularity difference matters

KM computes a class's *entire* subsumer set from its context in the shared fixpoint;
adding classes adds contexts to one connected saturation. rustdl, off the EL fast
path, issues an independent refutation per candidate pair. This is the same structural
gap this repo measured on **realization** (rustdl DNFs where Konclude/CB read types
off one model): per-pair probing is O(individuals×classes) / O(classes²) independent
hard searches, whereas the CB style amortizes into one saturation. On the *EL/Horn*
fragment rustdl already *is* one-saturation (and competitive/faster on EL — see the
whelk/EL benchmarks); the gap is entirely in the off-fragment per-pair regime.

### 3.2 Why the disjunction difference matters

rustdl's hardest walls (the "wine-wall", the SROIQ DNF tail) are **branching**
explosions: the tableau case-splits on `⊔`, and — per this repo's wine-wall
analysis — nominal merges fold causation into every clash's dependency set, so
backjumping/caching/lemma-learning all degrade together. KM's CB core never
case-splits: a disjunction is a clause that is *resolved forward*, so it cannot induce
that search blow-up (its cost reappears as derived×derived clause growth instead,
bounded by redundancy elimination + the `branch_ordered` guard). This is the concrete
mechanism behind "Konclude/CB is fast on wine because its architecture doesn't create
dense dependency chains." It is *not* a caching trick; it is the paradigm.

### 3.3 Soundness — the empirical crossover

The full-ORE 4-reasoner run recorded in this repo found **rustdl FP=0** at scale while
**KM had genuine false positives on ~10 ontologies** (concrete-domain / datatype-range
collapse, e.g. `FastExposure ⊑ SlowExposure`). So KM's calculus-complete CB core buys
completeness on its fragment but its *datatype* layer is where soundness broke;
rustdl's conservatism (sound under-approximation, `trust_sat` only ever MISSes) is
where it wins. Different risk postures: KM optimizes for calculus completeness +
portfolio coverage; rustdl optimizes for FP=0 and degrades to sound-partial.

### 3.4 Convergences (not everything differs)

Both treat EL by forward consequence and read subsumptions off a saturation on that
fragment. Both carry a **hypertableau that classifies by per-pair `C⊓¬D` probes with
blocking** — KM's `hypertableau.rs` and rustdl's wedge are close cousins (hyperres +
disjunctive branching + blocking); the difference is KM keeps it as a *race
competitor to* a CB core, whereas rustdl makes it the *primary* off-fragment engine
with `trust_sat`. Both use dependency sets for backjumping in that tableau.

### 3.5 Architecture posture

KM spends complexity on **breadth**: many engines + a router + a learned tree + a live
race, plus an in-progress faithful Konclude port. rustdl spends it on **a single tuned
hybrid** with a strict fragment gate and a trust_sat accelerator, and treats the hard
tail as an accepted, characterized limitation rather than a portfolio to be widened.

---

## Part 4 — Implications for rustdl

1. **The off-fragment per-pair regime is the structural gap**, and it is the same one
   behind both the classify DL-tail and the realize DNF-tail. The consequence-based
   answer (compute a class's full subsumer set in one connected saturation, with
   backward **Pred** propagation and **disjunctive clauses instead of branching**) is
   the theoretically-grounded fix — and it is exactly the "Sequoia-style contexts"
   direction this repo has scoped before (and deferred as a large re-architecture).
   KM is a working existence proof that the port is feasible in Rust.
2. **Backward propagation (Pred) is the specific missing rule** in rustdl's forward EL
   saturator that a CB extension would add; it is what lets a successor's consequences
   refine a predecessor without a tableau probe.
3. **Disjunction-as-clauses vs branching is the lever for the wine-wall class** — the
   one place rustdl's search genuinely explodes and CB structurally does not.
4. **But KM's datatype FPs are the cautionary note**: a CB re-architecture must not
   inherit the concrete-domain unsoundness; rustdl's FP=0 discipline is the asset to
   protect. A CB core would need the same sound datatype layer rustdl already has.

Net: KM and rustdl agree on EL (both consequence-based, one saturation) and both keep a
per-pair hypertableau; they diverge on *everything off EL* — KM stays consequence-based
(disjunctive clauses, Pred, one-saturation read-off, portfolio race), rustdl switches
to per-pair refutation with tableau branching under a trust_sat accelerator. That
divergence explains both rustdl's soundness edge and its DL-tail / realize
scale disadvantage.

---

## Part 5 — Runtime profiling of KM's CB engine (measured)

**Method.** No `perf`/`valgrind` on this host and KM's release profile carries no
debug symbols, so symbol-level sampling wasn't viable. Instead I used **KM's own
built-in per-rule profiler** — `KM_STATS=1 KM_PROF_TIME=1` — which the engine author
added precisely to "split the saturation loop's cost across its phases"
(`engine.rs:194-203`). I drove the pure CB engine directly via the worker pipeline
`km ofn <ont>` (→ clause JSON) `| km engine` (the orchestrator otherwise captures the
worker's stderr to a temp file, hiding the stats). rustdl walls are the CLI `classify`
at default budgets. Onts: `ore_ont_9053` (988 KB → 6660 clauses) and `ore_ont_10197`
(1.8 MB → 1603 clauses), both off rustdl's EL/Horn fast-path.

**Per-rule breakdown (KM CB engine, cumulative wall per rule):**

| ont | engine wall | `add_clause` | `hyper` | `propagate` | `subsume` | `pred_local` | `eq` | hyper_calls |
|---|---|---|---|---|---|---|---|---|
| 9053 | 757 ms | **176 ms (49%)** | 100 ms (28%) | 75 ms (21%) | 6 | 3 | 0 | 33 396 |
| 10197 | 86 ms | 0.8 ms | 0.2 ms | 1.0 ms | 0 | 0 | 0 | 269 |

Two findings the profile makes concrete:

1. **The CB engine's own hotspot is clause management, not hyperresolution.** On the
   non-trivial ont (9053) the dominant cost is **`add_clause` (~49%)** — inserting
   derived clauses through the forward/backward-subsumption redundancy trie — then
   **`hyper` (~28%)** and inter-context **`propagate` (~21%)**. Hyperresolution, the
   "signature" CB step, is a minority; the redundancy/antichain maintenance that keeps
   the disjunctive clause set from blowing up is where the time goes. (This matches the
   engine comment calling `add_clause`/`propagate` the "beat-Konclude lever" target.)

2. **KM's classify is not literally one saturation — it is a primary saturation plus
   *hundreds* of small repair re-saturations.** The engine emitted **325 `KM_STATS`
   dumps for 9053 and 191 for 10197** — one per `Engine::saturate()` completion. So the
   Part-1 "one saturation reads off all subsumptions" is the *calculus* ideal; the Rust
   implementation runs a bulk saturation and then a root-ordered **repair regime** that
   re-saturates to recover ordering-sensitive missed pairs (`reasoner.rs` "split /
   root-ordered / repaired_pairs"). Each re-run is small and shares the precomputed TBox
   closure, so the total stays sub-second. (Nuance, not a contradiction: at the *Engine*
   level each run is still one saturation with disjunctive-clause read-off; the
   *classify* wrapper stacks many of them.)

**Head-to-head classify wall (same onts):**

| ont | KM CB classify | rustdl classify | ratio |
|---|---|---|---|
| 9053 | **0.76 s** | 53.4 s | ~70× |
| 10197 | **0.086 s** | 26.7 s | ~310× |

On these two off-EL-fragment ABox ontologies KM's CB classify is **70–310× faster**
than rustdl's. This is the §3.1 granularity thesis with numbers: rustdl runs its
top-down **per-pair** `C ⊓ ¬D` tableau regime over these (10197 was the unbounded-
classify case; even bounded it is a per-pair sweep), while KM amortizes into the CB
(multi-)saturation. *Caveat:* verdict-equality on these two specific onts was not
re-checked here — the soundness/completeness picture is the prior full-ORE result
(rustdl FP=0 and ~90% exact-match; KM more complete on some, with genuine datatype FPs
on ~10 onts). The number is a *throughput* comparison on the classify path, not a
soundness claim.

**Honest counterpoint — the router matters.** Forcing the CB route is *not* a universal
win: `km classify --route cb_absorb16` on `ore_ont_14379` **timed out (>180 s)** — the
same ont rustdl's bounded classify does in ~2 s and KM's *default* `auto` route handles
by routing elsewhere (HT race / a different bundle). So KM's speed advantage on 9053/
10197 is the CB engine on ontologies suited to it; its portfolio router exists precisely
because no single engine (CB included) wins everywhere. rustdl's single hybrid takes the
opposite bet.

**Net of the profile:** it confirms the paradigm-level story — KM turns classification
into a forward CB saturation (fast here, 70–310×) whose *internal* cost is clause-set
redundancy management rather than hyperresolution, and whose implementation quietly does
a repair-driven multi-saturation rather than a single pass; rustdl's off-fragment
per-pair refutation is the orders-of-magnitude-slower regime on exactly these inputs,
which is the same structural gap behind its DL-tail and realize frontiers.

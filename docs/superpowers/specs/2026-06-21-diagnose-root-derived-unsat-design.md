# `rustdl diagnose` — root/derived unsatisfiability diagnosis (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/diagnose-root-derived-unsat`

This is **sub-project A** of the explanation/debugging suite (the lane chosen after
`justify` + `prove` shipped). Companion future sub-projects, explicitly out of scope
here: **B** repair suggestions (hitting set over justifications), **C** laconic /
fine-grained justifications (axiom-fragment granularity), **D** visual/web rendering.

## Goal

One command that tells the user **where to start fixing** a broken ontology:
distinguish **root** unsatisfiable classes (genuine causes) from **derived** ones
(unsatisfiable only because they depend on a root). This is the classic
Schlobach & Cornet root/derived diagnosis (what Protégé's debugger and Pellet's
"root unsatisfiable classes" provide) — a niche Konclude (no built-in justification
or explanation) does not serve.

## Soundness framing

Root/derived is a **diagnostic heuristic**, not a logical entailment, so classic
FP=0 (no false subsumptions) is not *directly* at stake. The analogous discipline:

1. The unsatisfiable-class set we partition is exactly the classified set (already
   FP=0/MISSED=0). `diagnose` adds **no entailments** and is **read-only** over
   classification → it cannot introduce a false subsumption. FP=0 is untouched by
   construction.
2. The partition must be **conservative**: never hide a true root by mislabeling it
   "derived." Over-reporting roots is safe (the user inspects a harmless extra);
   under-reporting (a hidden root) is the dangerous failure that breaks the
   "fix roots → everything resolves" mental model.

## Architecture & placement

- New module `crates/owl-dl-reasoner/src/diagnose.rs` — the dependency-graph builder,
  the partition logic, and the public `diagnose()` entry point.
- New CLI subcommand `diagnose` in `owl-dl-cli`.
- **Reuses, does not reinvent:** `is_consistent` / `classify` (consistency verdict +
  unsatisfiable-class set), and `justify`'s `find_one_justification` /
  `find_all_justifications` / inconsistency path (`Entailment::Inconsistent`). No new
  reasoning engine, no new entailments.

Each unit has one responsibility: the dependency graph is a pure function of
(unsat set, logical axioms); the partition is a pure function of the graph; the CLI
formats results. They can be tested independently.

## Algorithm

**Consistency verdict tracks `classify`'s view, not the slow main-tableau
`is_consistent`.** This is both faster and more faithful: the unsat set and the
inconsistency verdict come from the same engine, so `diagnose` always agrees with
what `rustdl classify` shows the user. An ontology `classify` itself fails to flag
inconsistent gets a partition — honest, because `classify` would surface those same
classes. Inconsistency is detected by three cheap, sound signals, in order: (a) the
ABox-saturation pre-check (catches family-style ABox clashes `classify`'s own
`abox_check` misses), (b) `classify`'s own `inconsistent` flag, and (c) a definitive
`is_consistent` tiebreak invoked **only** when every declared class is unsatisfiable
— the signature of pure-TBox global inconsistency (`⊤ ⊑ ⊥`), which `classify` reports
as "all classes unsat" without flagging. The slow `is_consistent` path therefore
never runs on a normal ontology (it is gated behind all-classes-unsat, and is itself
fast on the inconsistent inputs that gate triggers on).

```
1. parse → convert (once)
2. ABox-saturation inconsistency pre-check (signal a)
   └─ clash → INCONSISTENT → justify the inconsistency, print. Done.
3. classify (top-down path, sets the inconsistent flag) → unsat set U
   ├─ classify flagged inconsistent (signal b) → INCONSISTENT → justify, print. Done.
   └─ continue
4. all-classes-unsat guard (signal c): if |U| == #declared classes, run is_consistent
   └─ inconsistent → INCONSISTENT → justify, print. Done.
5. U empty → print "coherent: no unsatisfiable classes." Done.
6. build the structural dependency graph G over U (edge C→D means "C depends on D")
7. partition U:
   - ROOT    = unsat class with NO outgoing edge to another unsat class
   - DERIVED = unsat class with ≥1 outgoing edge
8. justify each ROOT (find-one by default; --all opt-in), print the minimal axiom set
9. for each DERIVED class, report the root(s) it transitively reaches in G
```

### The edge set (the soundness crux)

An edge `C → D` (with `D ∈ U`) is drawn **only** when `D`'s unsatisfiability
*certainly forces* `C`'s — i.e. `D` appears in a positive, unsat-forcing position of
`C`'s definition (axioms with `C` on the LHS of `SubClassOf`, and `EquivalentClasses`
members, which give `C ⊑ Expr`).

| Axiom (C on LHS), with D ∈ U | Edge C→D? | Rationale |
|---|---|---|
| `C ⊑ D` (told subsumption) | **yes** | `C ⊑ D ⊑ ⊥` |
| `C ⊑ ∃r.D` | **yes** | `∃r.⊥ ≡ ⊥` ⟹ `C ⊑ ⊥` |
| `C ⊑ D ⊓ …` / `C ≡ D ⊓ …` | **yes** (per unsat conjunct) | a `⊥` conjunct forces `⊥` |
| nested positive `⊓`/`∃` combinations of the above | **yes** | same, structurally recursed |
| `C ⊑ ∀r.D` | **no** | constrains only successors; does not force `C` unsat |
| `C ⊑ ¬D` / `DisjointClasses(C,D)` | **no** | `¬⊥ ≡ ⊤`, no constraint on `C` |
| `C ⊑ ≤n r.D` | **no** | not unsat-forcing |
| a disjunct of `C ⊑ … ⊔ D ⊔ …` | **no** | `D` unsat just drops that disjunct |
| `D ⊑ C` (D on LHS) | **no** | wrong direction |

**Why this exact set is conservative.** A *spurious* edge gives `C` an outgoing edge
and labels it derived — if `C` were really a root, that hides a root (the dangerous
failure). A *missing* edge can only cause `C` to be labeled root when it is really
derived — a harmless extra. So we include an edge **only** when the dependency is
logically certain (the "yes" rows), and exclude every position whose contribution to
`C`'s unsatisfiability is conditional (the "no" rows). This guarantees we never hide a
true root; at worst we list a few harmless extra roots (e.g. a class unsat *only* via
`∃r.⊤ ⊓ ∀r.D` is reported as a root because the `∀` edge is intentionally omitted).

### Cycles

Mutually-referencing unsat classes (e.g. `C ≡ …D…` and `D ≡ …C…`, both unsat) form a
strongly-connected component with no single source. Report **all members of the SCC
as co-roots** (each justified). Conservative: a cycle has no unique cause, so hiding
any member would risk hiding the real one. Implementation: condense G into its SCCs;
an SCC is a root-cluster iff it has no edge leaving it to *another* SCC in U.

### Transitive root attribution

For a derived class `C`, its reported root(s) are the roots reachable from `C` in `G`
(following the dependency edges to their sources). A chain `C → D → E(root)` reports
`C ⇐ E`. A class may reach multiple roots; report all.

## Output format

```
# diagnose: pizza-broken.ofn
# consistency: consistent
# unsatisfiable: 12  (3 root, 9 derived)

## ROOT unsatisfiable classes (fix these first)
ROOT  ex:CheeseyVegetable
  justification (2 axioms):
    CheeseyVegetable SubClassOf Vegetable
    CheeseyVegetable SubClassOf hasTopping some Cheese
  derives: ex:SpicyCheeseyVegetable, ex:HotCheeseyVegetable

## DERIVED unsatisfiable classes (likely resolve once roots are fixed)
DERIVED ex:SpicyCheeseyVegetable   <= ex:CheeseyVegetable
DERIVED ex:HotCheeseyVegetable     <= ex:CheeseyVegetable
```

For an inconsistent ontology:

```
# diagnose: family.ofn
# consistency: INCONSISTENT
## responsible axioms (one justification):
  <the inconsistency justification, reused from the shipped path>
```

Flags (mirroring `justify`):
- `--labels` — gloss each entity with its `rdfs:label` (reuse justify's glossing).
- `--all` — print all minimal justifications per root (default: one), capped by `--max`.
- `--max N` — cap for `--all` (default 10, same as justify).

## Testing

- **Unit (dependency-graph builder)** — pure-function tests on synthetic axiom sets:
  told-subsumption chain, existential chain, conjunct-forces-unsat, nested
  `∃`/`⊓`, an SCC/cycle; and **negative controls** that must *not* create an edge:
  `C ⊑ ∀r.D`, `DisjointClasses(C,D)`, `C ⊑ ¬D`, `C ⊑ ≤n r.D`, a `⊔` disjunct, and the
  wrong-direction `D ⊑ C`.
- **Partition** — given a hand-built graph, assert ROOT/DERIVED sets and transitive
  root attribution; assert SCC members are co-roots.
- **Integration** — a crafted fixture with a known root→derived cascade →
  assert the exact partition and that each root's justification is the expected
  minimal set; `family.ofn` → `diagnose` reports INCONSISTENT + the responsible axioms.
- **Corpus conservation invariant** — on every corpus fixture that has unsatisfiable
  classes (e.g. sio), assert `roots ∪ derived == classified-unsat-set` **exactly**
  (nothing dropped, nothing added) and that `diagnose` does not crash; assert the
  classification closure is byte-identical with/without `diagnose` (it is read-only).
- **Performance** — `diagnose` wall ≈ classify wall + (small) per-root justify;
  assert no pathological blowup (the graph build is linear in axiom size; the only
  reasoning cost is one find-one justification per root, over the small root set).

## Out of scope (v1)

- **B** repair suggestions, **C** laconic justifications, **D** visual rendering.
- Non-class diagnostics (unsatisfiable *properties*, etc.) — classes only in v1.
- `diagnose` emits text; sub-project D will later render this same output visually.

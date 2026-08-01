# R1 findings — `owl-dl-core/src/convert.rs` + `data_axioms.rs` (conversion / DKey front end)

## Binary provenance — read this first, it very nearly went wrong twice

1. **My opening `RUSTUP_TOOLCHAIN=stable cargo build --release --workspace 2>&1 | tail -5` reported
   "exit code 0" and had in fact FAILED** — `cargo` is not on `PATH` in this shell at all
   (`~/.cargo/bin` contains only `samply`; the toolchains live at
   `~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`). The `| tail` swallowed the real
   status. That is exactly the trap the brief names; I fell into it and then caught it.
   *Recovery:* `git log --since="2026-07-31 19:00" --name-only -- crates/` returns **nothing** — every
   commit after the pre-existing `target/release/rustdl` (Jul 31 19:14) is docs/bench only — so that
   binary was in fact source-current for `crates/` at `509efc8`.
2. **A concurrent reviewer (R4) rebuilt `target/release/rustdl` at 00:56:20, mid-session**, adding
   env-gated (`RUSTDL_GATE_PROBE`, unset in all my runs) report-only instrumentation to
   `owl-dl-reasoner/src/classify.rs` — a different crate from the one under review.
3. **Therefore every load-bearing number below was re-run on a pinned, clean binary**
   `scratchpad/rustdl-509efc8-clean` (45 304 552 B, built 01:15 from a detached worktree at
   `509efc8` with `PATH=~/.rustup/toolchains/stable-.../bin:$PATH`, `build rc=0` checked directly, not
   through a pipe). Every verdict and wall reproduced:
   `only_neg` 0 unsat / 1 under `BOUNDED_DKEY_DISJOINT=0` / 1 under `GATE=0 SPLIT=0`;
   `only_direct` 1; `oneof` pairwise `C≤E yes` and the other five `no`; `oneof_clash` only `H`;
   `negzero` `equiv`; `ore_ont_9347` **10.71 s / 124 MB**; `ore_ont_16632` **18.20 s / 2.50 GB**;
   `16632_collapsed` **3.49 s / 60 MB**. (Full log: `scratchpad/verify.txt`.)

All walls single-thread (`RAYON_NUM_THREADS=1` for profiles), `/usr/bin/time -f "%e %M"`.
Oracle = Konclude v0.7.0-1138 (`/data/dumontier/reasoners/Konclude-.../Binaries/Konclude
classification`), fed hand-written OWL/XML. Profiles = gdb `bt 40` sampling of the live process,
inclusive attribution (a frame counts if it appears anywhere in the sample's stack).

**No source file in `/data/dumontier/rustdl` was edited.** The temporary worktree
`scratchpad/wt509` was removed at the end.

---

## CONFIRMED

### F1 — incorrect (**D10 class**) — `convert.rs:2431-2484` (`seed_dkey_subsumptions`)

**Mechanism.** `data_range_dkey` (`convert.rs:668-706`) mints **twelve** DKey buckets: seven
interval/string (`untagged int`, `f:`, `db:`, `dec:`, `date:`, `dt:`, `str:`) and five numeric-
`DataOneOf` (`io:`, `fo:`, `deo:`, `dao:`, `dto:`, `convert.rs:690-699`). `seed_dkey_subsumptions`
collects **only the seven** (`convert.rs:2431-2468`) and therefore calls neither `seed_bucket` nor
`seed_disjoint_bucket` for the five oneof buckets. Consequences:

* no told `DKey ⊑ DKey` edge **within** a oneof bucket (`io:1 ⊑ io:1;2` is never seeded);
* no told edge **between** a oneof key and the interval key of the *same value space*
  (`DataHasValue(p,1)` → untagged `1:1`; `DataOneOf(1)` → `io:1` — two ClassIds for one value set);
* no `DisjointClasses` for oneof keys, so the D11b `∃p.DKey(v) ⊓ ∀p.DKey(r)` membership clash cannot
  fire when `r` is a `DataOneOf`.

Meanwhile the fragment gate certifies completeness: `∃p.DKey(io:…)` is an EL concept, so `is_pure_el`
passes and the banner reads *"saturator alone is complete"*. Exactly the shape CLAUDE.md calls the
D10 class.

**How verified.** Two fixtures, both with a working same-file control, both adjudicated by Konclude.

*Subsumption half* (`scratchpad/oneof.ofn` / `oneof.owx`):
```
C ≡ DataHasValue(p,"1"^^xsd:integer)                      -- untagged  1:1
D ≡ DataSomeValuesFrom(p, DataOneOf("1","2"))             -- io:1;2
E ≡ DataSomeValuesFrom(p, xsd:integer[>=0,<=5])           -- untagged  0:5   (CONTROL)
F ≡ DataSomeValuesFrom(p, DataOneOf("1"))                 -- io:1
```
| pair | Konclude | rustdl |
|---|---|---|
| `C ⊑ E` (control, interval↔interval) | yes | **yes** |
| `C ≡ F` | **EquivalentClasses(F,C)** | no / no |
| `F ⊑ D` | yes | **no** |
| `D ⊑ E` | yes | **no** |

rustdl banner on that file: `# fragment: pure-EL (trust_sat sound by construction; saturator alone
is complete)`.

*Clash half* (`scratchpad/oneof_clash.ofn` / `oc.owx`): `G ⊑ ∀p.DataOneOf(1,2)`, `G ⊑ DataHasValue(p,3)`
next to the interval control `H ⊑ ∀p.[1,2]`, `H ⊑ DataHasValue(p,3)`. Konclude:
`EquivalentClasses(Nothing, H, G)`. rustdl: `# unsatisfiable: 1 … unsat H` — **G missed**, banner
`# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)`.

**Falsifiable prediction.** Collect the five oneof buckets in `seed_dkey_subsumptions` and seed them
through the existing `seed_bucket`/`seed_disjoint_bucket` (set-containment / set-disjointness), plus
the two *same-value-space* cross-edges (`untagged ↔ io:` and `f: ↔ fo:` / `db: ↔ ?` — note `fo:`
is currently `f64`-keyed and normalizes signed zero, `f:` does not, see F6). Then: `oneof.ofn`
reports `equiv C F`, `F ⊑ D`, `D ⊑ E`; `oneof_clash.ofn` reports both G and H unsat; the curated
FP=0 net is **byte-identical** (no curated fixture uses a numeric `DataOneOf` — grep confirms
`DataOneOf` appears only in the `str:` fixtures), and `ore_ont_{9347,5368,16632,7607}` concept_rules
are unchanged (none of them has a `DataOneOf`).

**Do not confuse with:** `ore_ont_12174`-style `xsd:string` `DataOneOf`, which *is* seeded (the
`str:` bucket handles `DataOneOf`-of-strings). The gap is the five **numeric/temporal** oneof buckets.

---

### F2 — inefficient — `convert.rs:3075-3092` (`seed_bucket`)

**Mechanism.** Unguarded ordered-pair double loop over every key in a bucket — `k²−k` calls to the
bucket's `subset` predicate — with no index, no early exit, and (unlike its sibling
`seed_disjoint_bucket`, `convert.rs:2964`) no component argument. Emission is sparse; the *scan* is
quadratic.

**How verified — two independent methods that agree.**

1. *Sampling profile of `ore_ont_9347`* (100 gdb samples over the full 10.76 s run,
   `classify --saturation-only`, single thread):

   | frame | share |
   |---|---|
   | `convert_ontology` | 100.0% |
   | `seed_dkey_subsumptions` | 96.0% |
   | **`seed_bucket`** | **96.0%** |
   | `derive_data_axioms` | 2.0% |
   | `build_told_tables` | 1.0% |

   `9347` has 17,918 distinct `xsd:string` literals and (per the shipped merging-gate spec) **zero**
   `DataSomeValuesFrom`/`DataAllValuesFrom`/`DataHasValue`/`DataPropertyRange`, so every `str:` key is
   a **singleton** `StrSet::Set` and no `Top` key exists ⇒ `StrSet::subset` is false for every one of
   the 321M ordered pairs ⇒ **`seed_bucket` emits zero axioms and consumes 96% of the wall.**

2. *Synthetic scaling*, `classify --saturation-only`, one non-merging data property, n distinct
   values, wall / peak-RSS:

   | n | `xsd:string` (key minted) | `xsd:integer` (key minted) | `xsd:int` (**dropped**, no key) |
   |---|---|---|---|
   | 4 000 | 0.55 s / 24 MB | 0.14 s / 24 MB | 0.05 s / 24 MB |
   | 8 000 | 2.14 s / 44 MB | 0.42 s / 44 MB | 0.10 s / 44 MB |
   | 16 000 | 7.83 s / 93 MB | 1.40 s / 93 MB | 0.22 s / 82 MB |

   ×3.8 per doubling with **linear RSS** ⇒ quadratic *work*, not quadratic *output*. Yields
   ≈29 ns per `StrSet::subset` and ≈4.6 ns per `IntegerRange::subset`. Cross-check on 9347:
   321M × 29 ns = 9.3 s of a 10.76 s wall — agrees with the 96% profile figure.
   Attribution to `seed_bucket` rather than `seed_disjoint_bucket` on the synthetics is pinned by the
   flag A/B at n=4 000 `xsd:integer`: default (gate ON) **0.14 s / 24 MB**;
   `RUSTDL_DKEY_MERGING_GATE=0` **3.74 s / 276 MB**; `RUSTDL_BOUNDED_DKEY_DISJOINT=0` **12.70 s / 3.0 GB**
   — i.e. the disjointness path emits nothing at all in the default configuration.

**The 96% does NOT generalize — measured share is workload-dependent.** On `ore_ont_16632`
(191 samples, 18.6 s): `saturat*` 50.3%, `convert_ontology` 38.7%, `seed_dkey_subsumptions` 17.8%
(`seed_disjoint_bucket` 13.6% + **`seed_bucket` 4.2%**), `build_told_tables` 11.5%,
`derive_data_axioms` 7.3%. That 4.2% is the same order as the "real cost was 6%" figure the brief
warns about — I reproduce it, and the 96% on 9347 is a *different ontology* where the disjointness
and saturator costs have already been eliminated by the shipped gates.

Predicted `seed_bucket` share from `Σ_bucket k²` × per-call cost (distinct-literal census):

| ont | k (str/float) | Σk² | wall | predicted `seed_bucket` |
|---|---|---|---|---|
| 9347 | 17 918 | 321M | 10.76 s | ~9.3 s (**profiled 96%**) |
| 7607 | 8 527 | 72.7M | 6.93 s | ~2.1 s (~30%) |
| 12182 | 5 450 | 29.7M | 2.38 s | ~0.9 s (~36%) |
| 11126 | 6 851 / 1 626 | 49.6M | 16.97 s | ~1.4 s (~8%) |
| 10425 | 4 542 / 720 | 21.1M | 9.81 s | ~0.6 s (~6%) |
| 16632 | 6 933 / 1 788 | 51.3M | 18.59 s | ~1.4 s (**profiled 4.2%**) |
| 2504 | 20 677 | 427.5M | (>150 s) | ~12 s |

**Assessment of the recorded-but-unbuilt fix ("skip pairs whose `sup` is a singleton").**
*Right in direction, not the whole story, and it needs one guard:*
* it is exactly right for the dominant case — all-singleton buckets (9347, 2504, 4141): the inner
  loop collapses to O(k × #non-singleton keys) ⇒ O(k) when only a bare-datatype `Top` key exists;
* it does **nothing** for a bucket of overlapping faceted ranges (a general fix must index — e.g.
  sort by bound and only compare against candidate supersets, or bucket by `min`);
* its stated justification is **false in `f:`/`db:`** — see **F6**. Applied verbatim it silently
  drops `DKey(f:-0.0) ↔ DKey(f:0.0)`.

**Falsifiable prediction.** With the skip (guarded per F6): `ore_ont_9347` 10.76 → ~0.5 s,
`7607` 6.93 → ~4.9 s, `12182` 2.38 → ~1.5 s; `16632`/`10425`/`11126` change by <10%; every closure in
the FP=0 net stays byte-identical (the curated fixtures have no all-singleton DKey bucket of size >5).

---

### F3 — incorrect (**D10 class**) — `convert.rs:2210` + `2688-2724` + `2784-2807` + `2885-2911`

**Mechanism.** `seed_dkey_subsumptions` (hence `dkey_components`) is called at `convert.rs:2210`,
**inside `convert_ontology`, before NNF**. Both role classifications match only syntactic shapes that
exist pre-NNF:
* `merge_inducing` matches `ConceptExpr::Max(_,r,_) | ConceptExpr::All(r,_)` (`convert.rs:2719`);
* `collapse` matches `Max` / non-pure-DKey `All` (`convert.rs:2799-2804`);
* `broadcast_in` is recorded only at the `ConceptExpr::All` arm (`convert.rs:2893`) and the
  `ObjectPropertyRange` loop (`convert.rs:2901`).

A universal restriction that exists **only after NNF** is therefore invisible. The reachable OWL 2 DL
shape is the double negation
`ObjectComplementOf(DataSomeValuesFrom(q, DataComplementOf(r)))` ⇒ NNF ⇒ `∀q.DKey(r)`:
pre-NNF the pool holds `Not(Some(q, Not(DKey)))`, so `q` is marked neither merge-inducing nor
collapse/broadcast, and the disjointness pair `DKey(9:9) ⟂ DKey([0,5])` is dropped — after which the
post-NNF `∀q.DKey([0,5])` has nothing to clash against.

**How verified.** `scratchpad/only_neg.ofn`:
```
Negated ⊑ ¬∃q.¬(xsd:integer[>=0,<=5])      -- i.e. ∀q.[0,5]
Negated ⊑ DataHasValue(q,"9"^^xsd:integer)
```
Konclude: `EquivalentClasses(Nothing, Negated)`. rustdl, `unsat` count by configuration:

| configuration | unsat |
|---|---|
| default | **0** |
| `RUSTDL_LABEL_HEURISTIC=0` | 0 |
| `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` | 0 |
| `RUSTDL_HYPERTABLEAU=0` | 0 |
| `RUSTDL_DKEY_MERGING_GATE=0` | 0 |
| `RUSTDL_DKEY_COLLAPSE_SPLIT=0` | 0 |
| **`RUSTDL_DKEY_COLLAPSE_SPLIT=0 RUSTDL_DKEY_MERGING_GATE=0`** | **1** |
| **`RUSTDL_BOUNDED_DKEY_DISJOINT=0`** | **1** |

Banner in every missing case: `# fragment: Horn (… hyper Horn fixpoint is complete)`. The
directly-written control `Direct ⊑ ∀p.[0,5] ⊓ ∃p.{9}` (`scratchpad/only_direct.ofn`) **is** caught at
every setting, so the calculus can do it — only the gate's role classification fails.

Three things this establishes precisely:
1. it is a **completeness regression** versus pre-v0.3.29 behaviour (`BOUNDED=0` catches it);
2. **either gate alone is sufficient to lose it** — turning off only one does not restore it, so this
   is not attributable to a single 2026-07-30 change;
3. the fragment banner certifies completeness while the axiom is dropped ⇒ D10 class.

**Side observation, `scratchpad/nnf_forall.ofn`** (the two classes in one file): default finds **both**
unsat, `RUSTDL_DKEY_MERGING_GATE=0` finds **neither** (deterministic over 2 runs;
`# label heuristic: pruned=1` in the flag-off run vs `pruned=0` default). Adding sound entailed
disjointness axioms therefore *loses* entailments — so the "flag-OFF byte-identity" gate the two
shipped DKey specs rely on does **not** establish behavioural equivalence, and DKey-pair volume/order
is not answer-neutral. I did not root-cause this (it lives in the wedge/label cache, outside this
subsystem); recorded as an observation, not a finding.

**Falsifiable prediction.** Computing `merge_inducing`/`collapse`/`broadcast_in` over the NNF'd axiom
set (or, minimally, additionally treating a pooled `Not(Some(r,_))` as an `All(r,_)` occurrence at
`convert.rs:2719`/`2802`/`2893`) makes `only_neg.ofn` report `unsat Negated`; `ore_ont_9347`
concept_rules stays **113** and `ore_ont_5368` stays **18,620,251** (neither contains a negated data
existential), so the discriminating check from the shipped spec still passes.

---

### F4 — missing — `data_axioms.rs:1680/1686/1695` (and the sibling `parse_*_range` functions)

**Mechanism.** The datatype recognizers accept exactly seven IRIs: `xsd:integer`, `xsd:decimal`,
`xsd:float`, `xsd:double`, `xsd:date`, `xsd:dateTime`, `xsd:string` (grep of `XMLSchema#` in
`data_axioms.rs` returns only those seven). Every other OWL 2 datatype-map entry — the whole bounded
integer family (`xsd:int`, `long`, `short`, `byte`, `nonNegativeInteger`, `positiveInteger`,
`negativeInteger`, `nonPositiveInteger`, `unsignedLong/Int/Short/Byte`), plus `xsd:boolean`,
`xsd:anyURI`, `xsd:normalizedString`, `xsd:token`, `xsd:hexBinary`, … — returns `None` from
`data_range_dkey`, which makes the **whole axiom** unrepresentable, not just its datatype content:
the ABox edge and the individual's typing are lost too.

**How verified.** `ore_ont_16632` stderr, default configuration:
`warning: 1882 axiom(s) not understood and dropped (DataPropertyAssertion: unsupported data range
×1874, DataPropertyRange: unsupported data range ×8)`. Its literal census is
`xsd:string 13 536 / xsd:float 2 004 / xsd:int 1 874 / xsd:integer 1` — the 1 874 dropped are exactly
its `xsd:int` assertions, and `DataPropertyRange(<…Year> xsd:int)` ×8 are the dropped ranges. The
`xsd:int` synthetic in F2's table is the same effect isolated: no DKey is minted at all, hence the flat
linear curve.

**Second instance, same mechanism, standard construct: `DatatypeDefinition` is never resolved.**
The in-tree fixture `crates/owl-dl-reasoner/tests/fixtures/datatype/datatype_definition.ofn`
(`DatatypeDefinition(:AgeAdult xsd:integer[>=18])` +
`EquivalentClasses(:Adult (:Person ⊓ ∃age.:AgeAdult))`) produces
`warning: 2 axiom(s) not understood and dropped (DatatypeDefinition ×1, EquivalentClasses ×1)` —
the drop **cascades from the datatype to the class definition**, so `Adult`'s defining
`EquivalentClasses` is lost entirely, while the banner still reads
`# fragment: pure-EL (… saturator alone is complete)`. A one-pass substitution of defined datatypes
before `data_range_dkey` would fix both this and the ORE occurrences.

This is **not** a silent drop — the graceful-degradation path (#43) records it in `DroppedAxioms` and
prints the warning, so the user has a signal. It is a plain coverage gap.

**Falsifiable prediction.** Mapping the bounded integer family onto `IntegerRange` with each type's
implied bounds (`xsd:int` → `[-2³¹, 2³¹−1]`, etc.) takes `ore_ont_16632`'s dropped count 1 882 → 8
(the `xsd:int` ranges recovered too ⇒ 0) and adds 21 keys to its int bucket; `ore_ont_7607`/`1685`
likewise recover their `xsd:int ×1` / `xsd:boolean ×3` / `xsd:anyURI ×2` ranges. Curated FP=0 net
unchanged (no curated fixture uses a bounded integer type).

---

### F5 — inefficient — `data_axioms.rs:3143-3164`, `3013-3031`, `3216-3225`

**Mechanism.** Three per-individual value scans re-walk the *whole* `ind_dp_vals` /
`ind_dj_values` map inside an outer loop, comparing `String`s:

* `emit_data_cardinality_violations_typed` (`3143-3164`):
  `for (class,dp,n,dr) in constraints { for (ind,types) in ind_types { for ((i,q),vals) in
  &ind_dp_vals { if i != ind { continue } … } } }` — O(|constraints| × |typed individuals| ×
  |(ind,prop) entries|).
* `emit_functional_dp_cardinality_violations` (`3013-3031`): identical shape, keyed on
  |functional dps| × |individuals|.
* `emit_disjoint_dp_same_value_clash` (`3216-3225`): a full `.iter().filter()` over `ind_dj_values`
  per disjoint pair, and `facts.ind_dj_values.get(&(ind.to_owned(), dq.clone()))` **allocates two
  `String`s per lookup**.

Additionally `derive_data_axioms` (`data_axioms.rs:72-98`) makes **five** full passes over the source
`SetOntology` (`extract_facts`, `propagate_intersection_bounds`, `emit_data_range_value_violations`,
`emit_functional_dp_cardinality_violations`, `emit_data_cardinality_violations_typed`), each building
a fresh `String` per IRI via `dp_iri`/`class_iri`/`individual_iri`.

**How verified.** `ore_ont_16632` sampling profile (191 samples): `derive_data_axioms` 7.3%, **all of
it** `emit_data_cardinality_violations_typed` (7.3%). Its census: 74 `DataMaxCardinality` constraints,
312 `ClassAssertion`, ~12k distinct `(ind, prop)` entries. The two siblings are inert on 16632 only
because it has **0** `FunctionalDataProperty` and **0** `DisjointDataProperties` (both functions
early-return); `ore_ont_5368` has 15 `FunctionalDataProperty`, so `3013-3031` is live there.

**Falsifiable prediction.** Re-keying `ind_dp_vals` as `BTreeMap<&str, BTreeMap<&str, BTreeSet<…>>>`
(individual → property → values) removes the inner scan from all three. `ore_ont_16632` loses ≈1.3 s
of its 18.6 s (7.3% → <1%); no answer changes anywhere (these functions only ever push `Top ⊑ Bot`).
This is a modest lever — do not fund it ahead of F2.

---

### F6 — incorrect (invariant, not soundness) — `convert.rs:2475` comment vs `convert.rs:81-95`

**Mechanism.** The comment that justifies F2's recorded fix —
*"distinct keys ⟹ strict subset, since equal ranges share one ClassId"* (`convert.rs:2475-2477`) —
is **false in the `f:` and `db:` buckets**. `tagged_float_dkey_iri` keys bounds by `f64::to_bits()`
(`convert.rs:83`), which distinguishes `-0.0` (`0x8000000000000000`) from `+0.0` (`0x0`), while
`FloatRange::subset`/`disjoint` compare with IEEE `==` (`data_axioms.rs:621`, `626`, `646`), for which
`-0.0 == 0.0`. So `DKey(f:-0.0)` and `DKey(f:0.0)` are **two ClassIds that are mutual subsets and
both singletons** — a `sup.is_singleton()` skip would drop both edges.

The float **oneof** bucket does not have this problem: `OrdF64::new` normalizes `-0.0 → +0.0`
(`data_axioms.rs:1216-1226`, with an explicit "FP-critical" comment). The two float encodings in the
same file are inconsistent about signed zero.

**How verified.** `scratchpad/negzero.ofn` (`Pos ≡ DataHasValue(p,"0.0"^^xsd:float)`,
`Neg ≡ DataHasValue(p,"-0.0"^^xsd:float)`): rustdl reports `equiv Neg Pos` and answers `yes` to
`subclass` in **both** directions. Konclude on the same ontology: `EquivalentClasses(Pos, Neg)`.
So rustdl's *answer* is correct (this is **not** an FP) — only the invariant is wrong.

**Falsifiable prediction.** Applying F2's skip as recorded makes `negzero.ofn` report neither
`Pos ⊑ Neg` nor `Neg ⊑ Pos` (a new MISS vs the Konclude oracle). Normalizing `-0.0 → +0.0` inside
`tagged_float_dkey_iri`'s `bound()` restores the invariant (the two keys intern to one ClassId), keeps
the equivalence, and makes the skip safe in all seven buckets — verify with `negzero.ofn` still
reporting `equiv` and the `datatype_value_membership` suite (66 tests) still green.

---

## SUSPECTED

### S1 — incorrect — `convert.rs:2864-2871` (the `anchor` early return) — the "unanchored ×
### non-merging-anchored" hole

**Mechanism, characterised precisely.** Requirement **R6** of
`docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` reads: *"keys dropped from
same-component value×value grouping must still participate in `global × anchored` pairing"*.
It is **not implemented**. The collapse split was correctly built as a per-pair drop at emission
(`convert.rs:3035-3049`), which is R6-safe — but its predecessor, the merging gate, still drops the
key at **anchor** time:

```
convert.rs:2863  let comp = uf.find(role.role_id().index() as usize);
convert.rs:2864  if merging_comps.as_ref().is_some_and(|m| !m.contains(&comp)) { return; }
```
so the key never enters `components`; in `seed_disjoint_bucket` it is then neither in
`comp.unanchored` nor in `comp.components` (`convert.rs:2990-3004`), falls into the *"neither anchored
nor unanchored … skip it entirely"* branch, and is therefore absent from `anchored` — so the
**unconditional** `global × anchored` loop at `convert.rs:3065-3067` never pairs it with an unanchored
key. R6's own wording ("this design touches exactly that code") shows the authors intended to fix it
here; they did not.

**Why SUSPECTED rather than confirmed.** I could not construct a reachable trigger, and I believe it
is doubly guarded: (a) `unanchored` requires a **top-level bare `DKey`** in a class position, which no
lowering produces — `collect_direct_dkeys` (`convert.rs:2622-2640`) stops at `Some`/`All`, and the
`seed_bucket` `SubClassOf(DKey,DKey)` edges that *would* look top-level are deliberately pushed
**after** `dkey_components` runs (`convert.rs:2469-2474`); (b) even given an unanchored key, producing
a *clash* additionally requires both keys co-labelled, which needs the very merge the gate is testing
for. So this is an invariant break with no demonstrated consequence — real, but latent.

**Falsifiable prediction.** Changing `anchor` to record the component but mark it non-merging (instead
of returning early), and letting the existing per-pair `droppable` test at `convert.rs:3035` suppress
the same-component pairs, is **behaviourally identical** on the whole ORE pool: `ore_ont_9347`
concept_rules **113**, `ore_ont_5368` **18,620,251**, and `RUSTDL_DKEY_SPLIT_STATS` totals unchanged on
all 356 pair-seeding ontologies. If any of those move, the hole was not latent.

---

### S2 — inefficient (cross-subsystem) — the per-distinct-literal DKey minting in `convert.rs`

**Mechanism.** `DataPropertyAssertion(p,a,v)` lowers to `ClassAssertion(a, ∃p.DKey(v))`, minting one
synthetic **class** per distinct literal. The resulting class count, not any single loop, is what
produces the "tiny input / multi-GB RSS" cluster: the EL saturator's per-class structures are then
sized by it.

**How verified — a bound, not a fix.** Rewriting `ore_ont_16632`'s 17,415 `DataPropertyAssertion`
literals to 4 constant values (one per datatype; **every axiom, individual, class and property
preserved**) changes classify from **18.59 s / 2.50 GB** to **3.48 s / 60 MB** — 5.3× wall, 42× RSS.
The sampling profile of the unmodified file puts 50.3% of the wall inside the saturator (over the
inflated class space) and 38.7% inside `convert_ontology`. Distinct-key counts across the cluster are
in F2's table (4.5k–20.7k keys against 11–few-hundred real classes).

**Why SUSPECTED, and the obvious fix explicitly REFUTED.** The natural lever — "mint a per-value key
only when some axiom can consume value-level information on that property" (reusing the `m_star`
predicate `dkey_components` already computes) — **would not help `ore_ont_16632` at all**. Census of
that file: 74 data properties bear assertions and **all 74** are consuming (35 `DataPropertyRange`,
74 `DataMaxCardinality(1, p)`, sub-property-closed) ⇒ 12,087 / 12,087 distinct values sit on
consuming properties, 0.0% droppable. Its `DataMaxCardinality(1, p)` really does make two distinct
values a clash — which is why `abox_check` correctly reports it **inconsistent**. So the cost is
*intrinsic to the encoding*, not a gate hole. A refined lever would have to bound how many value keys
a `≤n` needs (n+1), which is a concrete-domain counting change; the repo's own
`docs/superpowers/specs/2026-07-31-data-cardinality-counting-design.md` was **RETRACTED** (commit
`7ac8863`) — read that before re-proposing anything in this direction.

---

## REFUTED starting points

* **"the parser mutual-exclusivity canary may not cover all pairs."** It does.
  `numeric_oneof_parser_matrix_exclusivity` (`convert.rs:4113-4178`) builds one IRI for **all twelve**
  buckets via the real encoders and probes **all twelve** decoders — a full 12×12 matrix, with the
  diagonal asserted `Some` and every off-diagonal asserted `None`. (The older
  `parser_matrix_mutual_exclusivity` at `4077` is the 7×7 subset.) Caveat: one sample IRI per bucket,
  so it is a spot check rather than a proof; exclusivity nevertheless holds by construction — every
  tag (`f: db: dec: date: dt: str: io: fo: deo: dao: dto:`) is alphabetic, they are pairwise
  non-prefixing, and the untagged integer decoder requires its first `:`-token to parse as `i64` or be
  `*`. **No cross-datatype seeding risk found. No FP found anywhere in this subsystem.**

* **"`seed_bucket`'s missing component argument is the defect."** Partly wrong framing. The told
  `DKey ⊑ DKey` edge is a genuine class subsumption usable wherever a DKey appears, so
  component-bounding it (as `seed_disjoint_bucket` does) would be **unsound for completeness**. The
  defect is the unindexed quadratic *scan*, not the absence of a component bound. See F2.

* **"`17,415 assertions → 303M calls ≈ 12 s`."** Reproduced the trap and avoided it: `ore_ont_16632`
  has 17,415 assertions but only **6,933 distinct** `xsd:string` values, so the true string-bucket
  scan is 48M, not 303M, and `seed_bucket` profiles at **4.2%** there — matching the recorded "real
  cost was 6%". The 96% figure in F2 is a *different* ontology (`ore_ont_9347`, 17,918 **distinct**
  keys) and is backed by a sampling profile, not by the arithmetic alone.

---

## Cosmetic (not one of the three categories, noted in passing)

`convert.rs:2530-2537` — the doc comment describing the non-merging-component gate
(`RUSTDL_DKEY_MERGING_GATE`) is attached to `dkey_split_stats_enabled` (`convert.rs:2541`), not to
`dkey_merging_gate_enabled` (`convert.rs:2558`), which has none. Pure documentation drift.

---

## Artifacts

All under `scratchpad/`: `oneof.ofn`/`oneof.owx`/`oneof-kon.owx`, `oneof_clash.ofn`/`oc.owx`/`oc-kon.owx`,
`only_direct.ofn`, `only_neg.ofn`/`on.owx`/`on-kon.owx`, `nnf_forall.ofn`, `negzero.ofn`/`nz.owx`/`nz-kon.owx`,
`synth_str_{1000..16000}.ofn`, `synth_int_*.ofn`, `synth_xint_*.ofn`, `16632_collapsed.owl`,
`st3.txt` (16632 stacks, 191 samples), `st9347.txt` (9347 stacks, 100 samples).

# `MAX_BODY_VARS = 8` — a real silent MISS, and raising it is a hard stop

**Date:** 2026-08-03 · **Base:** v0.4.13 (`e2ba481`) · **Subject:** row 2 of
`docs/2026-08-03-constant-audit.md`, the one candidate that audit could not close.

**Verdict in one line.** The silent MISS is **REAL** — demonstrated on a synthetic
whose 12-variable clause body is refused by the cap (proved by instrumentation, not
inspection) and whose withheld entailment **both Konclude v0.7.0 and HermiT 1.4.3
derive**. Raising the cap to 16 **recovers nothing on any real ontology** (0 added
pairs over 23 binders plus 6 curated fixtures) and takes **three currently-completing
ORE ontologies from OK to DNF at a 300 s cap**, losing 9 773 sound pairs. So the lever
ships **default OFF as a documented negative result**, and the cap stays.

---

## 0. What was being asked, and what was already known

`docs/2026-08-03-constant-audit.md` §6 established that `MAX_BODY_VARS` (`hyper.rs:46`)
**binds on 23 of 368** probed ontologies, that `ore_ont_10140`'s real requirement is
**12 variables**, and that raising the cap changes the fixpoint materially
(`fp_max_steps` 7 899 → 905) with no wall effect. It could not answer the question that
matters, because **every ontology it saw the cap bind on was a DNF**, so no closure was
comparable to an oracle. §1 of that audit also put the constant in a class of its own:
it is not an early-termination budget but a **structural completeness cap** whose *raise*
direction is the FP direction.

Two things therefore had to be settled here, in order: whether a real entailment is
withheld at all, and whether raising the cap is safe and useful.

## 1. The mechanism, re-read in code before measuring anything

`eval_order` (`hyper.rs:4669`) BFS-orders a clause body's role atoms and refuses the body
in **three distinct situations**, only one of which is the cap:

| refusal | reachable by raising the cap? |
|---|---|
| `bound.len() > MAX_BODY_VARS` | **yes — this is the cap** |
| `bound.contains(v)` (two atoms target one var, or a cycle) | no |
| no progress (a source unreachable from `X`) | no |

`build_clause_match_plan` (`:951`) turns any refusal into `None`, and — separately — also
returns `None` for an equality/inverse body atom. `match_body` (`:4046`) propagates that
`None`, and **all three consumers** (`:3243`, `:3299`, `:3992`) respond by *skipping the
clause*: `continue` in the two disjunction-selection sites, `FireOutcome::NoChange` in
`fire_clause`. Nothing anywhere reports it. So the cap does not bound work — it **deletes
inferences**, and the defect is a **silent MISS**: a completeness loss with no
`incomplete` signal.

This is why the four `None` branches had to be separated before any conclusion could be
drawn. A refusal that is `NotTree` or `Disconnected` is untouched by the cap, and a
measurement that conflated them would have attributed the wrong cause. The code now
carries an `OrderReject` enum so the refusal reason is nameable, and
`RUSTDL_TRACE_BODY_VARS=1` prints it.

## 2. The fixture, and the proof it actually trips the cap

Building the fixture was the hard part the brief predicted. The shape comes from reading
the clausifier rather than guessing: `Clausifier::encode_antecedent`'s
`ConceptExpr::Some` arm (`owl-dl-core/src/clause.rs:539`) allocates **one fresh variable
per `∃` occurrence** in an antecedent, and the `And` arm concatenates the resulting
bodies. So

```
SubClassOf(ObjectIntersectionOf(∃r1.A1 … ∃r11.A11) ObjectUnionOf(B C))
```

clausifies to **one** non-Horn clause with 11 role atoms on 11 distinct successor
variables plus `X` — **12 variables**, exactly `ore_ont_10140`'s measured requirement.
Adding `SubClassOf(X, ∃ri.Ai)` for each `i` makes the body match at `X`, and
`DisjointClasses(X, B)` refutes the `B` disjunct, so **`X ⊑ C` is entailed and only that
clause can prove it** — the EL saturator cannot do the case split, and there is no common
told subsumer to shortcut through (the first variant tried, `B ⊑ D` + `C ⊑ D`, was
discarded precisely because rustdl found `X ⊑ D` at n = 11 by another route and so proved
nothing).

Fixtures: `crates/owl-dl-cli/tests/fixtures/max_body_vars/wide-body-12vars.ofn` (plus a
`narrow-body-8vars.ofn` control and a `wide-body-12vars-no-entailment.ofn` FP control).

**Observed var count, from instrumentation, at the shipped default:**

```
[mbv] refused body: vars=12 role_atoms=11 cap=8 reason=VarCap { vars: 9, cap: 8 }
```

`vars=12` is the body's full requirement; `VarCap { vars: 9 }` is the count at the instant
of refusal (`bound` starts holding `X`, so the ninth variable is the first over an
eight-var cap). `reason=VarCap` is the load-bearing token: the refusal is the cap, not one
of the two shape branches.

**The threshold is exactly where the cap's arithmetic says it should be.** Sweeping the
same construction at n conjuncts (so n + 1 variables), asking only whether `X ⊑ C` is
reported:

| n (successor vars) | 5 | 6 | 7 | **8** | 9 | 10 | 11 | 12 |
|---|---|---|---|---|---|---|---|---|
| `X ⊑ C` found | ✓ | ✓ | ✓ | **✗** | ✗ | ✗ | ✗ | ✗ |

The break is at n = 8, i.e. `bound.len() = 9 > 8`. Nothing else in the pipeline changes
at that point.

## 3. Is the missed entailment real? YES — both oracles derive it

`X ⊑ C` on `wide-body-12vars.ofn`:

| reasoner | reports `X ⊑ C` |
|---|---|
| **Konclude v0.7.0** (native) | **yes** |
| **HermiT 1.4.3** (ROBOT 1.9.6) | **yes** |
| rustdl v0.4.13, shipped default | **NO — silent MISS** |
| rustdl, `RUSTDL_WIDE_BODY_VARS=1` | yes |

Both oracle outputs are committed beside the fixture
(`wide-body-12vars-konclude.owx`, `wide-body-12vars-hermit.txt`) so the adjudication is
reproducible without Konclude or Docker present, and a canary asserts both contain the
pair. With the flag on, the same run prints
`[mbv] accepted body: vars=12 role_atoms=11 cap=16` — so the recovery is attributable to
the cap and to nothing else.

**This is the first direct confirmation that `MAX_BODY_VARS` costs a real entailment.**
The audit could only show the cap was reached.

## 4. Is raising it FP-safe? Verified, not assumed — and yes

The argument on offer was "firing a clause the clausifier derived adds only entailed
consequences". That is the right argument but it was checked rather than accepted.

**(a) The scratch buffers spill; they do not truncate.** The subagent audit of downstream
`≤ 8` assumptions returned exactly one "BREAKS" claim — `bound: SmallVec<[Var; 8]>` at
`eval_order` — and **that claim is wrong**: `SmallVec` grows onto the heap, it is not a
fixed array. Rather than argue it, `eval_order_with_cap` was split out so a unit test can
order a **41-variable chain presented in reverse** at cap 64 and assert the resulting
order is exactly `n-1 … 0`. A truncating or corrupting buffer shows up there as a wrong
order, not as a slow one. It passes, and the deliberate sabotage of making `bound.push`
silently drop past index 8 fails three tests. So the spill is **perf-only**, as the audit
predicted. `Binding = SmallVec<[(Var, HNode); 4]>` likewise spills (11 successors ⇒ heap);
its only length-sensitive consumer is `resolve_var`'s linear scan, which is O(n) but
correct at any n.

**(b) Nothing downstream keys on a small fixed variable set.** `Var` is a bare `u32` with
`X = 0` — no bitmask, no `1 << var`, no per-variable array. `fresh_var`
(`clause.rs:225`) uses `checked_add` and has no cap. `ANTECEDENT_DNF_CAP = 64` bounds DNF
cross-product breadth, not variable count. The clause indexes (`build_clause_indexes`,
`build_clause_index_delta`, `index_one_clause`, `x_trigger`,
`inverse_first_trigger`) are keyed by `ClassId`/`Role`, never by variable count. No
serialization of bindings or match plans exists.

**(c) The FP direction was tested with a fixture, not reasoned about.**
`wide-body-12vars-no-entailment.ofn` is the same 12-variable clause with the
`DisjointClasses` premise removed, so `X ⊑ B ⊔ C` holds but neither disjunct is entailed.
Under the flag the body is **asserted to fire** (`accepted body: vars=12`) and rustdl
reports **no subsumption for `X`** — matching a committed Konclude oracle, which reports
only `Thing` rows. Firing the wide clause adds no spurious pair.

**(d) The flag-OFF path is byte-identical to the pre-change binary.** Pinned
`rustdl-base` (`f42a9c9b…`) vs the changed `rustdl-mbv-flag` (`d36f8357…`), `classify
--pair-timeout-ms 1000`, verdict rows sorted: **8/8 identical** (bibtex 15, pizza 314,
ro 49, ro-stripped 49, sulo 15, sulo-stripped 15, sio 1617, go-basic 57 803).

**Conclusion: raising the cap is FP-safe.** Every measurement below found **zero added
pairs**, so there was in the end no added pair anywhere needing adjudication — but the
FP-control fixture is what makes that an argument rather than an absence.

## 5. Does raising it recover anything real? NO — and it breaks three ontologies

### 5a. Binding census, re-derived (the audit's instrumentation was reverted)

`RUSTDL_TRACE_BODY_VARS=1 rustdl consistent`, 20 s wall cap, 24 GB address-space cap, over
**all 868** OFN ontologies in `/data/dumontier/ore-run/work/sym`. Counting only
`reason=VarCap` refusals. This is a *binding* census — a boolean/count predicate, not a
wall measurement — so it was run at `-P 8`; every wall number elsewhere in this document
is strictly serial and single-threaded.

**23 of 868 bind the cap.** The three the audit named by number all appear, with matching
requirements — `ore_ont_10140` at **12** (the audit's figure exactly), `ore_ont_11629` at
9, `ore_ont_3575` at 16 — which is the cheapest available confirmation that this census
and the audit's are measuring the same thing.

| max body vars needed | ontologies |
|---|---|
| 9 (one over the cap) | `11629` `11745` `15672` `16535` `2129` `3529` `786` `9855` `9890` |
| 11 | `7775` |
| 12 | `10140` `1016` `11460` `11623` `16669` `4604` `6952` |
| 16 | `16461` `3575` |
| 25 | `15491` `7712` |
| **133** | `12993` `6682` |

**A finding the audit could not see: 16 is not enough for 4 of the 23.** `15491`/`7712`
need 25 and `12993`/`6682` need **133**. So "raise it to 16" would leave the silent MISS
open on 17% of its own addressable set — the constant is not merely mis-sized, it has no
right fixed value, the same disease `MAX_SEARCH_DEPTH` was diagnosed with in the audit's
§4.

### 5b. `ore_ont_15672` — the completing binder the audit needed

`ore_ont_15672` is a **curated FP=0-net fixture** (`ore-15672-shoin.ofn`, 83 classes,
SHOIN) that classifies in 0.06 s, and it **binds the cap** at `vars=9`. This is the
"completing binder, not a faster one" the audit closed §6 asking for.

| arm | trace | wall | rows |
|---|---|---|---|
| OFF | `refused body: vars=9 … reason=VarCap` | 0.064 s | 75 |
| ON | `accepted body: vars=9 cap=16` | 0.056 s | **75** |

`lost=0 added=0`. **The withheld clause is inert here.** So the very first comparable
closure says the cap's MISS, while real, is not costing this ontology anything.

### 5c. All 23 binders, OFF vs ON, serial and single-threaded

`classify` (no per-pair budget — the mode in which the cap can bind at all),
`RAYON_NUM_THREADS=1`, 60 s wall cap, `ulimit -v` 24 GB, arms interleaved per ontology.
`lost` / `added` are `comm` over sorted verdict rows.

| ontology | vars | OFF wall | OFF rows | ON wall | ON rows | lost | added |
|---|---|---|---|---|---|---|---|
| `ore_ont_1016` | 12 | 0.28 | 4111 | 0.28 | 4111 | 0 | 0 |
| `ore_ont_11623` | 12 | 0.24 | 3309 | 0.23 | 3309 | 0 | 0 |
| `ore_ont_16669` | 12 | 0.28 | 4111 | 0.28 | 4111 | 0 | 0 |
| `ore_ont_15672` | 9 | 0.06 | 75 | 0.07 | 75 | 0 | 0 |
| `ore_ont_16535` | 9 | 0.14 | 90 | 0.15 | 90 | 0 | 0 |
| `ore_ont_2129` | 9 | 0.15 | 3159 | 0.14 | 3159 | 0 | 0 |
| `ore_ont_3529` | 9 | 0.18 | 3157 | 0.18 | 3157 | 0 | 0 |
| `ore_ont_786` | 9 | 1.50 | 12459 | 1.48 | 12459 | 0 | 0 |
| **`ore_ont_16461`** | **16** | **0.02** | **84** | **dnf @60** | **0** | **84** | 0 |
| **`ore_ont_7775`** | **11** | **3.18** | **1510** | **dnf @60** | **0** | **1510** | 0 |
| **`ore_ont_15491`** | **25** | **25.63** | **8179** | **dnf @60** | **0** | **8179** | 0 |
| `ore_ont_10140` | 12 | dnf @60 | 0 | dnf @60 | 0 | 0 | 0 |
| `ore_ont_11460` | 12 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_4604` | 12 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_6952` | 12 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_11629` | 9 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_11745` | 9 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_9855` | 9 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_9890` | 9 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_3575` | 16 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_7712` | 25 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_12993` | 133 | dnf | 0 | dnf | 0 | 0 | 0 |
| `ore_ont_6682` | 133 | dnf | 0 | dnf | 0 | 0 | 0 |

**Tally: 8 completers unchanged (verdict-identical, walls flat), 12 DNF on both sides,
3 completers DESTROYED. Zero added pairs, anywhere.**

### 5d. The three regressions are not a 60 s artifact

Re-run strictly serially on a quiet host at a **300 s** cap:

| ontology | OFF | ON |
|---|---|---|
| `ore_ont_16461` | 0.02 s, 84 rows | **dnf @300 s**, 0 rows |
| `ore_ont_7775` | 3.14 s, 1510 rows | **dnf @300 s**, 0 rows |
| `ore_ont_15491` | 27.98 s, 8179 rows | **dnf @300 s**, 0 rows |

`ore_ont_16461` is the clearest instance: **0.02 s → not finished in 300 s**, a ≥15 000×
blow-up, on a 246-line ontology. Its `clause-stats` says it has **exactly one disjunctive
clause**, and the trace says that clause is the 15-role-atom / 16-variable body the cap
was withholding. Admitting it opens a single non-Horn branching point of width 15 into an
otherwise Horn problem, and the search does not come back. The mechanism is the same for
`7775` (10 role atoms, 11 vars).

**This is exactly the precedent the brief cited** — a flag validated on a small benchmark
that took four other ontologies from ~5 s to DNF — reproduced here on the first
population contact.

### 5e. Curated corpus, ON vs OFF

`classify --pair-timeout-ms 1000`, verdict rows sorted, `lost`/`added` by `comm`:

| fixture | OFF rows | ON rows | lost | added |
|---|---|---|---|---|
| bibtex | 15 | 15 | 0 | 0 |
| pizza | 314 | 314 | 0 | 0 |
| ro | 49 | 49 | 0 | 0 |
| sulo | 15 | 15 | 0 | 0 |
| sio | 1617 | 1617 | 0 | 0 |
| go-basic | 57 803 | 57 803 | 0 | 0 |
| ore-15672 | 75 | 75 | 0 | 0 |

Inert throughout — which, per the standing caveat in `CLAUDE.md`, demonstrates
**non-regression only**. The evidence that carries this document is the synthetic plus
the oracle plus §5c/§5d.

## 6. What ships

`RUSTDL_WIDE_BODY_VARS=1` raises the cap from `MAX_BODY_VARS = 8` to
`WIDE_BODY_VARS = 16`. **DEFAULT OFF**, house idiom
(`std::env::var(...).ok().is_some_and(|v| v == "1")`), read once through a `OnceLock`.
`RUSTDL_TRACE_BODY_VARS=1` prints one line per refused body (with the refusal *reason*)
and one per body accepted above `MAX_BODY_VARS`.

**Do not flip the default.** The measurement above is the argument against it, not merely
an absence of one: on the 23 ontologies where the constant binds, the flag buys **zero**
recovered pairs and costs **three completing ontologies and 9 773 sound pairs**.

The cap is nonetheless a genuine, now-documented completeness defect, and the honest
summary is that **it is mis-designed rather than mis-tuned**. Anything that closes it has
to satisfy two constraints this lever cannot:

1. **No single fixed value works.** Its own binders need 9, 11, 12, 16, 25 and 133.
2. **Admitting a wide body must not admit an unbounded non-Horn branching point.** The
   three regressions are all cases where the withheld clause was **disjunctive**. A
   plausible next lever, unbuilt and unmeasured, is to raise the cap **for Horn bodies
   only** — `fire_clause`'s consumer derives facts deterministically and cannot open a
   branch, whereas the two `find_open_disjunction` consumers can. That would have
   recovered the synthetic only if its clause were Horn (it is not), so it is not a fix
   for this document's fixture; it is the shape a *safe* raise would have to take.

## 7. Gates

| gate | result |
|---|---|
| `cargo fmt --all -- --check` | **clean** |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **clean** |
| `cargo test --workspace --exclude owl-dl-py --no-fail-fast` | **60 result groups, 1073 passed, 0 failed** |
| `./scripts/run-soundness-diff.sh`, flag **OFF** | **11 VERIFIED, all closures exact**, 22 passed / 0 failed |
| `./scripts/run-soundness-diff.sh`, flag **ON** | **11 VERIFIED, all closures exact** — manifest identical to flag-OFF |
| flag-OFF byte-identity vs pinned pre-change binary | **8/8 identical** |
| flag-ON superset over flag-OFF | **0 lost, 0 added** on the curated set; on ORE, **3 ontologies lose everything** — the hard stop that keeps the default OFF |

Both FP=0 manifests read: galen 27 997, notgalen 32 739, sio 8904, ore-10908 6001,
wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16 — the
reference values in `CLAUDE.md`. The three long-standing `NOT VERIFIED` entries
(`ro-stripped`, `sulo-stripped`, `sio-stripped`) are absent fixtures, unrelated to this
change and identical on both arms.

Per the standing caveat, an all-green FP=0 net here demonstrates **non-regression**, not
soundness for this area — the curated corpus contains no over-8-variable body that
matters (only `ore-15672`'s, which §5b shows is inert). The soundness evidence is the
FP-control fixture and the Konclude ∪ HermiT adjudication.

### 7b. Sabotage — 9 run, 9 caught, 0 unqualified survivors

Strictly serial, each reverted from a pinned pristine copy before the next.

| # | sabotage | caught by |
|---|---|---|
| 1 | `max_body_vars()` returns `MAX_BODY_VARS` in both arms (lever is a no-op) | `wide_body_vars_recovers_the_entailment` (1 of 5 failed) |
| 2 | `max_body_vars()` returns `WIDE_BODY_VARS` in both arms (default flipped) | `wide_body_trips_the_variable_cap_specifically` + `shipped_default_silently_misses_…` (2 of 5) |
| 3 | cap refusal reported as `OrderReject::NotTree` (attribution lies) | `wide_body_trips_the_variable_cap_specifically` (1 of 5) |
| 4 | fixture narrowed to 7 conjuncts so it no longer reaches the cap | 3 of 5 failed — **the hazard the brief names by name** |
| 5 | committed HermiT oracle edited to say `X ⊑ D` | `committed_oracles_both_derive_the_recovered_pair` (1 of 5) |
| 6 | `bound.push` silently dropped past index 8 (truncate instead of spill) | 3 of 5 unit tests |
| 7 | `WIDE_BODY_VARS = 10` (too small to admit 12) | `wide_cap_admits_…` + 2 of 6 CLI canaries |
| 8 | `distinct_body_vars` drops the initial `X` | `distinct_body_vars_counts_the_full_requirement` — **but all 6 CLI canaries passed** |
| 9 | FP-control fixture given back its `DisjointClasses` premise | `wide_body_fires_without_inventing_a_subsumption` (1 of 6) |

**Sabotage 8 is reported as a qualified catch.** The unit test caught it; the CLI
canary's `vars=12` assertion did **not**, because `X` appears as the *source* of every
role atom in a star body and so is re-added by the loop — the initial `smallvec![X]`
only matters for a body with no role atoms at all. So the guard on that property is the
unit test, and a future change should not read the CLI assertion as protecting it.

## 8. Provenance

* Host 32-core / 251 GB, `up 42 days`, load ~1.8–2.6 during the serial arms. Every probe
  under `ulimit -v $((24*1024*1024))`; wall measurements `RAYON_NUM_THREADS=1` and
  serial; the binding census at `-P 8` (a boolean predicate).
* Binaries pinned immediately after the build that produced them:
  `rustdl-base` = pre-change v0.4.13, `sha256 f42a9c9b2f09d8141f0944d3a74fbe4d03d839125361472b82fda38d2a7d0285`;
  `rustdl-mbv-flag` = with the lever, `sha256 d36f8357ea347be3a15624386ab7ef53f63d499e6047d30de27c4f88ad06410d`.
  Built with `RUSTUP_TOOLCHAIN=stable` (a bare `cargo` is not on `PATH` at all on this
  host — `~/.cargo/bin` contains only `samply`; `cargo` lives in
  `~/.rustup/toolchains/stable-*/bin`).
* Corpus: `/data/dumontier/ore-run/work/sym/*.ofn` (868). The 1 920-file ORE pool is
  RDF/XML-or-OWL/XML `.owl` and was not converted here, so **the census denominator is
  868, not 1 920** — a smaller population than the ORE-wide sweep the audit's §0d asks
  any spec to run. It is nonetheless the *whole* of that 868, not a stride, and it
  reproduces all three of the audit's named binders at their audited requirements.
* Oracles: `/data/dumontier/reasoners/run-konclude.sh` (Konclude v0.7.0-1138 native),
  `run-hermit.sh` (HermiT 1.4.3 via `obolibrary/robot:v1.9.6`).

### Threats to validity, stated rather than hidden

* **The recovery claim rests on one synthetic.** No *real* ontology was found where
  raising the cap adds a pair — which is the finding, but it also means the entailment
  loss has been demonstrated only on a fixture built to demonstrate it. The
  three ontologies needing 25 or 133 variables are all DNF at both settings, so whether
  *they* lose entailments to the cap is still unknown, and 16 would not answer it.
* **Walls in §5c are single-pass, not min-of-k.** All three regressions are
  confirmed at 300 s (0.02→≥300 s, 3.1→≥300 s, 28.0→≥300 s) — magnitudes no plausible
  scheduling noise reaches. The eight unchanged rows are quoted only as "flat", not as
  measurements.
* **No ORE-wide two-arm sweep.** The audit's §0d names that as the first thing any spec
  must do, and it was not done: the population here is the 868 OFN ontologies, and the
  two-arm classify comparison covers only the 23 binders plus the curated set. Since the
  recommendation is *not* to flip the default, the sweep would only strengthen an
  already-negative result; it would be required before any future attempt to ship a raise.

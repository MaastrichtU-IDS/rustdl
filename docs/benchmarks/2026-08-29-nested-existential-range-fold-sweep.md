# Two-arm ORE sweep for the nested-existential RANGE fold (#81)

**Arms.** `BEFORE` = `main` @ `9e6d57d` (sha `db533d3746ea`, v0.4.23 — i.e. #80/#82 already in),
`AFTER` = the #81 fix (sha `57ef02ff0b39`). Each pinned immediately after its build and verified
against **two discriminating inputs** before use: `cascade.ofn` (BEFORE `[]`, AFTER
`[(A,FINAL)]`) and an inner-range fixture (BEFORE `[]`, AFTER `[(X,FINAL)]`). An arm that cannot
be told apart from the other cannot validate anything.

**Population.** All **1,920** ORE ontologies, no scoping.

**Method.** `owl-reasoner-harness` `sweep-arm.sh` per arm, 60 s cap, `--threads 1`, 4 chunks, one
invocation per ontology, resumable JSONL, arm sha in each chunk manifest. Legs ran
**sequentially**. Unlike the #80 sweep, this one sets `SWEEP_EXTRA=--digest-strip-comments`, so
**answer identity is actually measured** rather than left null — #80's report had to state that
its leg compared completion only.

**Instrument calibrated first.** `compare` was run against a sweep whose answer is recorded in
`CLAUDE.md` (the v0.4.7/v0.4.8 pair) and reproduced it exactly — 91 recoveries, 4 regressions,
and the same four named ontologies (`ore_ont_10838`, `15846`, `16315`, `3087`).

**Prediction stated before reading the result.** This fix ADDS entailments, so a "0 DIFFER, all
identical" outcome would have indicated an instrument failure, not a pass.

## Result

| | BEFORE | AFTER |
|---|---|---|
| ok | 1779 | 1778 |
| dnf | 138 | 139 |
| err_reject | 3 | 3 |

**Answer identity over the 1,778 both-completed: 1,777 IDENTICAL, 1 DIFFER.**
**REGRESSED: 1** (`ore_ont_7204`). **RECOVERED: 0.** Both flagged rows adjudicate clean, and in
both the change is provably INERT. Wall is balanced: 47 faster / 41 slower beyond ±10%.

## `ore_ont_12698` (DIFFER) — budget-truncation noise, settled at a non-truncating budget

Both arms report `incomplete: true` at the sweep's default per-pair budget, and `CLAUDE.md`
already records this ontology as run-to-run nondeterministic under truncation.

- Hasse diff read 11 gained / 2 lost — but `direct_subsumptions` is the Hasse relation, so a
  gained NEARER parent demotes a farther one. Comparing **transitive closures** instead gave
  10 gained / 2 lost.
- **Three runs of the SAME (AFTER) binary: 21310, 21304, 21309 rows** — a 6-row spread against a
  9-row arm difference. Closure sizes overlap between arms (BEFORE 126727/126729/126729, AFTER
  126722/126731/126713).
- Of the two "lost" pairs, `EFO_0002708 ⊑ EFO_0002936` is **absent from all six** repeat runs
  (it occurred only in the single sweep run), and `EFO_0002349 ⊑ EFO_0002888` flickers.
- **Settled as `CLAUDE.md` prescribes — at `--pair-timeout-ms 1000`, where both arms report
  `incomplete: false`, the two are IDENTICAL: 21,313 rows, 0 gained, 0 lost.**

## `ore_ont_7204` (ok → dnf) — a 60 s cap flip, on an ontology the change cannot touch

Sweep walls were BEFORE **52.42 s** (ok) and AFTER **60.05 s** (cap hit). The 52 s figure is
already within noise of a 60 s cap under a loaded host.

**The change is provably inert here**, which is what makes the cap attribution safe rather than
convenient. Every construct that could reach a modified code path is ABSENT:

| construct | count | path it would enable |
|---|---|---|
| `ObjectPropertyRange` | **0** | `effective_ranges` empty ⟹ `range_extras` returns `&[]` at every site |
| `ObjectMinCardinality` | **0** | the `Some`→`Some \| Min` widening in the `And`-LHS arm |
| `ObjectOneOf` / `ObjectHasValue` | **0** | the nominal-body arm of `atomic_existential_rhs` |
| `owl:Thing` | **0** | the `∃R.⊤` top-witness arm |

Ten interleaved runs on an idle host, 300 s cap:

| arm | walls (s) | median | max |
|---|---|---|---|
| BEFORE | 47.03, 34.16, 34.35, 35.59, 33.93 | 34.35 | 47.03 |
| AFTER | 34.25, 36.34, 37.77, 37.14, 35.06 | 36.34 | 37.77 |

**All 10 complete (`rc=0`)**, and BEFORE's own spread (33.9–47.0 s) is wider than the gap between
the medians, so there is no reliable slowdown to attribute. More decisively, **the banner-stripped
output is byte-IDENTICAL between arms on all five paired runs (29,231 rows each)** — the
structural inertness argument above, confirmed empirically.

## What this sweep does and does not establish

**Does:** no completion regression and no answer change anywhere in the corpus. Unlike #80's leg,
answer identity is measured, not assumed: 1,777 of 1,778 both-completed ontologies are strictly
identical on banner-stripped stdout, and the one exception resolves to identical at a
non-truncating budget.

**Does NOT:** establish that the entailments the fix ADDS on ORE ontologies are correct. A
two-arm diff cannot answer that, and this fix is *supposed* to change answers on
range-plus-nested-existential shapes. That evidence comes from the Konclude adjudication on the
three new entailments (`cascade` `A ⊑ FINAL`, an inner-range `X ⊑ FINAL`, and a range-plus-
disjointness `C` unsatisfiable) and from the FP=0 net (11 VERIFIED, every closure exact) — which
is itself INERT for this shape and therefore shows non-regression only.

**A note on how nearly-nothing moved.** Only ONE of 1,920 ORE ontologies produced any answer
difference at all, and that one was noise. The shape the fix needs — an `ObjectPropertyRange` on
a role whose existential is nested under a conjunctive LHS, or nested one level deeper — is
essentially absent from ORE. That is precisely why the corpus could not have validated this fix,
and why the peer adjudication is the evidence that counts.

## Second arm: does the fix raise `verify-el`'s false-`Violated` rate?

Worth asking, because the fix changes fact targets from bare markers to range-wrapped synthetics
— i.e. it changes label CONTENT on exactly the path
`docs/known-limitations/verify-two-expansion-paths-split-a-witness.md` says can split one logical
witness into two elements. It also demonstrably moved `cascade.ofn` from a true detection to an
F1 false positive, so a corpus-level rise was a live possibility rather than a hypothetical.

`scripts/verify-el-inertness.sh` run on both arms over its 20 pure-EL ORE ontologies:

| arm | Verified (exit 0) | timeout (exit 124) | Violated | Unresolved |
|---|---|---|---|---|
| BEFORE | 16 | 4 | 0 | 0 |
| AFTER | **16** | **4** | **0** | **0** |

**Per-ontology identical, 20 of 20 — no ontology changed verdict.** So the false-`Violated` rate
does not rise on this population, and the crate's recorded "16 of 20" inertness figure is
**re-verified against the post-fix engine** rather than left stale. The 4 remaining are `timeout`
exit 124 at a 300 s cap — **UNMEASURED, not passing**, the same distinction the crate's own
coverage note draws.

## Follow-up: `ore_ont_9429`, #80's un-root-caused +27%

#80's sweep left one regression recorded as **cause unknown** (`55.5 s → 70.7 s`), explicitly
refusing a plausible-sounding attribution. Two things are now settled about it.

**#81 is inert there.** Both arms are DNF at the 60 s cap with identical peak RSS, and a
single-thread run of each pin produces **identical output (2,706 rows), identical subsumption
counts (`saturation=28267 tableau=0`), and identical phases**:

| phase | BEFORE (ms) | AFTER (ms) |
|---|---|---|
| `label_cache_build` | 36,240 | 36,041 |
| `tier_walk` | 18,837 | 18,455 |
| `sweeps` | 14,852 | 14,692 |
| everything else | <200 | <200 |

**Its cost is 51% `label_cache_build`.** That is the phase `CLAUDE.md` partitions as
DEADLINE-BOUND — `wall = #units × deadline`, so a faster engine performs more work per unit and
the wall does not move. `ore_ont_9429` is therefore not a promising optimisation target, whatever
caused its step at #80.

**What is still NOT established:** which phase grew at #80. That needs the pre-#80 binary
(`4d6612a`), which is no longer pinned. The tempting story — #80's two-way markers create more
Tseitin synthetics, so the label cache has more to build — is consistent with the profile above
but is **not measured**, and is recorded here as a hypothesis rather than a cause, for the same
reason #80's own report declined to guess. Walls in this table were taken under contention with
another sweep and are not comparable across arms; only the phase PROPORTIONS and the byte-identical
output are being read from them.

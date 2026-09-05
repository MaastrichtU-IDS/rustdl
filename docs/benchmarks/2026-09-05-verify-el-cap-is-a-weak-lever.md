# Raising `verify-el`'s cap is a weak coverage lever — measured (2026-09-05)

With #87 closed the instrument reports **zero** false positives, so the next question was
coverage: of 1,920 ORE ontologies, **196 were UNMEASURED** because they hit the 60 s cap.
That is the second-largest gap after the fragment restriction, it costs only compute, and
with a clean instrument any violation found there would be a **candidate real engine
defect** — the first by this route.

**Result: 5× the cap buys +2.1 points of coverage and finds nothing. Do not spend more
compute here.**

## Method

`verify-el` takes no internal deadline from the CLI — it passes `None` to `verify`, and
`Bounds` governs construction only (`max_elements` 50k, `max_edges` 2M, `max_rounds` 8).
So the 196 were genuinely slow, not internally truncated, and a longer external cap can
convert them. All 196 re-run at **300 s**, 5 parallel.

## Result

| | 60 s cap | after the 300 s retry |
|---|---:|---:|
| verified | 362 | **403** |
| **violated** | **0** | **0** |
| unresolved (off-fragment / refused) | 1,361 | 1,392 |
| **timeout — UNMEASURED, not passing** | **196** | **124** |
| I/O or parse error | 1 | 1 |
| **CHECKABLE** (verified + violated) | **362 (18.9%)** | **403 (21.0%)** |

Of the 196: **41 → verified, 31 → unresolved, 124 still time out, 0 violated.**

## What this establishes

**The cap is a weak lever, and now quantified.** A 5× increase converts 37% of the
timeout bucket, and **63% is immune to it** — those 124 are not marginal cases sitting
just past 60 s, they are genuinely hard. A further increase would have to fight a
distribution that already resisted 5×.

**Coverage is bounded by the FRAGMENT, not the cap.** After the retry, **1,392 of 1,920
(72.5%)** are `unresolved` — off-fragment or refused. That is the constraint worth
attacking, and it is an engine-side change (extending `verify-el` past `is_pure_el` to the
Horn fragment), not a compute-side one. Note the retry *increased* the unresolved count by
31: several of the slow ontologies turned out to be off-fragment all along, so part of the
timeout bucket was never checkable in the first place.

**Zero violations across everything now measurable.** Combined with the post-#87 scan,
the instrument reports no violation on **403 checkable ontologies**. That is a real, if
unexciting, statement about the EL saturator on this corpus — and it is the first time the
statement can be made at all, since before #87 every `Violated` it produced was its own.

## What it does NOT establish

* **124 ontologies remain UNMEASURED.** Not passing — no verdict was reached. Any claim
  of the form "rustdl is clean on ORE" must carry that number.
* **Coverage is 21%.** The other 79% is not evidence of anything.
* **F1 and F3 are still live** builder false-positive mechanisms, so a future `Violated`
  remains a lead requiring adjudication.

## Recommendation

Stop raising the cap. Per-ontology rows: `2026-09-05-verify-el-timeout-retry.tsv`.

> **RETRACTED THE SAME DAY — the second half of this recommendation was wrong.** It said the
> lever is the fragment restriction, since 72.5% of the corpus is refused before checking begins
> against 6.5% lost to the cap. **The 72.5% is not a market.** `verify` reaches `Verified` only
> with zero unresolved axioms, and every construct that makes an ontology Horn-but-not-EL is
> refused by `eval.rs` — except one (a non-atomic `DisjointClasses` member). A widened gate would
> admit ontologies whose distinguishing axioms are all `Unresolved` and verify them vacuously,
> while `build_model`'s EL-saturator model source would make any `Violated` there an artifact.
> See `2026-09-05-verify-el-horn-widening-market.md`. **A count of refusals is not a count of
> recoverable cases** — that inference is what this file got wrong.

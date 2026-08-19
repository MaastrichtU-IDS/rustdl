# Re-verifying every named open target: 2 claims hold, 1 "retirement" later REFUTED

**Date:** 2026-08-18 · Run because this session's dominant finding is that the design record
drifts **optimistically** — five proposals aimed at already-shipped work, three named targets
already evaporated — and the `#[ignore]`d-sentinel finding showed that skipping this check costs
five weeks.

**Headline: `docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md` is
RETIRED on both of its censused instances.** Two DNF claims still hold.

## The 12 most-cited named ontologies, re-measured at the default

Single-threaded, 120 s cap, current `main`:

| ontology | wall | outcome | verdict on its recorded claim |
|---|---:|---|---|
| `ore_ont_11311` | 120 s | **DNF** | **HOLDS** — still the label-cache-build target |
| `ore_ont_9944` | 120 s | **DNF** | **HOLDS** |
| `ore_ont_1508` | 112.9 s | 13,951 rows | completes (record: 197.8 → 94.9 s) |
| `ore_ont_16056` | 90.2 s | 484 rows | completes, slow |
| `ore_ont_5368` | 85.6 s | 6,099 rows | completes — the 27 GB DNF framing is stale |
| `ore_ont_2574` | 60.6 s | 57,850 rows | completes |
| `ore_ont_7192` | 57.8 s | 50,752 rows | matches the post-parse-charge ~55 s |
| `ore_ont_15010` | 6.0 s | 170 rows | see below — **limitation retired** |
| `ore_ont_9347` | 5.6 s | 112 rows | better than the recorded 10.72 s |
| `ore_ont_11378` | 3.1 s | 8,733 rows | completes |
| `ore_ont_10019` | 2.3 s | 57 rows | confirms the already-recorded MOOT |
| `ore_ont_16847` | 0.4 s | 279 rows | trivial |

So of twelve names the record still discusses, **two are genuinely unfinished**. The rest
complete, several comfortably.

## The retired limitation, and the control that makes it a retirement

| ontology | arm | recorded | now |
|---|---|---:|---:|
| `ore_ont_15010` | default | 5.65 s | **5.99 s** |
| | `--pair-timeout-ms 1` | **103.98 s** | **6.19 s** |
| `ore_ont_15108` | default | 44.65 s | **44.20 s** |
| | `--pair-timeout-ms 1` | **200 s** | **45.40 s** |

**Both DEFAULT arms reproduce their recorded walls.** That is the whole argument: neither
ontology merely got faster, and neither host nor binary is confounding the comparison — only the
*pathological* arm moved. Had both arms dropped, this would be an uninterpretable speedup.

**Confirmed at the mechanism too**, not only the wall: the defect was that a small per-pair
budget starves the per-class build so its 96–100% pruning is lost. At `--pair-timeout-ms 1`,
`ore_ont_15010` now reports `# label heuristic: pruned=9268 pass_through=6 misses=751` — heavy
pruning in precisely the regime that used to starve.

**Cause unattributed, deliberately.** Nothing was fixed on purpose; something in v0.4.12–v0.4.19
or the 2026-08-17/18 work dissolved it. Inventing a mechanism here would repeat the error this
whole document exists to catch.

## The near-miss worth recording

I first measured `ore_ont_15010` at **6.00 s** and nearly declared the limitation closed by
comparing that against the **103.98 s** in its title — which is the `--pair-timeout-ms 1` arm.
The document's own *default* arm is 5.65 s, so 6.00 s was a **match, not a refutation**, and I
had run the wrong arm entirely.

**Read the conditions recorded WITH a number before calling it non-reproducing.** The real
retirement only appeared after running all four arms as the document specified them. A
same-shaped mistake would have produced a confident, wrong "closed".

## Scope

* Closed on the **two instances that document censused**. Its original census covered the 40
  slowest completers; claiming the class is empty needs that census re-run. **What is retired is
  the document's evidence, not a proof of absence.**
* `ore_ont_11311` / `9944` remain the real open target, and CLAUDE.md's budget-allocation finding
  about them stands — with its own recorded caveat that starvation-unblocking is already a
  measured negative (`unsat_probe_cap`), because the starved consumer cannot finish either.

---

## CORRECTION (2026-08-19): the retirement above was TOO BROAD

Re-running the limitation's **own census** found the starvation class has **5 members** in the
40-slowest frame, up from the "~2 known" recorded — and the aggregate wall trade-off **inverted**
(1377 → 1625 s under `pt=1`, where the original measured 1499 → 1267 s).

Both facts are true at once and the distinction is the whole lesson:

* the two **named** instances genuinely stopped reproducing (measured, twice, with controls);
* the **class** is not empty — it has different, more numerous members.

**Retiring a document's named examples is not retiring its defect.** The scope line above said
"what is retired is the document's evidence, not a proof of absence" — correct, but the headline
still read CLOSED, and a reader would have taken the defect as gone. Full data and the
pre-registered analysis: `docs/2026-08-19-label-cache-starvation-census.md`.

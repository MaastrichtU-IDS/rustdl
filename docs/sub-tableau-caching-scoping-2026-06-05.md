# Scoping: Konclude-style sub-tableau caching (2026-06-05)

**Ask.** Scope the "Konclude-style sub-tableau / model caching" lever — the
deferred completeness/perf lever in `handoff-2026-06-05.md` and
`architecture-roadmap.md` Lever C. Is it tractable now? Would it close the
remaining MISSES (notgalen 18, SIO 2)?

**Verdict.** **Do not build it as a completeness lever.** The sound form of
this lever is *already shipped* (Phase 1b/1c snapshot cache). Extending it to
the workloads that have MISSES requires solving the §2 back-propagation
soundness trap, which the staleness finding below does **not** make tractable.
And even if built, it is a *perf* lever, not a *completeness* lever — it cannot
recover the remaining MISSES. One roadmap premise is genuinely stale and is
worth correcting in the docs; that is the only shippable output.

---

## 1. The lever is already shipped (sound form)

`subsumes_via_tableau` (`classify.rs:1452`) consults a per-class **snapshot
cache** *before* the wedge: built on first `(sub, *)` query, replayed against
every later sup with `¬sup` injected (`RUSTDL_SNAPSHOT_CAPTURE`, default ON;
Phase 1b.5 lazy expansion `RUSTDL_SNAPSHOT_LAZY`, default ON). This **is**
"compute sub once, test each sup against the cached model" — the exact lever.

It is gated to `BackPropRisk::Safe` (`snapshot.rs:96`): **no inverse role, no
nominal, no cardinality, no datatype**. Those four are precisely the §2
back-propagation hazards. So:

| Workload | BackPropRisk | Snapshot cache |
|---|---|---|
| GALEN, notgalen, alehif (Horn) | **Safe** | active |
| pizza, SIO, ore-10908, ore-15672 (SROIQ) | **Unsafe** (cardinality/inverse/nominal) | falls through to wedge |

The hard-MISS / hard-perf workloads are exactly the **Unsafe** ones. The open
frontier is extending the cache to the Unsafe fragment — i.e. solving the §2
soundness trap (back-prop into snapshot nodes via `∀R⁻`/nominal/cardinality
invalidates the cached model). That trap is **not** addressed by anything below.

## 2. What is genuinely stale (and what is not)

`architecture-roadmap.md` lines 34–60 (2026-05-27) declared "Model caching
(Lever C) is dead for SIO" on two premises, both measured on the **classic**
tableau:

1. "the tableau cannot build a model of a single SIO class — `sat` times out at 5 s"
2. "blocking never fires (`is_blocked_true = 0`)"

**Both premises are stale for the wedge** (the engine whose models would be cached):

```
rustdl hyper-sat sio.ofn   → all 1585 classes Sat, 668.7 ms total,
                             max 11.84 ms/class, 0 stalled, deferred=0 (real models)
rustdl hyper-sat pizza.ofn → 97 sat / 2 unsat, 32.6 ms total, 0 stalled, deferred=0
```

So the "can't build a model" prerequisite is **no longer dead**. This is the
same shape as the blocking-lever correction (PR #13): a classic-tableau-era
death certificate invalidated by the wedge.

**But that was never the load-bearing objection.** §2's killer is the
**soundness trap on *reuse*** (back-propagation), not model-build cost. §2
*assumes* sub-Sat is cheap — the convergence result is the *premise* of the
tempting optimization, not a refutation of the dead-end. "Death certificate
stale" is true for the build-a-model signature and **false** for the
load-bearing reuse signature.

## 3. Why it cannot recover the MISSES (the gating measurement)

The MISSES are `trust_sat` short-circuits, not slow-path timeouts:

- **SIO 2** (`SIO_010092 ⊑ SIO_001353`, `⊑ SIO_010410`): Unsafe ontology. The
  wedge returns **Sat** → `trust_sat` answers "not subsumed" → MISS, *before*
  the complete path runs. Caching the wedge model would reproduce the same
  *incomplete* Sat faster — it does not change the verdict. Recovering the MISS
  needs the **complete** engine.
- **notgalen 18** (IPBP cluster): Horn ⇒ `RUSTDL_HORN_SHORTCIRCUIT` trusts the
  saturation closure and **bypasses the pair loop entirely**. The snapshot cache
  is never consulted. These are saturation-incompleteness; caching is irrelevant.

Discriminator probe on the complete engine (`sat` / `run_satisfiability` =
classic tableau, the only complete per-pair path):

```
sat(SIO_010092)                         → TIMEOUT  (60 s, classic tableau)
explain(SIO_010092 ⊑ SIO_001353), TRUST_SAT=0 → TIMEOUT  (90 s)
```

Even the **base** model `sat(sub)` is intractable on the complete engine. So:
the model you *can* cache (wedge) gives the incomplete answer; the model that
would give the *complete* answer (classic tableau) can't be built at all. Either
branch → **no MISS recovery**. This is exactly §B's "sound but won't move the
numbers," now confirmed for the real target pairs.

## 4. Engine map (for future readers)

| Path | Engine | SIO_010092 |
|---|---|---|
| `rustdl hyper-sat`, classify per-pair wedge | hypertableau (wedge) | Sat in ms (incomplete on Unsafe) |
| `rustdl sat`, `explain` fall-through | classic tableau (`run_satisfiability`) | timeout 60 s (complete) |
| classify snapshot cache | wedge replay, **Safe-gated** | not used (SIO is Unsafe) |

The wedge is fast but `trust_sat`-incomplete on Unsafe; the classic tableau is
complete but intractable per-pair on SIO/pizza. Caching bridges neither gap.

## 5. Recommendation

1. **Ship the doc correction** (cheap, real): update `architecture-roadmap.md`
   lines 34–60 to note the "can't build a model of a single SIO class" premise
   is stale for the wedge (668 ms / 1585 classes), as the blocking premise
   already was. Keep §2's reuse-soundness objection standing — it is the live
   blocker.
2. **Do not pursue Unsafe-fragment caching for completeness.** It is blocked by
   the unsolved §2 trap *and*, even if solved, recovers no MISSES (§3).
3. The real completeness path is unchanged and research-grade: a **complete
   engine fast enough to afford `trust_sat=off`** on Unsafe workloads (the
   blocking lever / hypertableau completeness), not caching. See
   `handoff-2026-06-05.md` "MISS probe".

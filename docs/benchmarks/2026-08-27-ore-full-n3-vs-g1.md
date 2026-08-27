# Full ORE (1,920) on n3 vs g1 — the deployments agree; n3 is not usable for TIMING

**Binary: ONE binary, both hosts.** `rustdl 0.4.23` @ `4fe9487`, sha256 `4839aec2c957e4e9`, built on
g1 and copied to n3 (glibc identical: `Ubuntu GLIBC 2.35-0ubuntu3.14` both sides). Each host's own
build had a *different* sha, which would have confounded machine with binary — so the g1 pin was
shipped and its sha verified on n3 before either arm ran.

Cap 120 s, `--threads 1`, `.owl`, 1,920 files. Chunks: n3 = 16, g1 = 6.
Anything over the cap is reported `dnf`, not missing.

## Outcomes

| | g1 | n3 |
|---|---:|---:|
| ok | **1,797** | 1,795 |
| dnf | 118 | **120** |
| err_reject | 5 | 5 |

Transitions g1 → n3: **1,795 ok→ok, 118 dnf→dnf, 5 err→err, 2 ok→dnf.** Both movers adjudicated:

* **`ore_ont_1508`** — g1 117.63 s → n3 dnf. Re-run sequentially on g1: **120.6 / 119.9 / 118.6 s**,
  i.e. it *straddles the 120 s cap*, so ok-vs-dnf is a coin toss. This is the ontology the design
  record already names as the clean instance of budget-induced nondeterminism.
* **`ore_ont_6923`** — g1 40.09 s (re-run: **40.2 / 40.0 / 39.9 s, rock stable**) → n3 dnf. On n3 it
  gave 60.9 / 91.4 / 76.4 s. **Attributed to external load, not the machine** — see below.

## Answers

Over the 1,795 both-ok ontologies, comparing `consistent` + `unsatisfiable` + `equivalent_groups` +
`direct_subsumptions` (`incomplete` deliberately **excluded** — it is a timing flag reporting whether
a pair hit the per-pair deadline, so it flips under contention without any answer changing):

| | |
|---|---:|
| comparable | 1,795 |
| **IDENTICAL** | **1,792** |
| differing | 3 |

The 3 — `ore_ont_12698`, `ore_ont_15066`, `ore_ont_7893` — are **n3-side nondeterminism, not a
machine difference**:

| ontology | g1 (2 runs) | n3 (2 runs) |
|---|---|---|
| `12698` | stable `7afa2200a1611ff5` | `8957579edc14b7eb` / `d05098020f549323` |
| `15066` | stable `48141981d962d896` | `df4a61b5ed92be18` / `b490219206f473e2` |
| `7893` | stable `3c006ed9432a508e` | **`3c006ed9432a508e`** / `22865a9e93dcdf4a` |

All three are stable on g1 and unstable on n3, and `7893`'s first n3 hash **equals g1's**. Under load,
per-pair deadlines fire at different points, truncation differs, and the answer moves.

## THE HEADLINE CAVEAT: n3 was not idle, and it is not ours alone

| | g1 | n3 |
|---|---|---|
| loadavg | **2.60** | **57.21** |

n3's top consumer during measurement was another user's `python3` at **4,753% CPU (~47 of 96 cores)**,
uid 756360, alongside `clickhouse-server` and `k3s-server`.

**Consequence: n3's wall distribution and its 2 extra DNFs are artefacts of that load, not
properties of the hardware.** The 96 cores are present but roughly half were already committed.

* **Do NOT use n3 for wall-time or DNF measurement** without first confirming it is quiet
  (`cut -d' ' -f1 /proc/loadavg`). A DNF verdict is wall-dependent, so a loaded host manufactures
  DNFs — which is precisely what happened to `ore_ont_6923`.
* It is fine for **answer-only** checks, provided each result is re-run for stability, since
  contention perturbs deadline-sensitive answers.
* g1's numbers here are the trustworthy ones: loadavg 2.6 throughout.

## Reference: g1 at v0.4.23, full corpus, 120 s cap

| ok-only | mean | median | p90 | max |
|---|---:|---:|---:|---:|
| wall (s) | 2.46 | 0.16 | 4.33 | 117.63 |
| peak RSS (MiB) | 190 | 28 | 480 | **16,291** |

**118 DNF at a 120 s cap.** Not comparable to the "140" and "143" figures in the design record — those
were censused at a 20 s budget, and a cap is not a neutral sampler.

## Method notes

* **A `nohup … &` launch is not durable.** The g1 arm's chunk `c05` appeared to die at ~200/331 while
  the other five finished; the aggregate read 1,904/1,920 and *looked* complete, with every chunk log
  reporting a sensible count. Only a per-chunk count exposed it. The missing items were **biased
  toward the slow tail** — the population that matters — so aggregating there would have inflated the
  completion rate. Run a sweep inside one long-lived job, as the n3 arm was.
* **The backfill produced 13 duplicates**, because `c05` had not in fact died — its records were still
  flushing when `pgrep` showed zero. All 13 duplicates **agree on outcome**, so dedup was safe; that
  was checked, not assumed.
* **The answer hasher initially globbed `*.out`.** The harness captures `*.json`, so it would have
  found zero files and reported a **false "identical"**. Caught by self-testing it against two real
  files before the runs finished.

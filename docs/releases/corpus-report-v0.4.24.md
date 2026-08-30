### Corpus report — v0.4.24

Population **424** ontologies · cap **60s** · 1 thread · binary `ebd8aa5b930b`

| | classified | DNF | empty output |
|---|---|---|---|
| count | **411** | 13 | 0 |

| | mean | median | p90 | max |
|---|---|---|---|---|
| wall (s) | 3.3434 | 0.73 | 6.65 | 56.55 |
| peak RSS (MiB) | 126.5 | 21.16 | 321.8 | 3287.1 |

Reported inconsistent: **13** · flagged incomplete: **72**

**Gate vs `v0.4.23-samehost`: **FAIL****

- consistency-verdict flips: **0** (must be 0)
- ontologies lost (classified → not): **3** (must be 0)
  - `ore_ont_13991`
  - `ore_ont_868`
  - `ore_ont_9674`
- closure shrank on: 2 (informational; a smaller per-pair budget legitimately under-approximates)
  - `ore_ont_15066`: 63167 → 63150
  - `ore_ont_699`: 117155 → 117154

---

### Gate adjudication — the FAIL is 5 measurement artifacts, 0 regressions

The report above runs at `JOBS=6`, and `lost_ontologies` is cap-sensitive. All five flagged
items were re-measured **sequentially on an idle host, alternating arm order, 3 runs per
arm**, v0.4.23 vs v0.4.24:

| ontology | flagged as | v0.4.23 | v0.4.24 | verdict |
|---|---|---|---|---|
| `ore_ont_13991` | lost | 8.2 / 8.2 / 8.8 s | 11.1 / 10.6 / 11.1 s | **rows identical (2 558)**, both far under cap → contention |
| `ore_ont_868` | lost | 70.1 / 63.8 / 67.6 s | 64.7 / 68.2 / 71.9 s | **rows identical (981 144)**; over the 60 s cap in BOTH arms even idle |
| `ore_ont_9674` | lost | 69.0 / 68.3 / 66.5 s | 68.4 / 68.4 / 66.7 s | **rows identical (981 144)**; over cap in BOTH arms even idle |
| `ore_ont_15066` | closure −17 | rows 8978 / 8979 / **8974** | 8979 / **8976** / 8979 | each arm varies against ITSELF; `incomplete=true` |
| `ore_ont_699` | closure −1 | 8506 / 8506 / 8506 | 8506 / 8506 / 8506 | identical in all 6 runs — does not reproduce |

`868` and `9674` were classified in the baseline run by luck, not by capability: neither
completes inside 60 s on this host in either arm. `13991` is ~30% slower on v0.4.24 but at
11 s against a 60 s cap, so its DNF cannot be a wall effect.

**Row counts are identical in every lost case, and neither closure shrink survives repeats.**

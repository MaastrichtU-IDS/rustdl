### Corpus report — v0.4.23

Population **424** ontologies · cap **60s** · 1 thread · binary `9704e28dfb75`

| | classified | DNF | empty output |
|---|---|---|---|
| count | **417** | 7 | 0 |

| | mean | median | p90 | max |
|---|---|---|---|---|
| wall (s) | 2.6397 | 0.21 | 5.32 | 59.7 |
| peak RSS (MiB) | 159.9 | 22.57 | 367.1 | 6711.6 |

Reported inconsistent: **13** · flagged incomplete: **65**

**Gate vs `v0.4.22`: **FAIL****

- consistency-verdict flips: **0** (must be 0)
- ontologies lost (classified → not): **1** (must be 0)
  - `ore_ont_2574`
- closure shrank on: 1 (informational; a smaller per-pair budget legitimately under-approximates)
  - `ore_ont_12698`: 126737 → 126736

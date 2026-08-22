### Corpus report — v0.4.22

Population **424** ontologies · cap **60s** · 1 thread · binary `5825001000e4`

| | classified | DNF | empty output |
|---|---|---|---|
| count | **417** | 7 | 0 |

| | mean | median | p90 | max |
|---|---|---|---|---|
| wall (s) | 2.6444 | 0.21 | 5.64 | 58.19 |
| peak RSS (MiB) | 162.5 | 22.53 | 367.1 | 6711.6 |

Reported inconsistent: **13** · flagged incomplete: **64**

**Gate vs `v0.4.21`: **FAIL****

- consistency-verdict flips: **0** (must be 0)
- ontologies lost (classified → not): **1** (must be 0)
  - `ore_ont_7204`
- closure shrank on: 0 (informational; a smaller per-pair budget legitimately under-approximates)

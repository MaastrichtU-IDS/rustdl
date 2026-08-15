### Corpus report — PAIR5-PROBEFIX

Population **424** ontologies · cap **60s** · 1 thread · binary `93f54fb23e31`

| | classified | DNF | empty output |
|---|---|---|---|
| count | **415** | 9 | 0 |

| | mean | median | p90 | max |
|---|---|---|---|---|
| wall (s) | 2.631 | 0.2 | 5.13 | 59.83 |
| peak RSS (MiB) | 147.1 | 22.44 | 356.1 | 6711.7 |

Reported inconsistent: **13** · flagged incomplete: **64**

**Gate vs `v0.4.18`: PASS**

- consistency-verdict flips: **0** (must be 0)
- ontologies lost (classified → not): **0** (must be 0)
- closure shrank on: 3 (informational; a smaller per-pair budget legitimately under-approximates)
  - `ore_ont_7532`: 10552 → 10550
  - `ore_ont_7893`: 973 → 971
  - `ore_ont_9662`: 9163 → 9161

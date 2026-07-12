# rustdl performance matrix

**Date:** 2026-07-12T03:40:45Z  
**Tier:** curated  
**Oracle:** konclude-0.7.0-1138 (FP = asserts what the oracle does not; MISSED = oracle subsumptions not asserted)  
**Host:** Mac17,6 · Apple M5 Max · 18 cores · 128 GB · macOS 26.5.1 (Darwin 25.5.0)  
**Budgets:** per-pair 250 ms, global 60 s

> **Caveats.** HermiT/ELK walls & RSS are end-to-end **JVM** figures (~0.4–1 s boot floor, ~240 MB baseline) — not pure reasoning time. Konclude runs under **Rosetta 2** (x64), so its walls/RSS are upper bounds. `n/a` = EL-only reasoner on a non-EL ontology.

| ontology | frag | classes | rustdl wall | rustdl RSS | rustdl FP/M | konclude wall | konclude RSS | konclude FP/M | hermit wall | hermit RSS | hermit FP/M | elk wall | elk RSS | elk FP/M | whelk-rs wall | whelk-rs RSS | whelk-rs FP/M |
|---|---|--:|--:|--:|:--|--:|--:|:--|--:|--:|:--|--:|--:|:--|--:|--:|:--|
| family | DL | 58 | 820 ms | 1300 MB | FP 0 / M 0 | 440 ms | 220 MB | FP 0 / M 0 | inconsistent | 810 MB | — | n/a | — MB | — | n/a | — MB | — |
| galen | EL | 2748 | 870 ms | 147 MB | FP 0 / M 0 | 180 ms | 96 MB | FP 0 / M 0 | 2350 ms | 1244 MB | FP 0 / M 6 | 880 ms | 411 MB | FP 0 / M 33 | n/a | — MB | — |
| pizza | DL | 99 | 460 ms | 23 MB | FP 0 / M 0 | 90 ms | 38 MB | FP 0 / M 0 | err | 238 MB | — | n/a | — MB | — | n/a | — MB | — |
| ro | DL | 58 | 20 ms | 36 MB | FP 0 / M 0 | 130 ms | 53 MB | FP 0 / M 0 | DNF | 3200 MB | — | n/a | — MB | — | n/a | — MB | — |
| sio | DL | 1585 | 230 ms | 96 MB | FP 0 / M 0 | 210 ms | 89 MB | FP 0 / M 0 | 20540 ms | 1158 MB | — | n/a | — MB | — | n/a | — MB | — |
| sulo | DL | 17 | 0 ms | 8 MB | FP 0 / M 0 | 70 ms | 30 MB | FP 0 / M 0 | 400 ms | 239 MB | — | n/a | — MB | — | n/a | — MB | — |
| trivial | EL | 3 | 0 ms | 4 MB | FP 0 / M 0 | 60 ms | 27 MB | FP 0 / M 0 | 280 ms | 144 MB | FP 0 / M 0 | 330 ms | 198 MB | FP 0 / M 0 | n/a | — MB | — |
| wine | DL | 137 | 90 ms | 41 MB | FP 0 / M 0 | 150 ms | 63 MB | FP 0 / M 0 | 2410 ms | 305 MB | FP 0 / M 6 | n/a | — MB | — | n/a | — MB | — |

## Summary

| reasoner | finished | DNF | error | n/a | total FP | total MISSED |
|---|--:|--:|--:|--:|--:|--:|
| rustdl | 8 | 0 | 0 | 0 | 0 | 0 |
| konclude | 8 | 0 | 0 | 0 | 0 | 0 |
| hermit | 5 | 1 | 1 | 0 | 0 | 12 |
| elk | 2 | 0 | 0 | 6 | 0 | 33 |
| whelk-rs | 0 | 0 | 0 | 8 | 0 | 0 |

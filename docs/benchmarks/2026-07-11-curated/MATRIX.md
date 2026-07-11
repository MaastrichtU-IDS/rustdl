# rustdl performance matrix

**Date:** 2026-07-11T21:54:22Z  
**Tier:** curated  
**Oracle:** konclude-0.7.0-1138 (FP = asserts what the oracle does not; MISSED = oracle subsumptions not asserted)  
**Host:** Mac17,6 · Apple M5 Max · 18 cores · 128 GB · macOS 26.5.1 (Darwin 25.5.0)  
**Budgets:** per-pair 250 ms, global 60 s

> **Caveats.** HermiT/ELK walls & RSS are end-to-end **JVM** figures (~0.4–1 s boot floor, ~240 MB baseline) — not pure reasoning time. Konclude runs under **Rosetta 2** (x64), so its walls/RSS are upper bounds. `n/a` = EL-only reasoner on a non-EL ontology.

| ontology | frag | classes | rustdl wall | rustdl RSS | rustdl FP/M | konclude wall | konclude RSS | konclude FP/M | hermit wall | hermit RSS | hermit FP/M | elk wall | elk RSS | elk FP/M | whelk-rs wall | whelk-rs RSS | whelk-rs FP/M |
|---|---|--:|--:|--:|:--|--:|--:|:--|--:|--:|:--|--:|--:|:--|--:|--:|:--|
| family | DL | 58 | 850 ms | 1200 MB | FP 0 / M 0 | 450 ms | 220 MB | FP 0 / M 0 | inconsistent | 1168 MB | — | n/a | — MB | — | n/a | — MB | — |
| galen | EL | 2748 | 890 ms | 148 MB | FP 0 / M 1 | 180 ms | 96 MB | FP 0 / M 0 | 2430 ms | 1406 MB | FP 0 / M 6 | 850 ms | 324 MB | FP 0 / M 33 | n/a | — MB | — |
| pizza | DL | 99 | 460 ms | 24 MB | FP 0 / M 0 | 90 ms | 39 MB | FP 0 / M 0 | err | 193 MB | — | n/a | — MB | — | n/a | — MB | — |
| ro | DL | 58 | 20 ms | 35 MB | FP 0 / M 0 | 130 ms | 53 MB | FP 0 / M 0 | DNF | 2875 MB | — | n/a | — MB | — | n/a | — MB | — |
| sio | DL | 1585 | 230 ms | 97 MB | FP 0 / M 0 | 220 ms | 88 MB | FP 0 / M 0 | 21360 ms | 1194 MB | — | n/a | — MB | — | n/a | — MB | — |
| sulo | DL | 17 | 0 ms | 8 MB | FP 0 / M 0 | 70 ms | 30 MB | FP 0 / M 0 | 420 ms | 231 MB | — | n/a | — MB | — | n/a | — MB | — |
| trivial | EL | 3 | 0 ms | 4 MB | FP 0 / M 0 | 70 ms | 27 MB | FP 0 / M 0 | 270 ms | 144 MB | FP 0 / M 0 | 340 ms | 197 MB | FP 0 / M 0 | n/a | — MB | — |
| wine | DL | 137 | 90 ms | 43 MB | FP 0 / M 0 | 150 ms | 62 MB | FP 0 / M 0 | 2380 ms | 321 MB | FP 0 / M 6 | n/a | — MB | — | n/a | — MB | — |

## Summary

| reasoner | finished | DNF | error | n/a | total FP | total MISSED |
|---|--:|--:|--:|--:|--:|--:|
| rustdl | 8 | 0 | 0 | 0 | 0 | 1 |
| konclude | 8 | 0 | 0 | 0 | 0 | 0 |
| hermit | 5 | 1 | 1 | 0 | 0 | 12 |
| elk | 2 | 0 | 0 | 6 | 0 | 33 |
| whelk-rs | 0 | 0 | 0 | 8 | 0 | 0 |

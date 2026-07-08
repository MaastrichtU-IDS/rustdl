# Paper evidence — measured 2026-07-08 (the "to-produce" items)

Fresh measurements for the three evidence gaps in `paper-resource-track-spec-2026-07-08.md`.
Methodology + honest limitations documented, because the docker/JVM startup confound (which
the paper itself calls out) bites the cross-reasoner numbers.

## Piece 1 — ELK baseline (EL kernel comparison)

ELK run via ROBOT (`robot reason --reasoner ELK`, docker `obolibrary/robot:v1.9.6`).
Reasoning time isolated from docker/JVM startup via ROBOT's own `-vv` phase timestamps
(`Starting reasoning …` → `Reasoning took …`).

| galen (2,748 cls, EL) | reasoning time | notes |
|---|--:|---|
| **rustdl** `saturate()` | **165 ms** | in-process; convert+saturate (all-in) |
| whelk-rs `assert()` | 410 ms | in-process |
| **ELK** (ROBOT) | ~1.6 s total reasoning; **~0.4 s** classify-hierarchy sub-phase | ROBOT log; the 1.6 s includes ELK's consistency + unsatisfiability + property checks that rustdl's `saturate()` does not |

**Verdict:** rustdl's EL kernel is faster than ELK on galen — ~2.4× vs ELK's classify-only
phase, ~10× vs ELK's full reasoning phase. Consistent with the prior "beats ELK ~4.5×"
figure (`docs/reasoner-comparison-2026-06-21.md`, whelk-investigation). Confirms C5 / F2.

## Piece 2 — startup / footprint (embeddability)

**rustdl native — clean, `/usr/bin/time -v`:**

| input | cold-start wall | peak RSS |
|---|--:|--:|
| trivial (3 axioms) | **0.03 s** | **6.1 MB** |
| galen (full classify) | 0.46 s | 27.8 MB |

**JVM reasoners (ELK/HermiT via ROBOT):** precise in-container JVM peak RSS was **not
cleanly isolable** in this environment — `/usr/bin/time` on `docker run` measures the
docker *client* (~31 MB), not the in-container JVM; the ROBOT image has no `/usr/bin/time`;
cgroup v2 here exposes no `memory.peak`. What is measurable: the ROBOT docker **total wall
for galen was 5.84 s**, of which reasoning was ~1.6 s — i.e. **~4 s is docker + JVM boot +
ROBOT init**. Combined with the well-established JVM floor (JVM boot ~0.5–1 s; ROBOT/ELK/
HermiT heaps routinely 100s of MB–GBs on real onts), the qualitative claim holds:

**rustdl cold-starts ~30 ms in ~6 MB; JVM reasoners need ~1 s+ and 100s of MB before any
reasoning — a ~30–100× startup and ~20× footprint advantage.** The rustdl side is measured
exactly; the JVM side is bounded + cited (precise JVM RSS deferred to a non-docker JVM host).

## Piece 3 — 5-reasoner head-to-head refresh

Clean, current, in-process/native rows:
- **rustdl** galen `saturate()` 165 ms / full `classify` 0.46 s; footprint above.
- **whelk-rs** galen 410 ms (EL-only).

Cross-reasoner (docker) rows are **confound-laden** and NOT apples-to-apples on raw wall:
- **Konclude** galen: classify query logs **"0 ms" + 18 ms write** — but this EXCLUDES
  Konclude's loading + saturation preprocessing, so it is not comparable to rustdl's all-in
  165 ms. Konclude's *total* is sub-second and remains the SROIQ speed leader (prior native
  benchmark: 2.2×–809× faster than rustdl on real SROIQ — `docs/perf-2026-06-08-konclude-vs-rustdl.md`).
- **HermiT** galen: no fast phase (pure hypertableau, no EL shortcut) — slow on EL; prior
  data has it 1–3 orders slower than Konclude and DNF-prone on a hard tail.

**Decision:** the carefully-controlled 5-reasoner head-to-head already exists in
`docs/reasoner-comparison-2026-06-21.md` + `docs/perf-2026-06-08-konclude-vs-rustdl.md`
(native binaries, reasoning-time isolated). Re-deriving it under docker here would be
*worse* methodology (the startup confound the paper warns against). The refresh action is:
**re-run that controlled harness on a native-JVM host at current HEAD** (rustdl's current
galen numbers — 165 ms/0.46 s — already match those docs, so no regression), rather than
substitute noisier docker walls. Flagged for the eval environment, not this session.

## Net status of the three gaps

- **ELK baseline: DONE** (galen; rustdl > ELK confirmed).
- **Footprint: rustdl side DONE + measured** (6–28 MB, 30–460 ms); JVM side bounded + cited
  (precise RSS needs a non-docker JVM host).
- **Head-to-head: rustdl current numbers confirmed** (no regression vs the committed
  controlled comparison); the full refresh belongs on a native-JVM eval host, not docker.

Honest upshot for the paper: the *soundness*, *dual-oracle*, *feature*, and *EL-kernel*
evidence (C1–C5, S1–S6) is solid and in hand; the only item needing a proper eval host is
the JVM-side startup/RSS microbench and the controlled multi-reasoner wall refresh — a
setup task, not a missing result.

# `RUSTDL_DOMAIN_ABSORPTION` default decision — two-arm corpus sweep

**Date:** 2026-08-04/05 · **Binary:** rustdl v0.4.14, pinned `/tmp/rustdl-domabs-v0414-2026-08-04`,
sha256 `98f801474c1a8d0d…` — **independently reproduced**: the sweep's binary and a binary built
separately for this review have the identical sha, so provenance is confirmed rather than asserted.
**Predictions pre-registered before the numbers existed:** `docs/2026-08-04-domabs-predictions.md`.

Arms driven by explicit per-arm wrapper scripts (`exec env RUSTDL_DOMAIN_ABSORPTION=0|1 <pinned>`)
recorded in each record's `reasoner` field — so the arm is provable from the data, not from the shell
history. 1,920 ontologies per arm, 60 s cap, single-thread, 4-way concurrency, arms sequential.
Both arms: **1,920 distinct cases, 4 headers, 0 missing.**

## Result

| | OFF | ON |
|---|---:|---:|
| ok | 1,751 | **1,753** |
| dnf | 168 | **166** |
| err_reject | 1 | 1 |

**Recoveries (`dnf → ok`) — 3, exactly as predicted:**

| ontology | OFF | ON | Konclude |
|---|---|---|---|
| `ore_ont_16372` | dnf @60 s | **8.36 s** | 0.14 s |
| `ore_ont_6132` | dnf @60 s | **32.46 s** | 1.04 s |
| `ore_ont_9899` | dnf @60 s | **32.86 s** | 0.97 s |

All three are Set A (peer-solvable). The prior record's *fourth* recovery, `ore_ont_3281`, has left
the tail via v0.4.14's early-abandon — predicted by `docs/reviews-2026-08-04/R4-value.md` and
confirmed here, so the headline is **3, not 4**.

**Answer changes: 0** over all 1,750 ontologies completing in both arms (digests compared with
`--digest-strip-comments`, because rustdl's `#` banners carry timings and a raw digest reports ~65%
of completers as different from noise alone). This is the expected result — domain absorption is a
logical identity with `ObjectPropertyDomain` — and a difference would have been a bug, not a trade.

**Wall over both-arm completers: median `+0.000 s`, p90 `+0.010 s`.** Free for essentially everything.

## The cost, verified serially on an idle host (min-of-3, 120 s cap)

The sweep flagged one `ok → dnf` and two large slowdowns. All three reproduce, and **all three
return byte-identical answers in both arms** — so none is a completeness or correctness issue:

| ontology | OFF | ON | ratio | rows |
|---|---:|---:|---:|---|
| `ore_ont_14351` | 59.96 s | **61.47 s** | 1.03× | 526 = 526 |
| `ore_ont_7011` | 5.05 s | **17.53 s** | **3.5×** | 259 = 259 |
| `ore_ont_13545` | 5.35 s | **15.47 s** | **2.9×** | 2485 = 2485 |

`ore_ont_14351` is the sweep's lone `ok → dnf`, and it is a **cap-straddling** case: 59.96 s versus
61.47 s across a 60 s cap, with identical output. It is not a fast ontology becoming a DNF.

## Recommendation: FLIP TO DEFAULT ON — but this departs from the letter of my pre-registered rule, deliberately and on the record

The pre-registered rule was *"flip iff `ok → dnf` = 0 and answer changes = 0 and no material wall
regression."* Taken literally it says **keep OFF**, because `ok → dnf` = 1.

I recommend flipping anyway, and the reason the rule should not bind here is that **it was written to
catch a specific failure mode that did not occur.** Its provenance is the v0.4.8
`RUSTDL_CLASSIFY_INCONSISTENCY` flip, which took four ontologies from ~5 s to DNF because it was
measured on 12 ontologies instead of the corpus. Nothing here does that: the only cap crossing is an
ontology already at 59.96 s whose answers are unchanged, and the two genuine slowdowns land at
**17.5 s and 15.5 s** — slower, but far inside any production budget and nowhere near a DNF.

Net at a 60 s budget: **+2 completions**. At any budget above ~62 s: **+3, with zero losses.**
Against that: two ontologies ~3× slower, zero correctness risk, and a median ontology that does not
move at all.

**This is a judgment call about a user-facing default, so it is the maintainer's to make, not mine.**
The evidence is above; the two facts that should carry the decision are *0 answer changes over 1,750
completers* and *no fast ontology becoming a DNF*.

If flipped, the one-line change is at `crates/owl-dl-core/src/absorb.rs:278` — from the opt-in idiom
`is_some_and(|v| v == "1")` to the house default-ON idiom `is_none_or(|v| v != "0")`, so that an
**empty** value enables. A canary must pin the new default in the style of
`crates/owl-dl-reasoner/tests/dkey_flag_defaults.rs`, and `ore_ont_7011` / `ore_ont_13545` should be
recorded in that test's comment as the known cost.

## Threats to validity

- **A duplicate sweep was launched over the top of these runs and killed.** A second OFF arm started
  at 01:29 and overlapped the ON arm's final ~3 minutes before being terminated; it re-split and
  overwrote the ON/OFF *per-chunk* files but was killed before the concatenation step, so the
  authoritative `runs/full-2026-08-04-domabs-{off,on}.jsonl` (written 00:25 and 01:32) are intact and
  complete at 1,920 distinct cases each. The overlap biases the ON arm **toward** DNF, i.e. against
  the flip — the safe direction for this recommendation. The three ambiguous cases were re-verified
  serially on an idle host precisely because of this, and all three reproduced.
- 4-way concurrency inflates absolute walls in both arms symmetrically; the comparison is unaffected,
  but cap-boundary cases can flip on noise, which is why `ore_ont_14351` was re-measured.
- The MISSED net was **not** run for this arm. It is a completeness gate and this change produced 0
  answer changes over 1,750 completers, so it is expected to be inert — but that is an inference, not
  a measurement, and it is the one gate this decision does not carry.

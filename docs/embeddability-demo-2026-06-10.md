# Embeddability demo — rustdl as an in-process, native EL/Horn classifier

Date: 2026-06-10. Validates the **Resource/In-Use-track** "who needs this"
framing chosen after the prefilter/completeness-certificate framings were
shown to lose to "just run Konclude" (Konclude is faster + complete + rarely
DNFs; see `paper-claims` §1). The honest, *measured* niche: an **embeddable,
native, sound, anytime classifier for the EL/Horn fragment** under latency- and
footprint-constrained deployments, where JVM reasoners (ELK/HermiT — startup +
heap) and an external Konclude process (subprocess/IPC + license) are
liabilities.

All numbers: this host, `/usr/bin/time -v` for wall + peak RSS; JVM reasoners
via the `obolibrary/robot:v1.9.6` docker image with the **docker container
overhead measured separately (~0.48 s) and subtracted** to isolate JVM cost.

## 1. Cold-start floor (time-to-first-result, trivial 2-class ontology)
| reasoner | cold start | peak RSS |
|---|---|---|
| **rustdl** (native) | **~2 ms** | **5 MB** |
| Konclude (native) | ~10–60 ms | 21 MB |
| HermiT (JVM) | **~1.0 s** (1.45 s − 0.48 docker) | JVM heap (100s of MB) |
| ELK via ROBOT (JVM) | ~2.9 s (3.39 s − 0.48 docker) | JVM heap |

rustdl's cold start is **~500× lower than the JVM reasoners** and below even
native Konclude — it has no runtime to initialize.

## 2. In-process embedding (the tangible differentiator)
`cargo run -p owl-dl-reasoner --example embed_classify` links the reasoner as a
**library** and classifies in the host process — no subprocess, no JVM, no
Konclude license:
- **bibtex** (15 classes): **0.2 ms** in-process, `complete=true`.
- **GALEN** (EL, 2748 classes, 27 997-pair closure): **482 ms** in-process,
  `complete=true (timed_out_pairs=0)`.

The API returns the **calibrated-incompleteness signal** directly
(`stats().timed_out_pairs == 0 ⟹ complete`), so an embedder gets soundness +
a per-result completeness guarantee, not just a hierarchy. ELK/HermiT require a
JVM in-process (or a subprocess); Konclude is a separate process. rustdl is the
only one that is a plain Rust function call (and compiles to WASM/edge).

## 3. Footprint vs fragment (the honest boundary)
rustdl's footprint is **fragment-dependent**: lean on the EL/saturation path,
heavy on the out-of-EL tableau path.

| ontology | rustdl mode | rustdl wall | rustdl RSS | Konclude |
|---|---|---|---|---|
| bibtex (Horn) | EL saturation | <10 ms | **5 MB** | — |
| GALEN (EL, 27 997) | EL saturation | 0.52 s | **30 MB** | — |
| shoiq-knowledge (out-of-EL) | hybrid tableau | 5.1 s | 308 MB | 0.16 s / 50 MB |
| pizza (SROIQ) | hybrid tableau | 8.5 s | 49 MB | — |
| alehif (ALC, has ∀) | hybrid tableau | 6.5 s | **1.47 GB** | 0.18 s / 60 MB |

**On EL/Horn rustdl is lean and fast** (5–30 MB, sub-second on GALEN-scale).
**On out-of-EL SROIQ it is NOT competitive** — slower and heavier than
Konclude (alehif 6.5 s / 1.47 GB vs 0.18 s / 60 MB). So the embeddable niche is
**EL/Horn (≈72 % of the ORE corpus: 256 PureEl + 233 Horn of 679 OK onts)**,
not full SROIQ. This is stated as a limit, not hidden.

## 4. The "who needs this" (scoped, honest)
A native, in-process, sound EL/Horn classifier with ~2 ms cold start, 5–30 MB
footprint, an anytime/bounded mode, and a per-result completeness signal — for
**serverless/FaaS, edge/WASM, CI ontology-gating, and bulk many-small-ontology
pipelines**, where each invocation is a fresh cold process and the JVM startup
tax (~1 s) + heap, or an external Konclude process + license, dominate. On
these workloads the startup/footprint advantage compounds per-request; on a
single heavyweight SROIQ classification it does not apply (use Konclude).

**Track:** Resource / In-Use. NOT a "beats Konclude on reasoning" claim —
Konclude wins on raw reasoning. The contribution is the *deployment profile* +
the soundness/calibration contract, on the fragment where rustdl is lean.

Artifact: `crates/owl-dl-reasoner/examples/embed_classify.rs`.

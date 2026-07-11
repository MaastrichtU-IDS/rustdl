# rustdl × neuro-symbolic reasoning — research agenda (2026-07-11)

Forward-looking directions for using/extending rustdl as the symbolic half of
neuro-symbolic (NeSy) systems. Not a spec and not a commitment — a prioritized
map of the space, to pick from later. Companion to the paper's motivating
scenario (`docs/paper-resource-track-spec-2026-07-08.md`; the LLM-assisted
authoring loop is written up there as Pattern 1).

## Why rustdl fits the symbolic half

The NeSy-relevant properties are exactly rustdl's contributions:

- **Sound (FP=0):** a symbolic verdict the neural half can trust as a hard
  constraint / reward, not another noisy signal.
- **Self-explaining (`justify`/`prove`/`diagnose`/`repair`):** produces *why*, not
  just *whether* — consumable by an LLM, a human, or a training loss.
- **Calibrated (`completeness_guaranteed()`):** knows when its "no" is a guarantee
  vs an approximation — a routing signal.
- **Embeddable (native Rust + Python, ms/MB, no JVM):** runs in the model's
  process at interactive latency; can be called per-token/per-step.

## The patterns

### Pattern 1 — LLM-assisted authoring loop (in the paper)
LLM proposes an ontology edit → rustdl classifies + checks consistency → on a
clash returns `diagnose`/`justify`/`repair` → LLM revises. rustdl is the sound
checker/explainer/repairer. **Status:** all services built + evaluated; the
autonomous agent is the open piece. Cheapest to prototype (wrap the Python API in
an agent loop; pizza / a real ontology as the sandbox).

### Pattern 2 — Neural recall + symbolic precision (KG/ontology completion)
Embeddings or a GNN propose candidate axioms/links at high recall (fuzzy, fast);
rustdl confirms which are **entailed** (precision) and **explains** each. Neural
generate → symbolic filter+explain. Fits ontology enrichment and KG completion,
where the bottleneck is trustworthy precision, not recall.
- *rustdl provides:* sound entailment check per candidate; a justification per
  accepted link (provenance); `materialize_*` to expand the confirmed set.
- *Open:* candidates are usually *plausible* not *entailed* — most won't be
  entailed by the current axioms, so this confirms "already-implied" links, not
  genuinely new knowledge. More useful framed as **consistency filtering** (reject
  candidates that make the KB inconsistent, via the ABox-saturation pre-check) than
  as entailment filtering. Needs efficient incremental re-check per candidate.

### Pattern 3 — Reasoner as verifier / training signal
Use rustdl's entailments, consistency verdicts, and proofs to supervise or gate a
neural model: filter LLM-generated axioms/answers that are unsound or introduce
inconsistency; use proof trees as step-level supervision; a symbolic-consistency
term in a loss/reward.
- *rustdl provides:* fast in-process consistency/entailment checks (the RL/verify
  inner loop needs low latency — the embeddability argument bites hardest here);
  proofs (`prove`) as structured, checkable traces.
- *Open:* granularity/latency of per-sample checks at training scale; turning a
  boolean/justification into a differentiable or dense reward; proof-tree ↔
  neural-trace alignment.

### Pattern 4 — Calibrated routing
The completeness contract (`PureEl`/`Horn`/`OutOfFragment` + `completeness_guaranteed`)
routes queries: guaranteed answers taken as final; approximate / out-of-fragment
ones handed to a neural approximator (or flagged for a human / heavier reasoner).
The contract is the neural↔symbolic arbiter.
- *rustdl provides:* the per-query guarantee label (unique among the compared
  reasoners).
- *Open:* calibrate the neural approximator's confidence against the symbolic
  guarantee; decide the routing policy; evaluate end-to-end accuracy vs a
  symbolic-only or neural-only baseline.

## Cross-cutting enablers (what to build to unlock the above)

- **Incremental reasoning** — re-classify / re-check after a single axiom add/remove
  without full recompute. Load-bearing for Patterns 1–3 (per-edit / per-candidate
  loops). Biggest engine investment; currently each check is from-scratch.
- **Richer proof export** — machine-readable proof trees (JSON) for Pattern 3
  training/verification, beyond the current human-oriented `prove`/`report`.
- **Batched / streaming Python API** — check many candidates per call to amortize
  setup (relevant to Pattern 2/3 throughput).
- **Confidence-annotated I/O** — accept/emit per-axiom scores so neural confidence
  and the symbolic guarantee travel together (Pattern 4).

## Prioritization (rough)

1. **Pattern 1 agent prototype** — lowest cost, directly realizes the paper's
   motivation; needs only an agent loop over the existing Python API.
2. **Incremental reasoning** — the enabler that unblocks 1–3; highest leverage,
   highest cost.
3. **Pattern 3 consistency-gate on LLM outputs** — concrete, evaluable, leans on
   the embeddability advantage (fast in-process checks).
4. **Pattern 4 routing** — smallest engine change (contract exists), but needs a
   task + neural baseline to evaluate.
5. **Pattern 2** — reframe as consistency filtering; depends on incremental check.

## Honest caveats

- None of this is built; the paper claims the *reasoner* + the *case*, not a NeSy
  system or NeSy results.
- The strongest near-term differentiators are **soundness** (trustable gate) and
  **embeddability** (fast in-process checks), not raw reasoning speed.
- Pattern 2's "confirm entailed candidates" is weak as stated (entailed ≈ already
  known); the consistency-filtering reframe is the useful version.

# Anywhere (pairwise/double) blocking — results (2026-06-15)

Feature: `RUSTDL_ANYWHERE_BLOCKING` (default OFF). Plan:
`docs/superpowers/plans/2026-06-15-anywhere-pairwise-blocking.md`.
Worktree `agent-aa9e9b0dee52b1c16`, branch `worktree-agent-aa9e9b0dee52b1c16`.

## What changed

`TableauContext::is_blocked` now dispatches on the cached `anywhere_blocking`
flag (`crates/owl-dl-tableau/src/lib.rs`):

- **ancestor-only** (`is_blocked_ancestor`, default) — the historical
  tree-ancestor pair-blocking walk, unchanged.
- **anywhere** (`is_blocked_anywhere`, when the gate is ON) — the candidate
  blocker `x'` ranges over ANY node created strictly before `y`
  (`x'.index() < y.index()`, exactly creation order). Pairwise conditions
  (1)–(4) preserved verbatim (parent-role match, `L(y)⊆L(x')`,
  `L(par(y))⊆L(par(x'))`) plus the `label_sig` prefilter. Candidate
  exclusions: roots/orphans (cond 1), redirected/merged nodes, nominal-labelled
  nodes; a nominal `y` is never blocked.

**Soundness rationale.** The main tableau gates GENERATION only on direct
`is_blocked(node)`; nothing is suspended when an ancestor becomes blocked. So
every non-directly-blocked node is fully expanded ⇒ any `x'` passing (1)–(4) has
a complete label set ⇒ valid blocker. The classic anywhere-blocking bug
(blocking against a node with stale labels via a blocked ancestor) cannot arise
here. Strict creation-order makes the blocking relation acyclic ⇒ termination.

**Phase B index** (`CompletionGraph::block_index: HashMap<Role, Vec<NodeId>>`),
maintained on node create/truncate, gated by `block_index_enabled`. Lets
`is_blocked_anywhere` iterate only the `parent_role` bucket (condition (2) holds
by construction) instead of `0..y.index()`. Required because the family
diagnosis showed the O(N) scan starving the deadline.

## SOUNDNESS GATE — `RUSTDL_ANYWHERE_BLOCKING=1`, `--release`, all GREEN

Closure-diff vs Konclude/HermiT oracle, every fixture **FP=0 / MISSED=0**:

| fixture | closure | FP | MISSED |
|---|---|---|---|
| galen | 27997 | 0 | 0 |
| notgalen | 32739 | 0 | 0 |
| sio | 8904 | 0 | 0 |
| ore-10908-sroiq | 6001 | 0 | 0 |
| ore-15672-shoin | 142 | 0 | 0 |
| wine | 653 | 0 | 0 |
| pizza | 499 | 0 | 0 |
| alehif | 247 | 0 | 0 |
| shoiq-knowledge | 449 | 0 | 0 |
| ro | 158 | 0 | 0 |
| sulo | 51 | 0 | 0 |
| bibtex | 16 | 0 | 0 |

`cargo test --workspace` (gate default-OFF): all green, 0 failed.
fmt + clippy `-D warnings`: clean.

## Non-vacuity: anywhere blocking is genuinely EXERCISED and SOUND

With the DEFAULT trust_sat ON, the corpus classify path answers every pair from
the wedge (its own anywhere blocking) and never reaches the main tableau — so a
gate-ON run is byte-identical to OFF **because the main-tableau code does not
run** (verified: 0 `TableauContext` counter dumps on ore-10908 at default).

To prove the change is not vacuous, ore-10908 (SROIQ: inverse roles + qualified
cardinality — the soundness-sensitive constructs) was run with
`RUSTDL_HYPERTABLEAU_TRUST_SAT=0` (forces main-tableau verification of every
wedge Sat) + `RUSTDL_ANYWHERE_BLOCKING=1` + counters:

- `is_blocked_calls = 929,404,068`, `is_blocked_true = 226,775,965`
  (6881 main-tableau contexts) — anywhere blocking fired 226M times.
- closure 6001 = 6001, **FP=0 / MISSED=0**.

⇒ anywhere blocking, heavily exercised on real SROIQ with inverse roles +
qualified cardinality, is **sound (FP=0) and complete (MISSED=0)**.

## Family termination (acceptance) — boundary documented, soundness-safe

Driving the MAIN tableau consistency path directly on family's 1848-individual
ABox (`PreparedOntology::decide_with_deadline(Top)` — the exact `decide` that
hangs under ancestor-only) with anywhere blocking ON:

- 120 s cap: `outcome = Ok(None)` — clean deadline (DepthLimit + deadline
  reached), **NOT a hang**, **NOT a false `consistent`**.
- counters (20 s): `is_blocked_true ≈ 2.5M` of ~5.2M calls — blocking fires
  heavily; `prefilter_rejects`-per-call ≈ 80 (the role-matched candidate
  count). The dominant single parent-role bucket is itself O(N), so even
  bucket-keyed (Phase B) anywhere blocking is only a constant-factor win and
  cannot bound family's generative nominal ABox.

**Verdict:** family does not converge under anywhere blocking alone — a
documented scoping limitation, matching the task's own note that closing family
end-to-end needs the separate (out-of-scope) wedge role-chain work (and/or a
HermiT-style NI-rule for nominal-driven generation). `Ok(None)` is the
soundness-safe direction (no false `consistent`); the family acceptance test
asserts ONLY soundness (a definite `consistent` fails; a clean deadline passes).

## Default decision (for the controller)

Gate stays **default OFF**. Anywhere blocking is sound + complete where
exercised (proven on ore-10908 main tableau), but:
- under the default trust_sat-ON classify path it is inert (wedge answers all);
- it does not unlock family (the headline large-ABox target).
Its value is on the consistency path / trust_sat-OFF / large-ABox `decide`
callers. Flip default ON only if a workload routes through the main tableau on a
large generative graph AND family-style nominal blowup is not the bottleneck.

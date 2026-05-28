# HF2-double-blocking — focused scoping

Drafted 2026-05-28. Pick-up point for the next principled phase, motivated
by the **SIO finding** (see [`hypertableau-summary.md`](hypertableau-summary.md)
§3 and `bbae964` commit): HF5 `Sat`-trust on SIO produces 38 false
positives, sourced to anywhere-blocking-with-inverses interactions at
scale. Anywhere blocking is known unsound with inverse roles since
Horrocks/Sattler 1999; the SIO measurement is the concrete manifestation
on a real workload. **Double-blocking is the textbook fix.**

The HF2 doc ([`hypertableau-hf2-scoping.md`](hypertableau-hf2-scoping.md))
sketched this at §3; this doc is the **focused implementation plan**
needed to turn it into commits.

## §0 — What's at stake

- *Without double-blocking:* `RUSTDL_HYPERTABLEAU_TRUST_SAT` must stay
  opt-in (and corpus-validated only). Off-corpus, the engine's `Sat`
  verdict can be wrong, and we know this concretely (SIO).
- *With double-blocking:* `Sat`-trust becomes sound *as a calculus
  property*, not just an empirical corpus claim. The flag can
  default-on, and HF5 wiring delivers its 13× win on every
  inverse-bearing ontology, not just the validated corpus.

No corpus payoff (corpus is already 100 % / 0 FP). The win is
**generalization**.

## §1 — The current state, precisely

`is_blocked(n)` in `crates/owl-dl-tableau/src/hyper.rs`:
```rust
fn is_blocked(&self, n: HNode) -> bool {
    let ln = &self.nodes[n.index()];
    for m in &self.nodes {
        if m.order < ln.order && subset_sorted(&ln.labels, &m.labels) {
            return true;
        }
    }
    false
}
```
*Anywhere blocking:* `n` is blocked iff some earlier-created node `m`
has a superset of `n`'s labels. Used to gate `∃` / `≥n` generation
(blocked node → don't generate). Cheap (one linear scan), correct for
SHIQ *without* inverses, **unsound with inverses**.

**Why unsound with inverses (textbook).** The blocking relation
"`n` can be folded into `m`" implicitly claims `n` and `m` are
interchangeable. With inverse roles, the predecessor of `n` sees `n`
through its inverse edge, and the predecessor of `m` sees `m`. If those
*predecessors* have different labels (or are reached by different role
labels), folding `n` into `m` doesn't actually preserve the model —
the predecessor's `∀R⁻.C` consequences might be satisfied at `m` but
not at `n`'s real predecessor, or vice versa.

## §2 — The double-blocking condition (Motik/Shearer/Horrocks 2009 §3.4)

`n` is **double-blocked by** `m` iff *all* of:
1. `L(n) = L(m)`                          *(equal labels — not subset)*
2. `L(parent(n)) = L(parent(m))`          *(equal parent labels)*
3. `edge_role(parent(n) → n) =`
   `edge_role(parent(m) → m)`             *(equal incoming-edge role)*
4. `m.order < n.order`                    *(blocker precedes blocked)*

Equal sets, not subsets. The label-equality is the key strengthening
over anywhere blocking; the parent + edge-role checks are what make
the inverse case sound (predecessors see equivalent neighbourhoods).

For SROIQ with nominals, pair-blocking refinement is needed too (Motik
et al. §3.4 details). Scope cut: **start with the pair-blocking variant
(label + parent label + edge role)**; nominal interaction is HF4-land
and the corpus's nominal usage is already handled by HF4a's NN-rule.

## §3 — Engine changes

### Per-node tracking

Each `HyperNode` needs:
- `parent: Option<HNode>` — the node that created it via `∃`/`≥n`.
- `parent_role: Option<Role>` — the role label of the edge from
  `parent` to `n`.

Both are set in `fire_exists` / `generate_at_least` when the node is
created. The root has `parent = None`. **Already partially present**
via the `preds: Vec<(Role, HNode)>` field — the first pred is the
parent in tree-shaped completions. But making it explicit (and Copy)
is cleaner and avoids the merge-changes-preds edge case.

### Replace `is_blocked`

```rust
fn is_blocked(&self, n: HNode) -> bool {
    let ln = &self.nodes[n.index()];
    let Some(np) = ln.parent else { return false; };  // root never blocked
    let pn = &self.nodes[np.index()];
    let nr = ln.parent_role.expect("non-root has parent_role");
    for m in &self.nodes {
        if m.order >= ln.order { continue; }
        let Some(mp) = m.parent else { continue; };
        let mp_node = &self.nodes[mp.index()];
        if ln.labels == m.labels
            && pn.labels == mp_node.labels
            && nr == m.parent_role.expect("non-root has parent_role")
        {
            return true;
        }
    }
    false
}
```

**Cost.** `n × n` scan, with three slice comparisons per pair. The
label-equality (vs anywhere's subset) is a tighter filter; fewer
matches. Need to measure that the tighter filter doesn't cause
**more nodes** before blocking kicks in — termination still holds,
but graph size could grow.

### Termination considerations

Anywhere blocking with subset-superset terminates fast because *any*
extension is a blocker. Double-blocking with `=` is stricter: only
*equal* label sets block. Cyclic ontologies that previously blocked
quickly may now generate more nodes before a true blocker appears.

**Mitigation:** the engine's existing `FIXPOINT_ITERS` defensive cap
(100 000) catches runaway generation, returning `Stalled`. Won't crash;
will defer to the tableau or surface as a hyper-classify-probe stall.
Real ontologies (per Motik et al. survey) have tractable
double-blocking depth — at most ~tens of nodes per `∃`-chain.

## §4 — Canary

The crafted canary: a small inverse-bearing ontology where anywhere
blocking gives a *false `Sat`* verdict and double-blocking corrects it
to `Unsat`. Construction is non-trivial; the SIO finding gave us the
shape but not a minimal example.

**Two paths to a canary:**

1. *Textbook* — Horrocks/Sattler 1999 or Motik 2009 §3.4 provides
   small examples. Transcribe one verbatim.
2. *Axiom-bisection on SIO* — start with the full SIO axiom set
   producing one of the 38 FPs, halve, see if the FP persists, repeat.
   Slow (each halving needs a classify run) but yields a real,
   automated repro.

Either canary is the gate: it fails today (engine gives wrong `Sat`)
and must pass after double-blocking lands. **Add as `#[ignore]`d
engine test first**, un-ignore when the implementation is in place.

## §5 — Validation gates (in order)

1. **The crafted canary passes** (under-pin: at least one inverse-
   blocking shape is correctly handled).
2. **Every existing engine test stays green.** The 86 hand-built tests
   are the regression net for soundness/termination basics. A
   double-blocking bug surfaces here first.
3. **Corpus stays at 100 % / 0 FP.** Pizza/ro/sulo are still the
   acceptance criterion. The slightly-tighter blocking shouldn't
   change verdicts here (corpus is inverse-inert in the relevant
   sense), but it might change *timing* — pizza probe wall could
   shift; the regression test budget (90 s) absorbs ±2× drift.
4. **SIO FP count drops.** The 38 FPs (or some measurable subset)
   should disappear. The other path — they don't — would be a real
   finding too: more than blocking-with-inverses is involved.
5. **Optional: SIO completes faster with trust-Sat.** A bonus, not a
   requirement.

## §6 — Risk

- *Soundness bugs in the blocking condition itself.* Off-by-one in the
  parent/edge-role check is the failure mode that turns sound `Unsat`
  into false `Unsat` (i.e., wrongly claims subsumption). The corpus
  diff caught backjumping's analogous bug; same net applies here.
- *Termination regressions.* Cyclic ontologies generating more nodes;
  the `FIXPOINT_ITERS` cap is the safety valve but a real regression
  needs measurement.
- *Merge interaction.* The `≤n` merge can fuse nodes; the merged
  node's `parent`/`parent_role` need a well-defined value (probably
  the union-find representative's). One more place where the "merge
  doesn't redirect in-edges" gap (still open from HF2 / HF3) might
  bite — but the corpus doesn't exercise the relevant interactions.
- *The canary is hard to construct.* Spending a turn on bisection vs
  textbook transcription is a real time choice.

## §7 — Effort estimate

- *Canary + engine test scaffold:* 1 commit.
- *Per-node `parent`/`parent_role` plumbing:* 1 commit, low risk.
- *Replace `is_blocked`:* 1 commit, **high risk** — this is where
  soundness regressions live. Land behind a runtime flag
  (`RUSTDL_HYPER_DOUBLE_BLOCK`?) initially so the existing tests
  exercise both blocking modes during transition.
- *Validation sweep:* 1 commit (corpus regression check + SIO measure).
- *Flag flip (default-on double-blocking; default-on
  `RUSTDL_HYPERTABLEAU_TRUST_SAT`?):* 1 commit, gated by SIO FP
  reduction.

Realistic total: a week of careful work, not months — *if* the canary
construction goes smoothly. If SIO bisection is required, add a week.

## §8 — What this turn delivers

This scoping doc + a **failing canary placeholder** (`#[ignore]`d
engine test that pins the target). The actual blocking-condition swap
is the next commit's work, not this one — keeping the disciplined
pattern: scope, gate, then build.

## §9 — Pointers

- Current `is_blocked` impl: `crates/owl-dl-tableau/src/hyper.rs:462`.
- `fire_exists` / `generate_at_least` (set `parent`/`parent_role` here):
  same file, search for `new_node()`.
- HF2 master doc: [`hypertableau-hf2-scoping.md`](hypertableau-hf2-scoping.md)
  §3, §4.
- The SIO finding (motivation): [`hypertableau-summary.md`](hypertableau-summary.md)
  §3 and `bbae964` commit message.
- Dead-end #11 (why this can't ship default-on without double-blocking):
  [`hypertableau-dead-ends.md`](hypertableau-dead-ends.md).

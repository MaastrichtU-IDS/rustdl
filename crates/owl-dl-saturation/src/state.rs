//! [`SaturationState`] — a saturation engine that survives across revisions.
//!
//! [`crate::saturate`] builds a `WorklistEngine`, runs it to quiescence and
//! drops it. An incremental session cannot afford that: every revision would
//! re-derive the whole closure. `SaturationState` keeps the engine alive so a
//! **monotone addition** can resume saturation where the previous revision
//! stopped.
//!
//! # P1 is addition-only
//!
//! [`SaturationState::apply_additions`] either splices the new axioms' rules
//! into the live engine, or gives up and rebuilds from scratch — reported by
//! [`DeltaOutcome::rebuilt`], which the evaluation reads as the reuse rate.
//! It rebuilds when:
//!
//! 1. the delta contains an **object-property axiom**. `role_super` is frozen
//!    when the engine is built and is consulted (as a dense `Vec` and a bitset)
//!    by every existential rule, so a role-hierarchy change invalidates every
//!    context. ELK has the same documented limitation — `SubObjectPropertyOf`,
//!    `EquivalentObjectProperties`, `TransitiveObjectProperty` and
//!    `ReflexiveObjectProperty` all trigger full re-classification there. The IR
//!    has no data-property axiom variants (`owl_dl_core::data_axioms` lowers
//!    those to class axioms before saturation ever sees them), so the
//!    object-property check is the whole of this rule.
//! 2. the new named classes would not fit under `synth_base`, i.e. the reserved
//!    slack gap is exhausted. Rebuilds with doubled slack.
//! 3. re-lowering the union changed something the incremental path cannot
//!    splice — see [`SaturationState::apply_additions`]'s compatibility gate.
//!
//! # Why the whole union is re-lowered
//!
//! The brief's sketch was "run `collect_el_rules` restricted to the new
//! indices". That is not safe: `collect_el_rules`' first pass collects
//! whole-ontology metadata (`role_ranges` above all) that its second pass folds
//! into the *new* axioms' lowering. Restricted to the delta, `role_ranges`
//! would come back empty and `A ⊑ ∃r.B` would compile without the range
//! constraint the from-scratch run folds in — a silent incompleteness. So the
//! union is lowered in full, against a **pre-seeded** allocator
//! ([`crate::collect_el_rules_seeded`]), and the rules that are genuinely new
//! are recovered by a multiset diff against the previous revision's compile.
//!
//! # Soundness
//!
//! Every rule the engine retains from the previous revision was sound with
//! respect to the previous axiom set, which is a subset of this one; entailment
//! is monotone, so it is still sound. Nothing is invalidated, and nothing needs
//! to be: a pure addition can only add consequences. The risk this module
//! actually runs is the *other* direction — a spliced rule that never fires
//! against facts already derived, i.e. an incompleteness — which is why
//! `retrigger` fires each new rule against the existing closure explicitly.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;

use owl_dl_core::{Axiom, ClassId, InternalOntology, RoleId};

use crate::{
    AtomicSubsumption, ConjunctiveTrigger, ElRules, ExistentialFact, ExistentialTrigger, NO_AXIOM,
    Subsumers, WorklistEngine, build_role_super, collect_el_rules, collect_el_rules_seeded,
    freeze_role_super,
};

/// What one [`SaturationState::apply_additions`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaOutcome {
    /// True iff the engine was thrown away and rebuilt from scratch.
    pub rebuilt: bool,
    /// Distinct classes this call revisited: every class the splice enqueued a
    /// consequence for, **plus** every class that gained a subsumer, an
    /// existential fact, or an unsatisfiability flag while the fixpoint
    /// drained. It therefore measures the resumed run's real work, not just
    /// the splice's opening move.
    ///
    /// The universe is `WorklistEngine::num_total_classes` — user classes,
    /// reserved slack gap and synthetics alike — because the splice marks
    /// synthetic ids too. On a rebuild the value is exactly that total: every
    /// context was derived from nothing. Both arms are in the same units, so
    /// `1 - marked_contexts / total` is a reuse rate that does not overstate.
    pub marked_contexts: usize,
}

/// A saturation engine plus everything needed to resume it on the next
/// revision. See the module docs.
pub struct SaturationState {
    engine: WorklistEngine,
    /// Reserved class-id headroom above the user vocabulary. New named classes
    /// are interned into `[num_user_classes, synth_base)`.
    slack: usize,
    /// First synthetic class id — `num_user_classes at build time + slack`.
    /// Never moves without a rebuild: the engine's synthetics live above it.
    synth_base: usize,
    /// The static compile the engine was built from — the left-hand side of the
    /// next revision's rule diff. NOT the engine's live `rules`, which also
    /// carries the runtime synthetics saturation minted.
    static_rules: ElRules,
    /// The frozen role hierarchy. A delta that changes it forces a rebuild.
    role_super_map: HashMap<RoleId, HashSet<RoleId>>,
}

impl SaturationState {
    /// Saturate `internal` from scratch, keeping the engine alive.
    ///
    /// `slack` reserves that many unallocated class ids above the user
    /// vocabulary so a later revision can intern new named classes without
    /// moving the synthetic universe. See `SaturateConfig::slack`.
    ///
    /// # Panics
    /// Panics if `num_classes + slack` overflows `usize`.
    #[must_use]
    pub fn build(internal: &InternalOntology, slack: usize) -> Self {
        let n = internal.vocabulary.num_classes();
        let synth_base = n
            .checked_add(slack)
            .expect("num_classes + slack overflows usize");
        let role_super_map = build_role_super(internal);
        let (rules, tseitin, num_total_classes) =
            collect_el_rules(internal, &role_super_map, synth_base);
        let static_rules = rules.clone();
        let role_super = freeze_role_super(&role_super_map);
        let mut engine = WorklistEngine::new(
            n,
            synth_base,
            num_total_classes,
            rules,
            tseitin,
            role_super,
            false,
            None,
        );
        engine.seed(internal);
        engine.run();
        Self {
            engine,
            slack,
            synth_base,
            static_rules,
            role_super_map,
        }
    }

    /// The closure as it stands. Identical, projected onto the user
    /// vocabulary, to [`crate::saturate`] on the current revision.
    #[must_use]
    pub fn subsumers(&self) -> &Subsumers {
        &self.engine.subsumers
    }

    /// The reserved id headroom this state currently carries. Grows on a
    /// slack-exhaustion rebuild.
    #[must_use]
    pub fn slack(&self) -> usize {
        self.slack
    }

    /// Absorb a **monotone addition**: `internal` is the post-delta ontology and
    /// `added` indexes the axioms it gained.
    ///
    /// Resumes the retained engine when it can, rebuilds when it must — see the
    /// module docs for the three rebuild triggers plus the compatibility gate
    /// below.
    ///
    /// **Addition-only.** A retraction is not expressible here: saturation does
    /// not consult `InternalOntology::live` at all, so clearing a live bit
    /// changes neither this path nor a from-scratch [`crate::saturate`]. P2 owns
    /// deletion; do not route one through this method expecting it to notice.
    pub fn apply_additions(
        &mut self,
        internal: &InternalOntology,
        added: &[usize],
    ) -> DeltaOutcome {
        // (1) Role-hierarchy freeze. `role_super` is baked into the engine as a
        //     dense Vec AND a bitset, and every existential/chain rule reads it.
        //     An index the caller cannot back with an axiom is treated the same
        //     way: we cannot rule out that it named a property axiom.
        if added
            .iter()
            .any(|&i| internal.axioms.get(i).is_none_or(is_property_axiom))
        {
            return self.rebuild(internal, self.slack);
        }
        // (2) Slack exhaustion. Ids in [num_user_classes, synth_base) are the
        //     only room new named classes can occupy; above synth_base they
        //     would alias a synthetic.
        let n = internal.vocabulary.num_classes();
        if n > self.synth_base {
            return self.rebuild(internal, self.slack.saturating_mul(2).max(1));
        }
        // A new role can appear without any property axiom (`∃newRole.C`), and
        // `role_super`/`role_super_bitset` are sized to the OLD role count, so
        // `is_sub_role(newRole, newRole)` would answer `false` — every rule on
        // the new role would silently stop firing. Compare the whole closure:
        // it also catches a hierarchy change smuggled in some other way.
        let role_super_map = build_role_super(internal);
        if role_super_map != self.role_super_map {
            return self.rebuild(internal, self.slack);
        }

        // Re-lower the union against the engine's LIVE allocator, so already
        // introduced bodies come back with their existing synthetic ids and
        // anything new is allocated above every runtime synthetic.
        let (u_rules, u_alloc, u_total) = collect_el_rules_seeded(
            internal,
            &role_super_map,
            self.engine.tseitin_runtime.clone(),
        );

        // (3) Compatibility gate: everything the incremental path cannot splice
        //     must be untouched by the delta.
        let Some(delta) = rule_delta(&self.static_rules, &u_rules) else {
            return self.rebuild(internal, self.slack);
        };

        let old_user_classes = self.engine.num_user_classes;
        let old_total = self.engine.num_total_classes;
        let old_tops = std::mem::replace(
            &mut self.engine.rules.top_subsumers,
            u_rules.top_subsumers.clone(),
        );
        self.grow_to(u_total.max(old_total), &u_alloc);
        self.engine.tseitin_runtime = u_alloc;
        self.engine.num_user_classes = n;

        let span = DeltaSpan {
            old_user_classes,
            new_user_classes: n,
            old_total,
            new_total: u_total,
            old_tops,
        };
        // Hand the splice's marks to the engine and let the drain keep
        // extending them, so `marked_contexts` covers the fixpoint too.
        let marked = self.splice_and_retrigger(&delta, &span);
        self.engine.touched_contexts = Some(marked);
        self.engine.run();
        let marked_contexts = self
            .engine
            .touched_contexts
            .take()
            .map_or(0, |touched| touched.len());
        self.static_rules = u_rules;
        DeltaOutcome {
            rebuilt: false,
            marked_contexts,
        }
    }

    /// Throw the engine away and saturate `internal` from scratch.
    ///
    /// Every context is derived from nothing, so `marked_contexts` is the whole
    /// class universe — the same universe the incremental arm counts in (see
    /// [`DeltaOutcome::marked_contexts`]); reporting `num_classes()` here would
    /// leave the two arms in different units and flatter the reuse rate.
    fn rebuild(&mut self, internal: &InternalOntology, slack: usize) -> DeltaOutcome {
        *self = Self::build(internal, slack);
        DeltaOutcome {
            rebuilt: true,
            marked_contexts: self.engine.num_total_classes,
        }
    }

    /// Widen every per-class Vec/bitset to `needed`, mirroring the growth path
    /// in `WorklistEngine::introduce_runtime_synthetic`, and register the
    /// atomic content of the newly allocated Tseitin conjunctions exactly as
    /// `WorklistEngine::new` does for the static ones.
    fn grow_to(&mut self, needed: usize, alloc: &crate::TseitinAllocator) {
        let e = &mut self.engine;
        if needed > e.num_total_classes {
            e.subsumers.subsumers.grow_to(needed);
            e.subsumers.unsatisfiable.grow(needed);
            e.subsumed_by.grow_to(needed);
            while e.facts_by_sub.len() < needed {
                e.facts_by_sub.push(Vec::new());
            }
            while e.facts_by_target.len() < needed {
                e.facts_by_target.push(Vec::new());
            }
            while e.conjunctive_by_body.len() < needed {
                e.conjunctive_by_body.push(Vec::new());
            }
            while e.existential_triggers_by_body.len() < needed {
                e.existential_triggers_by_body.push(Vec::new());
            }
            while e.disjoints_by_class.len() < needed {
                e.disjoints_by_class.push(Vec::new());
            }
            // A body that was already introduced is memoized, so `by_body`
            // can only have grown when `next_id` did — i.e. inside this guard.
            // Walking the whole map unconditionally would cost O(|bodies|) on
            // every revision, which is exactly what this path exists to avoid.
            for (body, &synthetic) in &alloc.by_body {
                e.atomic_content_of
                    .entry(synthetic)
                    .or_insert_with(|| body.iter().copied().collect());
            }
            e.num_total_classes = needed;
        }
    }

    /// Install the new rules, index them, seed the new classes and fire every
    /// new rule against the closure that already exists. Returns the classes
    /// marked so far; the caller hands the set to the engine so the fixpoint
    /// keeps extending it.
    ///
    /// Firing has to be explicit: `process_subsumer` returns immediately when
    /// `record_subsumer` reports the edge was already known, so re-pushing an
    /// existing `(C, D)` does NOT re-run the rules keyed on `D`.
    #[allow(clippy::too_many_lines)]
    fn splice_and_retrigger(&mut self, delta: &RuleDelta, span: &DeltaSpan) -> HashSet<ClassId> {
        let mut marked: HashSet<ClassId> = HashSet::new();

        // --- Reflexivity for the ids that did not exist before. Mirrors the
        //     two loops at the top of `WorklistEngine::seed`; the slack gap
        //     [new_user_classes, synth_base) stays unallocated and unseeded.
        for i in span.old_user_classes..span.new_user_classes {
            let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            self.engine.todo_subsumer.push_back((id, id));
            marked.insert(id);
        }
        for i in span.old_total.max(self.synth_base)..span.new_total {
            let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            self.engine.todo_subsumer.push_back((id, id));
            marked.insert(id);
        }
        // `⊤ ⊑ C` broadcast. `seed` sends every top subsumer to every named
        // class, so BOTH halves of the cross product need replaying: a top
        // subsumer the delta introduced has to reach the classes that were
        // already there, and a class the delta interned has to receive the
        // ones that were already broadcast.
        let tops = self.engine.rules.top_subsumers.clone();
        for i in 0..span.new_user_classes {
            let x = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            let fresh_class = i >= span.old_user_classes;
            for &c in &tops {
                if fresh_class || !span.old_tops.contains(&c) {
                    self.engine.todo_subsumer.push_back((x, c));
                    marked.insert(x);
                }
            }
        }

        // --- Told atomic subsumptions.
        for &(rule, ax) in &delta.atomic_subs {
            self.engine.rules.atomic_subsumptions.push(rule);
            self.engine.rules.axiom_of_atomic_sub.push(ax);
            self.engine.todo_subsumer.push_back((rule.sub, rule.sup));
            marked.insert(rule.sub);
        }

        // --- Told existential facts.
        for (fact, ax) in &delta.exist_facts {
            self.engine.rules.existential_facts.push(*fact);
            self.engine.rules.axiom_of_existential_fact.push(*ax);
            self.engine.push_fact(*fact);
            marked.insert(fact.sub);
        }

        // --- Told `C ⊑ ⊥`.
        for &(c, ax) in &delta.unsat {
            self.engine.rules.directly_unsat.push(c);
            self.engine.rules.axiom_of_directly_unsat.push(ax);
            self.engine.enqueue_unsat(c);
            marked.insert(c);
        }

        // --- Conjunctive triggers: index by body, then fire on every class
        //     that already carries the whole body.
        for (trigger, ax) in &delta.conjunctive {
            let idx = self.engine.rules.conjunctive_triggers.len();
            self.engine.rules.conjunctive_triggers.push(trigger.clone());
            self.engine.rules.axiom_of_conjunctive_trigger.push(*ax);
            for &b in &trigger.bodies {
                self.engine.conjunctive_by_body[b.index() as usize].push(idx);
            }
            let Some(&first) = trigger.bodies.first() else {
                continue;
            };
            for c in self.engine.subs_of_class(first) {
                if trigger
                    .bodies
                    .iter()
                    .all(|b| self.engine.subsumers.contains(c, *b))
                {
                    self.engine.enqueue_subsumer(c, trigger.head);
                    marked.insert(c);
                }
            }
        }

        // --- Existential triggers: index by body, then re-run every existing
        //     fact whose target already carries the body. `process_fact` is
        //     idempotent (every effect goes through `enqueue_*`/`push_fact`,
        //     all of which dedup), so replaying a fact index is safe.
        for (trigger, ax) in &delta.exist_triggers {
            let idx = self.engine.rules.existential_triggers.len();
            self.engine.rules.existential_triggers.push(*trigger);
            self.engine.rules.axiom_of_existential_trigger.push(*ax);
            self.engine.existential_triggers_by_body[trigger.body.index() as usize].push(idx);
            for t in self.engine.subs_of_class(trigger.body) {
                for fidx in self.engine.facts_by_target[t.index() as usize].clone() {
                    let fact = self.engine.facts[fidx];
                    if self.engine.is_sub_role(fact.role, trigger.role) {
                        self.engine.todo_fact.push_back(fidx);
                        marked.insert(fact.sub);
                    }
                }
            }
        }

        // --- Disjoint pairs: index both ways, then clash-check every class
        //     that already has both sides among its subsumers.
        for &((a, b), ax) in &delta.disjoint {
            self.engine.rules.disjoint_pairs.push((a, b));
            self.engine.rules.axiom_of_disjoint_pair.push(ax);
            self.engine.disjoints_by_class[a.index() as usize].push(b);
            self.engine.disjoints_by_class[b.index() as usize].push(a);
            for c in self.engine.subs_of_class(a) {
                if self.engine.subsumers.contains(c, b) {
                    self.engine.enqueue_unsat(c);
                    marked.insert(c);
                }
            }
        }

        // T5's invariant: every rule vector must stay exactly as long as its
        // `axiom_of_*` twin. The pushes above bypass the `push_*` helpers
        // (they carry provenance forward from the union compile rather than
        // from a `current_axiom` cursor), so check the parity explicitly.
        self.engine.rules.debug_assert_provenance_parity();
        marked
    }
}

/// The id ranges and top-subsumer set that changed across one revision — what
/// `splice_and_retrigger` needs to replay the parts of `WorklistEngine::seed`
/// that only apply to what is new.
struct DeltaSpan {
    old_user_classes: usize,
    new_user_classes: usize,
    old_total: usize,
    new_total: usize,
    old_tops: Vec<ClassId>,
}

/// The rules one revision gained over the previous one, each paired with the
/// source-axiom index the union compile attributed it to.
#[derive(Default)]
struct RuleDelta {
    atomic_subs: Vec<(AtomicSubsumption, u32)>,
    conjunctive: Vec<(ConjunctiveTrigger, u32)>,
    exist_facts: Vec<(ExistentialFact, u32)>,
    exist_triggers: Vec<(ExistentialTrigger, u32)>,
    disjoint: Vec<((ClassId, ClassId), u32)>,
    unsat: Vec<(ClassId, u32)>,
}

/// The rules `new` has that `base` did not — or `None` when the two compiles
/// are not splice-compatible and the caller must rebuild.
///
/// # Why a multiset diff and not a suffix
///
/// `collect_el_rules` pushes into the same vectors from several passes, so the
/// union's vectors are `[pass1(base), pass1(delta), pass2(base), …]` — the
/// previous revision's rules are interleaved, not a prefix. Comparing by
/// content is the only correct reading. It is also exact: the allocator was
/// pre-seeded, so re-lowering an unchanged axiom yields byte-identical rules
/// over identical synthetic ids, and they cancel.
///
/// # The leftover check
///
/// Rules `base` has and `new` does not are expected — and only expected — for
/// Tseitin definitional clauses, which are emitted once per body and skipped on
/// the memo hit. Those carry [`NO_AXIOM`]. A leftover with a *real* source
/// axiom means the union changed how an existing axiom lowers (a new
/// `ObjectPropertyRange` folding into an existing `∃r.B`, say). The engine
/// would then keep a rule the from-scratch run does not have; sound, but no
/// longer identical. Rebuild instead.
///
/// The structural maps below are not rule vectors and cannot be spliced, so
/// they must match exactly. `top_subsumers` is the one exception: it only feeds
/// the seed-time broadcast, which `splice_and_retrigger` replays.
fn rule_delta(base: &ElRules, new: &ElRules) -> Option<RuleDelta> {
    if !structurally_compatible(base, new) {
        return None;
    }
    // A new disjoint pair changes which disjuncts SP-B1 excludes for classes
    // that gain no new subsumer, and nothing re-examines them. Only reachable
    // on an ontology that has atomic disjunctions at all.
    let disjoint = added_with_provenance(
        &base.disjoint_pairs,
        &base.axiom_of_disjoint_pair,
        &new.disjoint_pairs,
        &new.axiom_of_disjoint_pair,
        |&(a, b)| (a, b),
    )?;
    if !disjoint.is_empty() && !new.disjunctions_by_class.is_empty() {
        return None;
    }
    Some(RuleDelta {
        atomic_subs: added_with_provenance(
            &base.atomic_subsumptions,
            &base.axiom_of_atomic_sub,
            &new.atomic_subsumptions,
            &new.axiom_of_atomic_sub,
            |r| (r.sub, r.sup),
        )?,
        conjunctive: added_with_provenance(
            &base.conjunctive_triggers,
            &base.axiom_of_conjunctive_trigger,
            &new.conjunctive_triggers,
            &new.axiom_of_conjunctive_trigger,
            |r| (r.bodies.clone(), r.head),
        )?,
        exist_facts: added_with_provenance(
            &base.existential_facts,
            &base.axiom_of_existential_fact,
            &new.existential_facts,
            &new.axiom_of_existential_fact,
            |r| (r.sub, r.role, r.target),
        )?,
        exist_triggers: added_with_provenance(
            &base.existential_triggers,
            &base.axiom_of_existential_trigger,
            &new.existential_triggers,
            &new.axiom_of_existential_trigger,
            |r| (r.role, r.body, r.head),
        )?,
        disjoint,
        unsat: added_with_provenance(
            &base.directly_unsat,
            &base.axiom_of_directly_unsat,
            &new.directly_unsat,
            &new.axiom_of_directly_unsat,
            |c| *c,
        )?,
    })
}

/// Every `ElRules` field that is NOT one of the six spliceable rule vectors.
/// These are consulted directly by the worklist rules and have no incremental
/// re-trigger path, so a delta that touches one forces a rebuild.
fn structurally_compatible(base: &ElRules, new: &ElRules) -> bool {
    // EXHAUSTIVE DESTRUCTURE, NO `..` — deliberate. The claim above is
    // "everything that is not one of the six spliceable vectors is checked
    // here", and nothing but the compiler can hold a claim like that. Add a
    // twelfth structural field to `ElRules` with a `..` in this pattern and it
    // is silently un-gated: the engine keeps the previous revision's value and
    // the spliced closure quietly diverges from from-scratch. With no `..`, the
    // same mistake is a build error. Bind the spliceable vectors and their
    // provenance twins to `_`; they are handled by `rule_delta`.
    let ElRules {
        // --- spliceable: diffed and installed by `rule_delta` ---
        atomic_subsumptions: _,
        conjunctive_triggers: _,
        existential_facts: _,
        existential_triggers: _,
        disjoint_pairs: _,
        directly_unsat: _,
        axiom_of_atomic_sub: _,
        axiom_of_conjunctive_trigger: _,
        axiom_of_existential_fact: _,
        axiom_of_existential_trigger: _,
        axiom_of_disjoint_pair: _,
        axiom_of_directly_unsat: _,
        // --- lowering-time cursor, never read after collection ---
        current_axiom: _,
        // --- structural: must match, or rebuild ---
        top_subsumers: b_tops,
        abox_nominal_reach: b_abox,
        forall_key_targets: b_forall,
        nominal_to_ind: b_nominal,
        max1_key_by_role: b_max1_key,
        max1_role_by_key: b_max1_role,
        role_domains: b_domains,
        role_ranges: b_ranges,
        chain_axioms: b_chains,
        functional_roles: b_func,
        functional_supers_of: b_func_supers,
        disjunctions_by_class: b_disj,
    } = base;
    let ElRules {
        atomic_subsumptions: _,
        conjunctive_triggers: _,
        existential_facts: _,
        existential_triggers: _,
        disjoint_pairs: _,
        directly_unsat: _,
        axiom_of_atomic_sub: _,
        axiom_of_conjunctive_trigger: _,
        axiom_of_existential_fact: _,
        axiom_of_existential_trigger: _,
        axiom_of_disjoint_pair: _,
        axiom_of_directly_unsat: _,
        current_axiom: _,
        top_subsumers: n_tops,
        abox_nominal_reach: n_abox,
        forall_key_targets: n_forall,
        nominal_to_ind: n_nominal,
        max1_key_by_role: n_max1_key,
        max1_role_by_key: n_max1_role,
        role_domains: n_domains,
        role_ranges: n_ranges,
        chain_axioms: n_chains,
        functional_roles: n_func,
        functional_supers_of: n_func_supers,
        disjunctions_by_class: n_disj,
    } = new;

    norm(b_domains) == norm(n_domains)
        && norm(b_ranges) == norm(n_ranges)
        && b_chains == n_chains
        && b_func == n_func
        && b_func_supers == n_func_supers
        && b_disj == n_disj
        && norm(b_abox) == norm(n_abox)
        && norm(b_forall) == norm(n_forall)
        && b_nominal == n_nominal
        && b_max1_key == n_max1_key
        && b_max1_role == n_max1_role
        // The broadcast list may only grow; a shrink means an axiom re-lowered
        // differently, which is a rebuild.
        && b_tops.iter().all(|c| n_tops.contains(c))
}

/// Order-insensitive view of a map-of-list field, so an incidental difference
/// in insertion order is not mistaken for a semantic change.
fn norm<K: Ord + Copy, V: Ord + Copy>(m: &HashMap<K, Vec<V>>) -> BTreeMap<K, Vec<V>> {
    m.iter()
        .map(|(k, v)| {
            let mut v = v.clone();
            v.sort_unstable();
            (*k, v)
        })
        .collect()
}

/// Multiset difference `new - base`, carrying each added rule's provenance.
/// `None` when a `base` entry with a real source axiom has no counterpart in
/// `new` — see [`rule_delta`].
fn added_with_provenance<T: Clone, K: Eq + Hash, F: Fn(&T) -> K>(
    base: &[T],
    base_ax: &[u32],
    new: &[T],
    new_ax: &[u32],
    key: F,
) -> Option<Vec<(T, u32)>> {
    debug_assert_eq!(base.len(), base_ax.len(), "base provenance parity");
    debug_assert_eq!(new.len(), new_ax.len(), "new provenance parity");
    let mut budget: HashMap<K, Vec<u32>> = HashMap::new();
    for (rule, &ax) in base.iter().zip(base_ax) {
        budget.entry(key(rule)).or_default().push(ax);
    }
    let mut out = Vec::new();
    for (rule, &ax) in new.iter().zip(new_ax) {
        match budget.get_mut(&key(rule)) {
            Some(seen) if !seen.is_empty() => {
                seen.pop();
            }
            _ => out.push((rule.clone(), ax)),
        }
    }
    if budget.values().flatten().any(|&ax| ax != NO_AXIOM) {
        return None;
    }
    Some(out)
}

/// True for the `RBox` axioms that can move the frozen role hierarchy or the
/// role-keyed rule metadata. The IR has no data-property variants — the data
/// preprocessing pass lowers those to class axioms upstream of saturation.
fn is_property_axiom(ax: &Axiom) -> bool {
    matches!(
        ax,
        Axiom::SubObjectPropertyOf { .. }
            | Axiom::EquivalentObjectProperties(_)
            | Axiom::DisjointObjectProperties(_)
            | Axiom::InverseObjectProperties(..)
            | Axiom::ObjectPropertyDomain { .. }
            | Axiom::ObjectPropertyRange { .. }
            | Axiom::TransitiveRole(_)
            | Axiom::SymmetricRole(_)
            | Axiom::AsymmetricRole(_)
            | Axiom::ReflexiveRole(_)
            | Axiom::IrreflexiveRole(_)
            | Axiom::FunctionalRole(_)
            | Axiom::InverseFunctionalRole(_)
            | Axiom::DeclareObjectProperty(_)
    )
}

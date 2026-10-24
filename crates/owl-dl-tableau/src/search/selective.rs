//! Selective backjumping (#288 step 4c, `RUSTDL_SELECTIVE_UNDO=1`).
//!
//! When a clash skips frames, chronological backjumping rolls back everything
//! above the frame T that catches it, and the search then rebuilds the skipped
//! frames' independent work. Here only what depends on the abandoned decisions
//! (T's option and the frames whose preconditions it removed) is undone; the
//! other frames above T stay, and T, re-numbered T′, moves on top of them to
//! try its next option. Plan: `plan-4.md` §4c and its revisions.

use super::{Frame, FrameKind, Record, trace_enabled};
use crate::TableauContext;
use crate::deps::meets;
use crate::graph::{DepSet, NodeId};
use crate::trail::{Checkpoint, EntryAction, TableauTrail, TrailEntry};
use owl_dl_core::ConceptId;
use std::collections::{HashMap, HashSet};

/// A frame on the search stack, with the option it is trying and that
/// option's checkpoint.
pub(super) type Stacked = (Frame, usize, Checkpoint);

/// The stack index of the frame T that should catch `Unsat(deps)` selectively,
/// or `None` to hand the verdict on chronologically: the flag is off, the top
/// frame is in `deps` (nothing is skipped), no frame is, or fewer frames would
/// be skipped than [`TableauContext::selective_min_skip`].
pub(super) fn target(
    ctx: &TableauContext<'_, '_, '_>,
    stack: &[Stacked],
    deps: &DepSet,
) -> Option<usize> {
    if !ctx.selective_undo() {
        return None;
    }
    let (top, _, _) = stack.last()?;
    if deps.binary_search(&top.my_id).is_ok() {
        return None;
    }
    // The stack is sorted by id (plan 4c S-1), so this is also `max(deps)`.
    let t = stack
        .iter()
        .rposition(|(f, _, _)| deps.binary_search(&f.my_id).is_ok())?;
    let skipped = stack.len() - 1 - t;
    if skipped < ctx.selective_min_skip() {
        return None;
    }
    // Nothing worth keeping (every frame above T rests on T or on a frame
    // that does): chronological unwinding gets the same state for less.
    let mut gone: DepSet = stack[t..].iter().map(|(f, _, _)| f.my_id).collect();
    gone.sort_unstable();
    let any_kept = stack[t + 1..].iter().any(|(f, _, _)| {
        let keep = !meets(&f.pre, &gone);
        if keep && let Ok(pos) = gone.binary_search(&f.my_id) {
            gone.remove(pos);
        }
        keep
    });
    any_kept.then_some(t)
}

/// Whether to run oracle (c)'s shadow search before this selective backjump:
/// `RUSTDL_SELECTIVE_SHADOW=1`, for at most `RUSTDL_SELECTIVE_SHADOW_MAX`
/// (default 8) backjumps per process, since each shadow re-runs the rest of
/// the search.
pub(super) fn shadow_wanted() -> bool {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static MAX: OnceLock<usize> = OnceLock::new();
    static USED: AtomicUsize = AtomicUsize::new(0);
    let max = *MAX.get_or_init(|| {
        if std::env::var_os("RUSTDL_SELECTIVE_SHADOW").is_some_and(|v| v == "1") {
            std::env::var("RUSTDL_SELECTIVE_SHADOW_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8)
        } else {
            0
        }
    });
    max > 0 && USED.fetch_add(1, Ordering::Relaxed) < max
}

/// What one selective backjump did, for the trace.
#[derive(Default)]
struct Stats {
    kept: usize,
    dead: usize,
    undone: usize,
    kept_entries: usize,
    recopied: usize,
    deleted_records: usize,
}

/// Catch `Unsat(deps)` at `stack[t]` selectively. Pops T and every frame above
/// it, undoes what depends on T's option and on the frames that do not
/// survive, pushes the surviving frames back in their order, and returns T
/// re-numbered (T′) with its refuted option recorded. The caller runs T′ next,
/// so its verdict goes to the top surviving frame.
pub(super) fn backjump(
    ctx: &mut TableauContext<'_, '_, '_>,
    stack: &mut Vec<Stacked>,
    t: usize,
    deps: DepSet,
) -> Frame {
    debug_assert!(
        deps.iter()
            .all(|x| ctx.active_branches().binary_search(x).is_ok()),
        "selective backjump: clash deps {deps:?} name an inactive decision"
    );
    let mut above = stack.split_off(t);
    let rest = above.split_off(1);
    let (mut target, k_t, cp_t) = above.pop().expect("T is on the stack");

    // C2: kept set, bottom-up. `gone` ends as U∖K: T and the frames that rest
    // on something undone.
    let mut gone: DepSet = std::iter::once(target.my_id)
        .chain(rest.iter().map(|(f, _, _)| f.my_id))
        .collect();
    gone.sort_unstable();
    let mut kept = Vec::with_capacity(rest.len());
    let mut stats = Stats::default();
    for (frame, k, cp) in rest {
        if meets(&frame.pre, &gone) {
            ctx.remove_branch(frame.my_id);
            stats.dead += 1;
        } else {
            let pos = gone.binary_search(&frame.my_id).expect("frame id in U");
            gone.remove(pos);
            kept.push((frame, k, cp));
        }
    }
    stats.kept = kept.len();
    ctx.note_selective_jump(stats.kept, stats.dead);
    debug_assert!(
        kept.iter()
            .all(|(f, _, _)| deps.binary_search(&f.my_id).is_err()),
        "selective backjump: a kept frame is in the clash deps"
    );

    // C3: classify the entries from T's option checkpoint on.
    let start = TableauTrail::position(cp_t);
    let kept_cps: HashSet<usize> = kept
        .iter()
        .map(|(_, _, cp)| TableauTrail::position(*cp))
        .collect();
    let actions = classify(ctx, start, &gone, &kept_cps);
    stats.undone = actions.iter().filter(|a| **a == EntryAction::Undo).count();
    stats.kept_entries = actions.iter().filter(|a| **a == EntryAction::Keep).count();

    // C4: undo, rebuild the trail, reset what the trail does not describe.
    let remap = ctx.selective_undo_from(cp_t, &actions);
    for (_, _, cp) in &mut kept {
        *cp = cp.remap(&remap).expect("a kept frame's checkpoint is kept");
    }
    stats.recopied = recopy_kept_units(ctx, start, &mut kept);

    // C5: records. T refutes its option; kept frames forget what rested on
    // an abandoned decision, and every inconclusive marker (the bound was
    // reached under T's old option).
    target.records[k_t] = Some(Record::Refuted(deps));
    for (frame, _, _) in &mut kept {
        for record in &mut frame.records {
            let stale = match record {
                Some(Record::Refuted(d)) => meets(d, &gone),
                Some(Record::Inconclusive) => true,
                None => false,
            };
            if stale {
                *record = None;
                stats.deleted_records += 1;
            }
        }
    }
    // Re-number T: nothing live mentions its old id any more (its option and
    // complements were undone, kept frames' `pre` and records exclude it).
    let old_id = target.my_id;
    ctx.remove_branch(old_id);
    let new_id = ctx.push_branch();
    for record in &mut target.records {
        if let Some(Record::Refuted(d)) = record
            && let Ok(pos) = d.binary_search(&old_id)
        {
            d.remove(pos);
            // The fresh id is the largest, so the set stays sorted.
            d.push(new_id);
        }
    }
    target.my_id = new_id;
    stack.extend(kept);
    debug_assert!(
        stack
            .iter()
            .map(|(f, _, _)| f.my_id)
            .chain(std::iter::once(new_id))
            .eq(ctx.active_branches().iter().copied()),
        "selective backjump: stack ids {:?} + T′ {new_id} != active {:?}",
        stack.iter().map(|(f, _, _)| f.my_id).collect::<Vec<_>>(),
        ctx.active_branches()
    );
    debug_assert!(
        stack
            .iter()
            .map(|(f, _, _)| f)
            .chain(std::iter::once(&target))
            .all(|f| f.records.iter().all(|r| match r {
                Some(Record::Refuted(d)) => d
                    .iter()
                    .all(|x| ctx.active_branches().binary_search(x).is_ok()),
                _ => true,
            })),
        "selective backjump: a surviving record names an inactive decision"
    );
    if trace_enabled() {
        let exhausted = target.records.iter().all(Option::is_some);
        eprintln!(
            "# trace selective t_id={old_id} new_id={new_id} kept={} dead={} undone={} kept_entries={} recopied={} deleted_records={} t_exhausted={}",
            stats.kept,
            stats.dead,
            stats.undone,
            stats.kept_entries,
            stats.recopied,
            stats.deleted_records,
            u8::from(exhausted)
        );
    }
    if super::selective_check_enabled() {
        check_invariants(ctx, stack, Some(&target));
    }
    target
}

/// C3: what to do with each entry from `start` on. `gone` is U∖K; `kept_cps`
/// the kept frames' option checkpoints.
fn classify(
    ctx: &TableauContext<'_, '_, '_>,
    start: usize,
    gone: &DepSet,
    kept_cps: &HashSet<usize>,
) -> Vec<EntryAction> {
    let trail = ctx.trail();
    let graph = ctx.graph();
    let entries = trail.entries_from(start);
    // Only nodes created in the window can be dead: anything below `start`
    // was recorded before every decision in U existed.
    let mut dead_nodes: HashSet<NodeId> = HashSet::new();
    // Deps of edges added and then removed in the window.
    let mut removed_deps: HashMap<u32, &DepSet> = HashMap::new();
    for entry in entries {
        match entry {
            TrailEntry::NodeCreated { prior_len } => {
                let n = NodeId::new(u32::try_from(*prior_len).expect("node id fits u32"));
                debug_assert!(graph.is_alive(n), "window names a tombstone {n:?}");
                if meets(ctx.exist_deps_of(n), gone) {
                    dead_nodes.insert(n);
                }
            }
            TrailEntry::EdgeRemoved {
                id,
                prior_edge_deps,
                ..
            } => {
                removed_deps.insert(*id, prior_edge_deps);
            }
            _ => {}
        }
    }
    let dead = |n: NodeId| dead_nodes.contains(&n);
    let undo_if = |b: bool| {
        if b {
            EntryAction::Undo
        } else {
            EntryAction::Keep
        }
    };
    // The fate of the merge unit being walked: undone or kept.
    let mut unit: Option<bool> = None;
    let mut actions = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let action = match entry {
            TrailEntry::Checkpoint => {
                if kept_cps.contains(&(start + i)) {
                    EntryAction::Keep
                } else {
                    EntryAction::Drop
                }
            }
            TrailEntry::StableMark { .. } | TrailEntry::SelectiveBarrier => EntryAction::Drop,
            TrailEntry::LabelAdded { node, concept } => {
                let d = graph
                    .node(*node)
                    .deps_of_label(*concept)
                    .expect("a recorded label is present");
                undo_if(dead(*node) || meets(d, gone))
            }
            TrailEntry::EdgeAdded {
                from, target, id, ..
            } => {
                let d = graph
                    .node(*from)
                    .deps_of_edge_instance(*id)
                    .or_else(|| removed_deps.get(id).copied())
                    .expect("a recorded edge is present or removed in the window");
                undo_if(dead(*from) || dead(*target) || meets(d, gone))
            }
            TrailEntry::DistinctMarked { a, b } => {
                let d = ctx
                    .inequality_deps_of(*a, *b)
                    .expect("a recorded mark is present");
                undo_if(dead(*a) || dead(*b) || meets(d, gone))
            }
            TrailEntry::NodeCreated { prior_len } => {
                let n = NodeId::new(u32::try_from(*prior_len).expect("node id fits u32"));
                undo_if(dead(n))
            }
            TrailEntry::NominalAssigned { node, deps, .. } => {
                undo_if(dead(*node) || meets(deps, gone))
            }
            TrailEntry::MergeBegin {
                source,
                target,
                deps,
            } => {
                debug_assert!(unit.is_none(), "merge units never nest");
                let undo = dead(*source) || dead(*target) || meets(deps, gone);
                unit = Some(undo);
                if undo {
                    EntryAction::Drop
                } else {
                    EntryAction::Keep
                }
            }
            TrailEntry::MergeEnd => {
                let undo = unit.take().expect("MergeEnd closes a unit");
                if undo {
                    EntryAction::Drop
                } else {
                    EntryAction::Keep
                }
            }
            TrailEntry::EdgeRemoved { from, target, .. } => {
                undo_if(unit.unwrap_or(false) || dead(*from) || dead(*target))
            }
            TrailEntry::ParentRewritten { node, .. } | TrailEntry::MergedRedirect { node, .. } => {
                undo_if(unit.unwrap_or(false) || dead(*node))
            }
        };
        actions.push(action);
    }
    trail.undo_later_writes(start, &mut actions);
    trail.drop_removals_of_undone_edges(start, &mut actions);
    debug_assert!(
        entries.iter().zip(&actions).all(|(e, a)| match e {
            TrailEntry::EdgeRemoved { from, target, .. } =>
                *a == EntryAction::Drop || !(dead(*from) || dead(*target)),
            _ => true,
        }),
        "selective backjump: a removal names a dead node but is not dropped"
    );
    debug_assert!(
        undone_units_are_whole(entries, &actions),
        "selective backjump: an undone merge unit keeps a label or edge it moved"
    );
    actions
}

/// Every `LabelAdded` / `EdgeAdded` inside an undone merge unit is undone:
/// they carry the unit's folded deps (C3 debug check).
fn undone_units_are_whole(entries: &[TrailEntry], actions: &[EntryAction]) -> bool {
    let mut in_undone = false;
    for (e, a) in entries.iter().zip(actions) {
        match e {
            TrailEntry::MergeBegin { .. } => in_undone = *a == EntryAction::Drop,
            TrailEntry::MergeEnd => in_undone = false,
            TrailEntry::LabelAdded { .. } | TrailEntry::EdgeAdded { .. }
                if in_undone && *a != EntryAction::Undo =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// Facts a kept merge unit or a kept frame needs, put back where the trail
/// says they were established (plan 4c Revision 2, B-A, and C6).
///
/// A merge copies the source's labels and marks, and a frame asserts its
/// option, with first-writer adds, which record nothing when the fact is
/// already there. If that earlier fact was undone (it carried an abandoned
/// decision) while the unit or frame is kept, the fact is gone although what
/// established it stands, and nothing re-derives it: `saturate` skips
/// redirected sources, and a frame asserts its option once. (A choose frame
/// on `¬c` hits this whenever `c` was refuted under T: the complement it
/// asserts first carries T.) So, walking the rebuilt window in order:
/// - each kept unit `x → y` (deps Z) gets every label and mark x had before
///   the unit that y did not have by the unit's end, with deps
///   `deps(fact at x) ∪ Z`, spliced just before the unit's `MergeEnd`;
/// - each kept frame gets its current option (an `⊔` frame's label, a merge
///   frame's distinct marks) with its own deps, spliced just after its
///   checkpoint, unless it holds before the next kept frame's checkpoint.
///
/// A later entry that recorded the same fact is removed: in the rewritten
/// history it was a first-writer no-op. That entry may be another kept
/// frame's option or a kept unit's copy; the fact then holds from before
/// that frame's checkpoint or that unit, as a consequence its rollback does
/// not remove, which is sound (its next option asserts the complement and
/// clashes with deps covering the real refutation). Returns how many facts
/// were put back.
#[allow(clippy::many_single_char_names)]
fn recopy_kept_units(
    ctx: &mut TableauContext<'_, '_, '_>,
    start: usize,
    kept: &mut [Stacked],
) -> usize {
    let mut r = Restore::default();
    // Events in trail order: (doubled position, unit or frame index).
    let mut events: Vec<(usize, Event)> = Vec::new();
    {
        let mut open = None;
        for (i, e) in ctx.trail().entries_from(start).iter().enumerate() {
            let p = start + i;
            match *e {
                TrailEntry::LabelAdded { node, concept } => {
                    r.labels.insert((node, concept), 2 * p);
                }
                TrailEntry::DistinctMarked { a, b } => {
                    r.marks.insert(pair(a, b), 2 * p);
                }
                TrailEntry::MergeBegin {
                    source,
                    target,
                    ref deps,
                } => open = Some((p, source, target, deps.clone(), false)),
                TrailEntry::MergedRedirect { node, .. } => {
                    if let Some(u) = open.as_mut()
                        && u.1 == node
                    {
                        u.4 = true;
                    }
                }
                TrailEntry::MergeEnd => {
                    let (b, x, y, z, redirected) = open.take().expect("MergeEnd closes a unit");
                    // Plan S-9: a kept unit keeps its redirect (the chain
                    // rule never undoes it: nothing merges a redirected node).
                    debug_assert!(
                        redirected,
                        "kept merge unit {x:?} -> {y:?} lost its redirect"
                    );
                    if redirected {
                        events.push((2 * b, Event::Unit { b, e: p, x, y, z }));
                    }
                }
                _ => {}
            }
        }
        debug_assert!(open.is_none(), "unclosed merge unit in the window");
    }
    for (i, (_, _, cp)) in kept.iter().enumerate() {
        events.push((2 * TableauTrail::position(*cp), Event::Frame(i)));
    }
    events.sort_by_key(|(k, _)| *k);
    for (_, event) in events {
        match event {
            Event::Unit { b, e, x, y, z } => {
                let source = ctx.graph().node(x);
                let x_labels: Vec<(ConceptId, DepSet)> = source
                    .labels()
                    .iter()
                    .zip(&source.label_deps)
                    .filter(|&(&c, _)| before(&r.labels, &(x, c), 2 * b))
                    .map(|(&c, d)| (c, crate::deps::union(d, &z)))
                    .collect();
                let x_marks: Vec<(NodeId, DepSet)> = source
                    .inequalities()
                    .iter()
                    .zip(&source.inequality_deps)
                    .filter(|&(&o, _)| o != x && o != y && before(&r.marks, &pair(x, o), 2 * b))
                    .map(|(&o, d)| (o, crate::deps::union(d, &z)))
                    .collect();
                for (c, d) in x_labels {
                    r.ensure_label(ctx, y, c, d, e, 2 * e);
                }
                for (o, d) in x_marks {
                    r.ensure_mark(ctx, y, o, &d, e, 2 * e);
                }
            }
            Event::Frame(i) => {
                let (frame, k, cp) = &kept[i];
                if frame.retired {
                    continue;
                }
                let at = TableauTrail::position(*cp) + 1;
                let limit = kept
                    .get(i + 1)
                    .map_or(usize::MAX, |(_, _, next)| 2 * TableauTrail::position(*next));
                let with_me = |d: &DepSet| {
                    let mut d = d.clone();
                    if let Err(pos) = d.binary_search(&frame.my_id) {
                        d.insert(pos, frame.my_id);
                    }
                    d
                };
                match &frame.kind {
                    FrameKind::Or {
                        node,
                        ordered,
                        parent_deps,
                    } => {
                        let deps = with_me(parent_deps);
                        r.ensure_label(ctx, *node, ordered[*k], deps, at, limit);
                    }
                    FrameKind::Merge { open } if *k == open.partners.len() => {
                        let marks: Vec<(NodeId, DepSet)> = open
                            .partners
                            .iter()
                            .map(|(w, d)| (*w, with_me(d)))
                            .collect();
                        for (w, deps) in marks {
                            r.ensure_mark(ctx, open.pivot, w, &deps, at, limit);
                        }
                    }
                    // A merge option is its unit, which carries the frame's id
                    // and is kept with it.
                    FrameKind::Merge { .. } => {}
                }
            }
        }
    }
    let recopied = r.inserts.len();
    if recopied > 0 {
        let remap = ctx.trail_mut().splice(start, r.inserts, &r.removes);
        for (_, _, cp) in kept {
            *cp = cp.remap(&remap).expect("a splice keeps every checkpoint");
        }
    }
    recopied
}

/// What [`recopy_kept_units`] walks.
enum Event {
    /// A kept merge unit `x → y` with deps `z`, from `MergeBegin` at `b` to
    /// `MergeEnd` at `e`.
    Unit {
        b: usize,
        e: usize,
        x: NodeId,
        y: NodeId,
        z: DepSet,
    },
    /// The kept frame at this index.
    Frame(usize),
}

/// The window's facts by doubled trail position (a splice before entry `e`
/// sits at `2e - 1`, between `e - 1` and `e`; a fact with no position
/// predates the window), and the splice being built.
#[derive(Default)]
struct Restore {
    labels: HashMap<(NodeId, ConceptId), usize>,
    marks: HashMap<(NodeId, NodeId), usize>,
    inserts: Vec<(usize, TrailEntry)>,
    removes: HashSet<usize>,
}

impl Restore {
    /// Make `c@node` hold from before entry `at` on, unless it already holds
    /// before doubled position `limit`.
    fn ensure_label(
        &mut self,
        ctx: &mut TableauContext<'_, '_, '_>,
        node: NodeId,
        c: ConceptId,
        deps: DepSet,
        at: usize,
        limit: usize,
    ) {
        let present = ctx.graph().node(node).labels().binary_search(&c).is_ok();
        if present && before(&self.labels, &(node, c), limit) {
            return;
        }
        if present {
            let q = self.labels[&(node, c)];
            debug_assert!(
                q.is_multiple_of(2),
                "a later splice cannot precede this one"
            );
            self.removes.insert(q / 2);
        }
        ctx.put_label_unrecorded(node, c, deps);
        self.labels.insert((node, c), 2 * at - 1);
        self.inserts
            .push((at, TrailEntry::LabelAdded { node, concept: c }));
    }

    /// Make `a ≠ b` hold from before entry `at` on, unless it already holds
    /// before doubled position `limit`.
    fn ensure_mark(
        &mut self,
        ctx: &mut TableauContext<'_, '_, '_>,
        a: NodeId,
        b: NodeId,
        deps: &DepSet,
        at: usize,
        limit: usize,
    ) {
        let present = ctx.are_distinct(a, b);
        if present && before(&self.marks, &pair(a, b), limit) {
            return;
        }
        if present {
            let q = self.marks[&pair(a, b)];
            debug_assert!(
                q.is_multiple_of(2),
                "a later splice cannot precede this one"
            );
            self.removes.insert(q / 2);
        }
        ctx.put_mark_unrecorded(a, b, deps);
        self.marks.insert(pair(a, b), 2 * at - 1);
        self.inserts.push((at, TrailEntry::DistinctMarked { a, b }));
    }
}

/// Whether the fact `k` was in place before doubled position `limit` (a fact
/// with no position predates the window).
fn before<K: std::hash::Hash + Eq>(m: &HashMap<K, usize>, k: &K, limit: usize) -> bool {
    m.get(k).is_none_or(|&q| q < limit)
}

/// An unordered node pair as a key.
fn pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Oracle (a) (plan 4c C9 and S-5): structural invariants after a selective
/// backjump. Panics on the first violation.
pub(super) fn check_invariants(
    ctx: &TableauContext<'_, '_, '_>,
    stack: &[Stacked],
    next: Option<&Frame>,
) {
    let active = ctx.active_branches();
    let in_active = |d: &[u32]| d.iter().all(|x| active.binary_search(x).is_ok());
    let graph = ctx.graph();
    for i in 0..graph.len() {
        let id = NodeId::new(u32::try_from(i).expect("node id fits u32"));
        let n = graph.node(id);
        if !graph.is_alive(id) {
            assert!(
                n.labels().is_empty()
                    && n.edges().is_empty()
                    && n.in_edges().is_empty()
                    && n.inequalities().is_empty(),
                "oracle: tombstone {id:?} is not empty"
            );
            continue;
        }
        assert!(
            in_active(&n.exist_deps),
            "oracle: {id:?} exists under an inactive decision"
        );
        assert_eq!(
            n.labels.len(),
            n.label_deps.len(),
            "oracle: {id:?} label vectors"
        );
        assert!(
            n.edges.len() == n.edge_deps.len() && n.edges.len() == n.edge_ids.len(),
            "oracle: {id:?} edge vectors"
        );
        assert!(
            n.in_edges.len() == n.in_edge_deps.len() && n.in_edges.len() == n.in_edge_ids.len(),
            "oracle: {id:?} in-edge vectors"
        );
        assert_eq!(
            n.inequalities.len(),
            n.inequality_deps.len(),
            "oracle: {id:?} marks"
        );
        for d in &n.label_deps {
            assert!(
                in_active(d),
                "oracle: a label at {id:?} rests on an inactive decision"
            );
        }
        let mut ids = HashSet::new();
        for (pos, &(role, t)) in n.edges.iter().enumerate() {
            let eid = n.edge_ids[pos];
            assert!(ids.insert(eid), "oracle: duplicate edge id {eid} at {id:?}");
            assert!(graph.is_alive(t), "oracle: edge {id:?} -> dead {t:?}");
            assert!(
                in_active(&n.edge_deps[pos]),
                "oracle: edge at {id:?} rests on an inactive decision"
            );
            let m = graph.node(t);
            let back = m.in_edge_ids.iter().position(|&e| e == eid);
            assert!(
                back.is_some_and(
                    |p| m.in_edges[p] == (role, id) && m.in_edge_deps[p] == n.edge_deps[pos]
                ),
                "oracle: edge {id:?} -{role:?}-> {t:?} has no matching mirror"
            );
        }
        for (pos, &other) in n.inequalities.iter().enumerate() {
            assert!(
                graph.is_alive(other),
                "oracle: mark {id:?} != dead {other:?}"
            );
            let d = &n.inequality_deps[pos];
            assert!(
                in_active(d),
                "oracle: mark at {id:?} rests on an inactive decision"
            );
            assert_eq!(
                ctx.inequality_deps_of(other, id),
                Some(d),
                "oracle: mark {id:?} != {other:?} is not symmetric"
            );
        }
    }
    // Merge units balanced; frame checkpoints are checkpoints, ascending;
    // every recorded fact still holds in the graph (a wrong splice or removal
    // would show here, not later inside some frame's rollback).
    let entries = ctx.trail().entries_from(0);
    let mut open = false;
    for e in entries {
        match *e {
            TrailEntry::LabelAdded { node, concept } => assert!(
                graph.node(node).labels().binary_search(&concept).is_ok(),
                "oracle: trail records {concept:?} at {node:?}, the graph lacks it"
            ),
            TrailEntry::DistinctMarked { a, b } => assert!(
                ctx.are_distinct(a, b),
                "oracle: trail records {a:?} != {b:?}, the graph lacks it"
            ),
            TrailEntry::MergedRedirect {
                node, new_target, ..
            } => assert_eq!(
                graph.node(node).merged_into(),
                Some(new_target),
                "oracle: trail records the redirect of {node:?}, the graph differs"
            ),
            _ => {}
        }
        match e {
            TrailEntry::MergeBegin { .. } => {
                assert!(!open, "oracle: nested merge unit");
                open = true;
            }
            TrailEntry::MergeEnd => {
                assert!(open, "oracle: MergeEnd without MergeBegin");
                open = false;
            }
            _ => {}
        }
    }
    assert!(!open, "oracle: unclosed merge unit");
    let mut last = None;
    for (frame, _, cp) in stack {
        let p = TableauTrail::position(*cp);
        assert!(
            matches!(entries.get(p), Some(TrailEntry::Checkpoint)),
            "oracle: frame {} checkpoint is not a Checkpoint entry",
            frame.my_id
        );
        assert!(
            last.is_none_or(|l| l < p),
            "oracle: frame checkpoints out of order"
        );
        last = Some(p);
    }
    for frame in stack.iter().map(|(f, _, _)| f).chain(next) {
        assert!(
            in_active(&frame.pre),
            "oracle: frame {} rests on an inactive decision",
            frame.my_id
        );
    }
    // Every merge unit's target carries what its source had before it (plan
    // 4c S-10: facts recorded at the source after the unit are not the
    // merge's), from no later than the unit's end.
    let mut label_at: HashMap<(NodeId, ConceptId), usize> = HashMap::new();
    let mut mark_at: HashMap<(NodeId, NodeId), usize> = HashMap::new();
    let mut unit = None;
    for (p, e) in entries.iter().enumerate() {
        match *e {
            TrailEntry::LabelAdded { node, concept } => {
                label_at.insert((node, concept), p);
            }
            TrailEntry::DistinctMarked { a, b } => {
                mark_at.insert(pair(a, b), p);
            }
            TrailEntry::MergeBegin { source, target, .. } => unit = Some((p, source, target)),
            TrailEntry::MergeEnd => {
                let (b, x, y) = unit.take().expect("balanced");
                let src = graph.node(x);
                for &c in src.labels() {
                    if label_at.get(&(x, c)).is_none_or(|&q| q < b) {
                        assert!(
                            graph.node(y).labels().binary_search(&c).is_ok()
                                && label_at.get(&(y, c)).is_none_or(|&q| q < p),
                            "oracle: merge {x:?} -> {y:?} target lacks {c:?} its source had"
                        );
                    }
                }
                for &o in src.inequalities() {
                    if o != x && o != y && mark_at.get(&pair(x, o)).is_none_or(|&q| q < b) {
                        assert!(
                            ctx.are_distinct(y, o)
                                && mark_at.get(&pair(y, o)).is_none_or(|&q| q < p),
                            "oracle: merge {x:?} -> {y:?} target lacks the mark with {o:?}"
                        );
                    }
                }
            }
            _ => {}
        }
    }
    // Every kept frame still carries its current option.
    for (frame, k, _) in stack {
        if frame.retired {
            continue;
        }
        match &frame.kind {
            FrameKind::Or { node, ordered, .. } => {
                let n = ctx.resolve(*node);
                assert!(
                    graph.is_alive(n),
                    "oracle: frame {} node is dead",
                    frame.my_id
                );
                assert!(
                    graph.node(n).labels().binary_search(&ordered[*k]).is_ok(),
                    "oracle: frame {} lost its current option",
                    frame.my_id
                );
            }
            FrameKind::Merge { open } if *k == open.partners.len() => {
                for (w, _) in &open.partners {
                    assert!(
                        ctx.are_distinct(open.pivot, *w),
                        "oracle: merge frame {} lost a distinct mark",
                        frame.my_id
                    );
                }
            }
            FrameKind::Merge { open } => {
                assert_eq!(
                    ctx.resolve(open.pivot),
                    ctx.resolve(open.partners[*k].0),
                    "oracle: merge frame {} lost its merge",
                    frame.my_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::recopy_kept_units;
    use crate::TableauContext;
    use crate::trail::{EntryAction, TableauTrail, TrailEntry};
    use owl_dl_core::{ClassId, ConceptPool};

    /// Plan 4c Revision 2, B-A: `c` reaches `y` first under T (deps {2}), so
    /// the kept merge `x → y` (deps {1}) copies nothing. Undoing T's label
    /// leaves `y` without `c` although `x` still has it; the re-copy puts it
    /// back inside the unit, with the merge's deps, so a rollback past the
    /// merge removes it again.
    #[test]
    fn a_kept_merge_gets_back_what_an_undone_first_writer_held() {
        let mut pool = ConceptPool::new();
        let c = pool.atomic(ClassId::new(0));
        let mut ctx = TableauContext::new(&pool);
        ctx.set_selective_undo(true);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label_with_deps(x, c, &[]);
        let cp_t = ctx.checkpoint();
        ctx.add_label_with_deps(y, c, &[2]);
        let cp_k = ctx.checkpoint();
        assert!(ctx.merge_into_with_deps(x, y, &[1]));
        let start = TableauTrail::position(cp_t);
        let actions: Vec<EntryAction> = ctx
            .trail()
            .entries_from(start)
            .iter()
            .enumerate()
            .map(|(i, e)| match e {
                _ if i == 0 => EntryAction::Drop,
                TrailEntry::LabelAdded { node, .. } if *node == y => EntryAction::Undo,
                _ => EntryAction::Keep,
            })
            .collect();
        let remap = ctx.selective_undo_from(cp_t, &actions);
        assert!(
            ctx.graph().node(y).labels().is_empty(),
            "the hole B-A closes"
        );
        let mut kept = [];
        assert_eq!(recopy_kept_units(&mut ctx, start, &mut kept), 1);
        assert_eq!(
            ctx.graph()
                .node(y)
                .deps_of_label(c)
                .map(smallvec::SmallVec::as_slice),
            Some(&[1][..])
        );
        let window = ctx.trail().entries_from(start);
        let end = window
            .iter()
            .position(|e| matches!(e, TrailEntry::MergeEnd))
            .expect("unit kept");
        assert!(
            matches!(window[end - 1], TrailEntry::LabelAdded { node, concept } if node == y && concept == c),
            "the copy sits inside the unit"
        );
        // The checkpoint before the merge was kept and still undoes it, copy
        // included. (No frame holds it, so remap it through the undo only.)
        let cp_k = cp_k.remap(&remap).expect("kept");
        let cp_k = TableauTrail::position(cp_k);
        assert!(matches!(
            ctx.trail().entries_from(cp_k)[0],
            TrailEntry::Checkpoint
        ));
    }
}

//! #288 spec step 2: the go/no-go measurement for selective undo.
//!
//! Diagnostic only, behind `RUSTDL_GONOGO=1`. At every backjump that skips at
//! least [`MIN_SKIP`] active decisions it computes, per §4.1–4.3 of
//! `docs/superpowers/specs/2026-10-23-dependency-directed-undo-design.md`,
//! which of the undone decisions selective undo would keep, and which undone
//! trail entries do not depend on the flipped decision. It then times how long
//! the search takes to grow the trail back to its pre-jump length. Output is
//! one stderr line per event:
//!
//! - `# gng jump t_us=.. skipped=.. u=.. k=.. entries=.. indep=.. unknown=.. cost_us=..`
//! - `# gng rebuilt t_us=.. dt_us=.. trunc=0|1` (the jump above it)
//! - `# gng beat t_us=..` (about once a second, so an unfinished run has an end)
//!
//! Entries that carry no deps yet (`DistinctMarked`, merge entries,
//! `NominalAssigned`) are `unknown` unless they reference a dead node; the
//! analysis bounds them both ways.

use std::collections::HashMap;
use std::time::Instant;

use crate::TableauContext;
use crate::graph::{DepSet, NodeId};
use crate::trail::TrailEntry;

/// Shortest jump measured. The spec's bar is about jumps of ≥ 50 levels; the
/// analysis filters on `skipped`, so a lower floor only adds lines.
const MIN_SKIP: usize = 10;

#[derive(Default, Debug)]
pub(crate) struct State {
    /// Per decision: the trail length at its current option's checkpoint, and
    /// its precondition deps (§4.2).
    frames: HashMap<u32, (usize, DepSet, char)>,
    /// The jump being rebuilt: (start wall, start thread CPU µs, pre-jump trail length).
    pending: Option<(Instant, u64, usize)>,
    start: Option<Instant>,
    last_beat_s: u64,
    /// Set by the choose-rule call site so `branch` can tag its frame.
    pub(crate) next_choose: bool,
}

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTDL_GONOGO").is_some_and(|v| v == "1"))
}

/// This thread's CPU time in µs (`/proc/thread-self/stat`, 10 ms ticks).
fn cpu_us() -> u64 {
    let Ok(stat) = std::fs::read_to_string("/proc/thread-self/stat") else {
        return 0;
    };
    let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
        return 0;
    };
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks = f.get(11).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0)
        + f.get(12).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
    ticks * 10_000
}

fn micros(ctx: &mut TableauContext<'_, '_, '_>, at: Instant) -> u128 {
    let start = *ctx.gonogo.start.get_or_insert(at);
    at.duration_since(start).as_micros()
}

/// Record a decision's current option: its checkpoint position, its
/// precondition deps, and its kind (`o` ⊔, `c` choose, `m` merge). Choose
/// frames are never kept: their precondition (the `≤n` label and the owner)
/// is not recorded here, so the measurement takes the pessimistic side.
pub(crate) fn frame(
    ctx: &mut TableauContext<'_, '_, '_>,
    id: u32,
    cp_len: usize,
    pre: DepSet,
    kind: char,
) {
    if enabled() {
        ctx.gonogo.frames.insert(id, (cp_len, pre, kind));
    }
}

/// Existence deps of `node`: the deps of the edge that attaches it to its
/// creator, unioned up the creator chain (§4.3).
pub(crate) fn existence(
    ctx: &TableauContext<'_, '_, '_>,
    node: NodeId,
    memo: &mut HashMap<NodeId, DepSet>,
) -> DepSet {
    if let Some(d) = memo.get(&node) {
        return d.clone();
    }
    let graph = ctx.graph();
    let n = graph.node(node);
    let mut deps = DepSet::new();
    if let Some(parent) = n.parent {
        if let Some(pos) = n.in_edges.iter().position(|&(_, src)| src == parent) {
            deps = crate::deps::union(&deps, &n.in_edge_deps[pos]);
        } else if let Some(pos) = n.edges.iter().position(|&(_, t)| t == parent) {
            deps = crate::deps::union(&deps, &n.edge_deps[pos]);
        }
        if parent != node {
            // Mark before recursing: a rewritten parent chain could cycle.
            memo.insert(node, deps.clone());
            let up = existence(ctx, parent, memo);
            deps = crate::deps::union(&deps, &up);
        }
    }
    memo.insert(node, deps.clone());
    deps
}

fn meets(a: &[u32], dead: &[u32]) -> bool {
    a.iter().any(|x| dead.binary_search(x).is_ok())
}

/// Called wherever an `Unsat` with `deps` is about to propagate.
pub(crate) fn jump(ctx: &mut TableauContext<'_, '_, '_>, deps: &[u32]) {
    if !enabled() {
        return;
    }
    let t0 = Instant::now();
    let active: Vec<u32> = ctx.active_branches().to_vec();
    let Some(&target) = active
        .iter()
        .rev()
        .find(|id| deps.binary_search(id).is_ok())
    else {
        return;
    };
    let skipped = active.iter().filter(|&&id| id > target).count();
    if skipped < MIN_SKIP {
        return;
    }
    let Some((cp_len, _, _)) = ctx.gonogo.frames.get(&target).cloned() else {
        return;
    };
    // A new long jump ends the rebuild of the previous one.
    if let Some((s, c, _)) = ctx.gonogo.pending.take() {
        let t = micros(ctx, t0);
        eprintln!(
            "# gng rebuilt t_us={t} dt_us={} wall_us={} trunc=1",
            cpu_us().saturating_sub(c),
            t0.duration_since(s).as_micros()
        );
    }

    // §4.1: bottom-up from T, keep a frame iff its preconditions avoid U ∖ K.
    let mut dead: DepSet = active.iter().copied().filter(|&id| id >= target).collect();
    let u = dead.len();
    let (mut kinds, mut kept_kinds) = ([0usize; 4], [0usize; 4]);
    for &id in active.iter().filter(|&&id| id > target) {
        let frame = ctx.gonogo.frames.get(&id);
        let slot = match frame.map(|f| f.2) {
            Some('o') => 0,
            Some('c') => 1,
            Some('m') => 2,
            _ => 3,
        };
        kinds[slot] += 1;
        let keep = frame.is_some_and(|(_, pre, kind)| *kind != 'c' && !meets(pre, &dead));
        if keep {
            kept_kinds[slot] += 1;
            if let Ok(pos) = dead.binary_search(&id) {
                dead.remove(pos);
            }
        }
    }
    let k = u - dead.len();

    let mut memo = HashMap::new();
    let mut node_dead = |ctx: &TableauContext<'_, '_, '_>, n: NodeId| {
        let e = existence(ctx, n, &mut memo);
        meets(&e, &dead)
    };
    let (mut entries, mut indep, mut unknown) = (0usize, 0usize, 0usize);
    let trail_len = ctx.trail().len();
    // Merge survivors in the window: their labels may have moved from a node
    // that is dead only by existence closure, which merge deps do not carry.
    let survivors: std::collections::HashSet<NodeId> = ctx
        .trail()
        .entries_from(cp_len)
        .iter()
        .filter_map(|e| match e {
            TrailEntry::MergedRedirect { new_target, .. } => Some(*new_target),
            _ => None,
        })
        .collect();
    for entry in ctx.trail().entries_from(cp_len) {
        let verdict = match entry {
            TrailEntry::Checkpoint | TrailEntry::StableMark { .. } => continue,
            TrailEntry::LabelAdded { node, concept } => {
                if node_dead(ctx, *node) {
                    false
                } else {
                    // A label reached from a dead neighbour (∀ over a
                    // non-tree edge, inverse propagation) or moved onto a
                    // merge survivor may depend on the dead node without
                    // saying so: count it as unknown.
                    let n = ctx.graph().node(*node);
                    let near_dead = n
                        .edges
                        .iter()
                        .map(|&(_, t)| t)
                        .chain(n.in_edges.iter().map(|&(_, s)| s))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .any(|m| m != *node && node_dead(ctx, m));
                    if near_dead || survivors.contains(node) {
                        entries += 1;
                        unknown += 1;
                        continue;
                    }
                    ctx.label_deps_of(*node, *concept)
                        .is_some_and(|d| !meets(d, &dead))
                }
            }
            TrailEntry::EdgeAdded { from, role, target } => {
                let n = ctx.graph().node(*from);
                let edge_ok = n
                    .edges
                    .iter()
                    .position(|&(r, t)| r == *role && t == *target)
                    .is_some_and(|pos| !meets(&n.edge_deps[pos], &dead));
                edge_ok && !node_dead(ctx, *from) && !node_dead(ctx, *target)
            }
            TrailEntry::NodeCreated { prior_len } => {
                let id = NodeId::new(u32::try_from(*prior_len).expect("node index fits u32"));
                if id.index() as usize >= ctx.graph().len() {
                    entries += 1;
                    unknown += 1;
                    continue;
                }
                !node_dead(ctx, id)
            }
            TrailEntry::DistinctMarked { a, b } => {
                entries += 1;
                if !node_dead(ctx, *a) && !node_dead(ctx, *b) {
                    unknown += 1;
                }
                continue;
            }
            TrailEntry::EdgeRemoved { from, target, .. } => {
                entries += 1;
                if !node_dead(ctx, *from) && !node_dead(ctx, *target) {
                    unknown += 1;
                }
                continue;
            }
            TrailEntry::MergedRedirect {
                node, new_target, ..
            } => {
                entries += 1;
                if !node_dead(ctx, *node) && !node_dead(ctx, *new_target) {
                    unknown += 1;
                }
                continue;
            }
            TrailEntry::ParentRewritten { node, .. } => {
                entries += 1;
                if !node_dead(ctx, *node) {
                    unknown += 1;
                }
                continue;
            }
            TrailEntry::NominalAssigned { .. } => {
                entries += 1;
                unknown += 1;
                continue;
            }
        };
        entries += 1;
        if verdict {
            indep += 1;
        }
    }
    let t1 = Instant::now();
    let t = micros(ctx, t1);
    eprintln!(
        "# gng jump t_us={t} cpu_us={} skipped={skipped} u={u} k={k} entries={entries} \
         indep={indep} unknown={unknown} cost_us={} or={}/{} choose={}/{} merge={}/{} none={}",
        cpu_us(),
        t1.duration_since(t0).as_micros(),
        kept_kinds[0],
        kinds[0],
        kept_kinds[1],
        kinds[1],
        kept_kinds[2],
        kinds[2],
        kinds[3]
    );
    ctx.gonogo.pending = Some((t1, cpu_us(), trail_len));
}

/// Called at every `search` entry: closes a rebuild once the trail is back at
/// its pre-jump length, and prints a heartbeat about once a second.
pub(crate) fn tick(ctx: &mut TableauContext<'_, '_, '_>) {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let t = micros(ctx, now);
    if let Some((s, c, len)) = ctx.gonogo.pending
        && ctx.trail().len() >= len
    {
        ctx.gonogo.pending = None;
        eprintln!(
            "# gng rebuilt t_us={t} dt_us={} wall_us={} trunc=0",
            cpu_us().saturating_sub(c),
            now.duration_since(s).as_micros()
        );
    }
    let secs = u64::try_from(t / 1_000_000).unwrap_or(u64::MAX);
    if secs > ctx.gonogo.last_beat_s {
        ctx.gonogo.last_beat_s = secs;
        eprintln!("# gng beat t_us={t} cpu_us={}", cpu_us());
    }
}

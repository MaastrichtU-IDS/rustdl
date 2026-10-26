//! Decision and jump trace for the #288 step-4e measurement, behind
//! `RUSTDL_TRACE_DECISIONS=1` (plan 4e, revisions 4e-1 and 4e-2).
//!
//! Three kinds of line on stderr:
//! - `# search start cpu_ns=.. tid=..`, once per [`super::search`] call. Later
//!   CPU figures are deltas from it, so wedge and preparation work already on
//!   the thread is not charged to the search.
//! - `# dec n=.. <identity> depth=..`, one per option a frame starts. Buffered,
//!   and flushed at the next jump line or at the end of the search, so the
//!   arm that makes more decisions does not pay more write syscalls.
//! - `# jump n=.. <identity> skipped=.. kept_below=.. skipped_eff=.. sel=..
//!   kept=.. cpu_ns=.. t_dec=..`, one per backjump of at least
//!   [`BIG_JUMP`] levels, written at once. `cpu_ns` is sampled when the
//!   `Unsat` that causes it reaches the drive loop, before any undo.
//!
//! An identity never names a node: an `⊔` frame (or a choose frame) is the
//! hash of its sorted disjunct ids, the option index and the chosen disjunct;
//! a merge frame is its `≤n` concept and the option index. Concept ids are
//! interned at conversion from sorted axioms, so they are the same in both
//! arms of one binary on one input.

use super::selective::Stacked;
use super::{Frame, FrameKind};
use crate::graph::DepSet;
use std::fmt::Write as _;
use std::io::Write as _;

/// A backjump skipping at least this many frames (counted as `skipped_eff`)
/// gets a `# jump` line. `RUSTDL_TRACE_DECISIONS_MIN_JUMP` lowers it, to
/// check the jump lines by hand on small inputs; the measurement uses 50.
pub(super) const BIG_JUMP: usize = 50;

/// [`BIG_JUMP`], or `RUSTDL_TRACE_DECISIONS_MIN_JUMP`, read once.
fn big_jump() -> usize {
    use std::sync::OnceLock;
    static MIN: OnceLock<usize> = OnceLock::new();
    *MIN.get_or_init(|| {
        std::env::var("RUSTDL_TRACE_DECISIONS_MIN_JUMP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(BIG_JUMP)
    })
}

/// `RUSTDL_TRACE_DECISIONS=1`, read once.
pub(super) fn enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0); // 0=unknown, 1=off, 2=on
    let s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        let on = std::env::var("RUSTDL_TRACE_DECISIONS").as_deref() == Ok("1");
        STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
        on
    } else {
        s == 2
    }
}

/// CPU time of the calling thread in ns (first field of
/// `/proc/thread-self/schedstat`), or 0 where that file does not exist.
fn thread_cpu_ns() -> u64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// FNV-1a over the ids.
fn hash_ids(ids: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for id in ids {
        for b in id.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// The node-free identity of `frame`'s option `k`.
fn identity(frame: &Frame, k: usize) -> String {
    match &frame.kind {
        FrameKind::Or { ordered, .. } => {
            let mut ids: Vec<u32> = ordered.iter().map(|c| c.index()).collect();
            ids.sort_unstable();
            format!(
                "kind=or set={:016x} opt={k} chosen={}",
                hash_ids(&ids),
                ordered.get(k).map_or(u32::MAX, |c| c.index())
            )
        }
        FrameKind::Merge { open } => format!("kind=merge max={} opt={k}", open.max.index()),
    }
}

/// `(skipped, kept_below)` for a backjump caught at stack index `t`.
///
/// Plan 4e-2 B1 and its lineage refinement: after a selective backjump the
/// kept frames sit below T′, where the chronological arm rebuilds them above
/// T. Count back in every frame below the target that a backjump refuting the
/// same decision kept, so the same clash reads the same depth in both arms. A
/// target that is not a re-numbered T′ has an empty lineage and counts none:
/// in the chronological arm the frames below it are below it too.
fn depth_counts(stack: &[Stacked], t: usize) -> (usize, usize) {
    let skipped = stack.len() - 1 - t;
    let lineage = &stack[t].0.lineage;
    let kept_below = if lineage.is_empty() {
        0
    } else {
        stack[..t]
            .iter()
            .filter(|(f, _, _)| f.splice != 0 && lineage.contains(&f.splice))
            .count()
    };
    (skipped, kept_below)
}

/// A big jump seen at interception, written once the arm has handled it.
pub(super) struct Pending {
    line: String,
}

/// Per-search trace state.
pub(super) struct Tracer {
    buf: String,
    decs: u32,
    jumps: u32,
    splices: u32,
    /// A verdict is being passed down the stack (plan 4e S1): the frames it
    /// passes through are the same jump, not new ones. Cleared only when a
    /// frame starts an option (`Start::Child` or `Start::Decided`).
    unwinding: bool,
    start_ns: u64,
}

impl Tracer {
    /// A tracer when `RUSTDL_TRACE_DECISIONS=1`, after writing the start line.
    pub(super) fn new() -> Option<Self> {
        if !enabled() {
            return None;
        }
        let start_ns = thread_cpu_ns();
        eprintln!(
            "# search start cpu_ns={start_ns} tid={:?}",
            std::thread::current().id()
        );
        Some(Self {
            buf: String::new(),
            decs: 0,
            jumps: 0,
            splices: 0,
            unwinding: false,
            start_ns,
        })
    }

    /// `frame` asserted option `k` (`Start::Child`). A retired frame asserts
    /// nothing and gets no line (plan 4e S2).
    pub(super) fn dec(&mut self, frame: &mut Frame, k: usize, depth: usize) {
        self.unwinding = false;
        if frame.retired {
            return;
        }
        self.decs += 1;
        frame.dec_n = self.decs;
        let _ = writeln!(
            self.buf,
            "# dec n={} {} depth={depth}",
            self.decs,
            identity(frame, k)
        );
    }

    /// An option concluded without a search (`Start::Decided`).
    pub(super) fn decided(&mut self) {
        self.unwinding = false;
    }

    /// `Unsat(deps)` reached the drive loop with `stack`. Returns the jump
    /// line to write if this starts a backjump of at least [`BIG_JUMP`].
    pub(super) fn intercept(&mut self, stack: &[Stacked], deps: &DepSet) -> Option<Pending> {
        let (top, _, _) = stack.last()?;
        if deps.binary_search(&top.my_id).is_ok() || self.unwinding {
            return None;
        }
        self.unwinding = true;
        let t = stack
            .iter()
            .rposition(|(f, _, _)| deps.binary_search(&f.my_id).is_ok())?;
        let (skipped, kept_below) = depth_counts(stack, t);
        let (target, k, _) = &stack[t];
        let skipped_eff = skipped + kept_below;
        if skipped_eff < big_jump() {
            return None;
        }
        let cpu_ns = thread_cpu_ns().saturating_sub(self.start_ns);
        self.jumps += 1;
        Some(Pending {
            line: format!(
                "# jump n={} {} skipped={skipped} kept_below={kept_below} \
                 skipped_eff={skipped_eff} cpu_ns={cpu_ns} t_dec={}",
                self.jumps,
                identity(target, *k),
                target.dec_n
            ),
        })
    }

    /// A selective backjump to stack index `t` left `stack` and returned
    /// `renumbered`: tag the kept frames with a new splice id and add it to
    /// T′'s lineage (which T′ carries from earlier re-numberings).
    pub(super) fn splice(&mut self, stack: &mut [Stacked], t: usize, renumbered: &mut Frame) {
        self.splices += 1;
        for (f, _, _) in &mut stack[t..] {
            f.splice = self.splices;
        }
        renumbered.lineage.push(self.splices);
    }

    /// Write the buffered decision lines and then `pending`, as handled:
    /// `kept` frames survived (0 for a chronological unwind).
    pub(super) fn write(&mut self, pending: Pending, selective: bool, kept: usize) {
        let Pending { mut line } = pending;
        let _ = write!(line, " sel={} kept={kept}", u8::from(selective));
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(self.buf.as_bytes());
        self.buf.clear();
        let _ = writeln!(err, "{line}");
    }

    /// Write the buffered decision lines (end of the search).
    pub(super) fn flush(&mut self) {
        if !self.buf.is_empty() {
            let _ = std::io::stderr().lock().write_all(self.buf.as_bytes());
            self.buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tracer, depth_counts};
    use crate::TableauContext;
    use crate::graph::DepSet;
    use crate::search::Frame;
    use crate::search::selective::Stacked;
    use owl_dl_core::{ClassId, ConceptPool};

    fn tracer() -> Tracer {
        Tracer {
            buf: String::new(),
            decs: 0,
            jumps: 0,
            splices: 0,
            unwinding: false,
            start_ns: 0,
        }
    }

    /// Plan 4e lineage refinement: one decision T refuted three times by
    /// selective backjumps, each keeping frames above it. On the fourth clash
    /// at T‴ the chronological arm would have every frame kept on the way
    /// (`f1 f2`, then `g1`, then `h1`) above T, plus what was pushed since.
    #[test]
    fn a_decision_refuted_three_times_counts_every_kept_frame_back_in() {
        let mut pool = ConceptPool::new();
        let opts = [pool.atomic(ClassId::new(0)), pool.atomic(ClassId::new(1))];
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let cp = ctx.checkpoint();
        let frame = |ctx: &mut TableauContext<'_, '_, '_>| -> Stacked {
            (Frame::or(ctx, 10, n, &opts, &[], DepSet::new()), 0, cp)
        };
        let mut tr = tracer();
        let mut stack: Vec<Stacked> = Vec::new();
        let base = frame(&mut ctx);
        stack.push(base);
        let (mut t_frame, _, _) = frame(&mut ctx);
        // Splice 1 at T (index 1): f1, f2 kept.
        stack.push(frame(&mut ctx));
        stack.push(frame(&mut ctx));
        tr.splice(&mut stack, 1, &mut t_frame);
        stack.push((t_frame, 1, cp));
        // Splice 2 at T′ (index 3): g1 kept.
        stack.push(frame(&mut ctx));
        let (mut t_frame, _, _) = stack.remove(3);
        tr.splice(&mut stack, 3, &mut t_frame);
        stack.push((t_frame, 1, cp));
        // Splice 3 at T″ (index 4): h1 kept.
        stack.push(frame(&mut ctx));
        let (mut t_frame, _, _) = stack.remove(4);
        tr.splice(&mut stack, 4, &mut t_frame);
        stack.push((t_frame, 1, cp));
        assert_eq!(stack[5].0.lineage, vec![1, 2, 3]);
        // Two fresh frames above T‴, then a clash caught at T‴ (index 5).
        stack.push(frame(&mut ctx));
        stack.push(frame(&mut ctx));
        assert_eq!(
            depth_counts(&stack, 5),
            (2, 4),
            "f1 f2 g1 h1 counted back in"
        );
        // The bottom frame is no T′: nothing below it is counted.
        assert_eq!(depth_counts(&stack, 0), (7, 0));
        // A kept frame as the target counts nothing either.
        assert_eq!(depth_counts(&stack, 2).1, 0);
    }

    /// Frames kept by a backjump that refuted a different decision are not
    /// counted for this one.
    #[test]
    fn frames_kept_for_another_decision_are_not_counted() {
        let mut pool = ConceptPool::new();
        let opts = [pool.atomic(ClassId::new(0)), pool.atomic(ClassId::new(1))];
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let cp = ctx.checkpoint();
        let frame = |ctx: &mut TableauContext<'_, '_, '_>| -> Stacked {
            (Frame::or(ctx, 10, n, &opts, &[], DepSet::new()), 0, cp)
        };
        let mut tr = tracer();
        // Splice 1 at A (index 0) keeps x; A′ goes on top.
        let mut stack: Vec<Stacked> = vec![frame(&mut ctx)];
        let (mut a, _, _) = frame(&mut ctx);
        tr.splice(&mut stack, 0, &mut a);
        stack.push((a, 1, cp));
        // Splice 2 at B (index 2) keeps y; B′ goes on top.
        let (mut b, _, _) = frame(&mut ctx);
        stack.push(frame(&mut ctx));
        tr.splice(&mut stack, 2, &mut b);
        stack.push((b, 1, cp));
        assert_eq!(depth_counts(&stack, 3), (0, 1), "only y, kept for B");
        assert_eq!(depth_counts(&stack, 1), (2, 1), "only x, kept for A");
    }
}

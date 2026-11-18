//! TEMPORARY p12698 Step-0 instrumentation (diag/p12698-step0). Never ships.
//!
//! - `RUSTDL_P12698_CTR=1`: process-wide counters + timers, one summary line on
//!   stderr (`P12698SUM k=v ...`) printed by the CLI at exit.
//! - `RUSTDL_P12698_PAIR=1`: one `P12698PAIR` line per classify wedge probe.
//! - `RUSTDL_P12698_PREFILTER=1`: Fix C prototype (X-class reject before deferral).
#![allow(missing_docs, clippy::all, clippy::pedantic)]

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

fn flag(name: &str) -> bool {
    std::env::var(name).as_deref() == Ok("1")
}

#[inline]
pub fn on() -> bool {
    static O: OnceLock<bool> = OnceLock::new();
    *O.get_or_init(|| flag("RUSTDL_P12698_CTR"))
}

#[inline]
pub fn prefilter() -> bool {
    static O: OnceLock<bool> = OnceLock::new();
    *O.get_or_init(|| flag("RUSTDL_P12698_PREFILTER"))
}

#[inline]
pub fn pair_trace() -> bool {
    static O: OnceLock<bool> = OnceLock::new();
    *O.get_or_init(|| flag("RUSTDL_P12698_PAIR"))
}

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy)]
        #[allow(non_camel_case_types)]
        pub enum C { $($name),* }
        const NAMES: &[&str] = &[$(stringify!($name)),*];
    };
}

counters!(
    fcw,             // fire_clause_with calls
    fcw_nonhorn,     // ... returned at the non-Horn check
    ev,              // process_event calls
    edges,           // edges created (add_edge / fire_exists / at_least)
    nodes,           // new_node calls
    decisions,       // branches_taken increments
    deferred,        // fresh pushes at the defer site
    defer_dup,       // defer-site pushes absorbed by the dedup
    requeued,        // fresh defer-site pushes of an entry drained earlier in this engine
    drained,         // fire_deferred_gen calls (popped and fired)
    drain_rejected,  // ... whose re-match produced no binding
    prefilter_rej,   // Fix C prototype: defer-site firings rejected on a missing X-class
    head_skip,       // head_present_skip returns
    // match calls per category (after the defer/head-skip gates)
    m_pin_gen, m_pin_non, m_imm_gen, m_imm_non, m_drain_gen, m_drain_non,
    // bindings enumerated per category
    b_pin_gen, b_pin_non, b_imm_gen, b_imm_non, b_drain_gen, b_drain_non,
    // timers (ns) and their sample counts
    t_drain_ns, t_drain_n,           // fire_deferred_gen's fire_clause (match + heads)
    t_drain_match_ns, t_drain_match_n, // the match alone inside a drain
    t_queue_ns, t_queue_n,           // defer-site push + next_deferred_gen (incl. awaits_nominal_disjunction)
    t_defer_ns, t_defer_n,           // defers_generation (outside drains)
    t_pin_gen_ns, t_pin_gen_n,       // pinned firing of a generating clause (match + heads)
    t_imm_gen_ns, t_imm_gen_n,       // unpinned immediate firing of a generating clause
    probes,          // classify wedge probes (pair trace)
    probe_cpu_ns,    // their summed thread CPU
);

static CTR: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];

#[inline]
pub fn inc(c: C) {
    if on() {
        CTR[c as usize].fetch_add(1, Relaxed);
    }
}

#[inline]
pub fn add(c: C, n: u64) {
    if on() {
        CTR[c as usize].fetch_add(n, Relaxed);
    }
}

pub struct Timer(pub Option<(Instant, C, C)>);

impl Timer {
    #[inline]
    pub fn start(ns: C, n: C) -> Self {
        Timer(on().then(|| (Instant::now(), ns, n)))
    }
}

impl Drop for Timer {
    #[inline]
    fn drop(&mut self) {
        if let Some((t0, ns, n)) = self.0 {
            let d = u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX);
            CTR[ns as usize].fetch_add(d, Relaxed);
            CTR[n as usize].fetch_add(1, Relaxed);
        }
    }
}

thread_local! {
    /// Bindings produced by the last `fire_clause_with` match on this thread
    /// (`u64::MAX` when it returned before matching).
    pub static LAST_BINDINGS: Cell<u64> = const { Cell::new(u64::MAX) };
}

/// Thread CPU time in ns (`CLOCK_THREAD_CPUTIME_ID`).
pub fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid pointer to a timespec on the stack.
    unsafe {
        libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}

/// Per-timer overhead (ns) of one `Timer` start/stop pair, calibrated here.
fn timer_overhead_ns() -> f64 {
    let n = 1_000_000u32;
    let t0 = Instant::now();
    let mut acc = 0u128;
    for _ in 0..n {
        let a = Instant::now();
        acc += std::hint::black_box(a.elapsed().as_nanos());
    }
    std::hint::black_box(acc);
    t0.elapsed().as_nanos() as f64 / f64::from(n)
}

/// The summary line, or `None` when the counters are off.
pub fn summary() -> Option<String> {
    if !on() {
        return None;
    }
    let mut s = String::from("P12698SUM");
    for (i, name) in NAMES.iter().enumerate() {
        s.push_str(&format!(" {name}={}", CTR[i].load(Relaxed)));
    }
    s.push_str(&format!(" timer_overhead_ns={:.1}", timer_overhead_ns()));
    s.push_str(&format!(
        " gl={} prefilter={} nmp={}",
        std::env::var("RUSTDL_WEDGE_GENERATE_LAST").unwrap_or_else(|_| "default".into()),
        prefilter() as u8,
        std::env::var("RUSTDL_NOMINAL_MERGE_PRUNE").unwrap_or_else(|_| "default".into()),
    ));
    Some(s)
}

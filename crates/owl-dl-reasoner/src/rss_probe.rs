//! Diagnostic RSS (resident-set-size) probes for memory attribution.
//!
//! Controlled by the `RUSTDL_TRACE_RSS` environment variable.  When the var is
//! absent (the default), [`probe`] returns immediately without touching `/proc`
//! — the off-path cost is a single relaxed atomic load.  When the var is set
//! (any non-empty value), each probe reads `/proc/self/statm` and emits one
//! line to stderr:
//!
//! ```text
//! [rss] <phase>=<N.N>GB
//! ```
//!
//! The `pair=NNN` variant is produced by [`probe_pair`] for the periodic
//! pair-counter probe inside the tier walk.
//!
//! Both functions compile to a no-op on non-Linux targets (the
//! `#[cfg(target_os = "linux")]` guard in `read_rss_gb` returns `0.0` there).
//!
//! **Thread safety of the gate.** The `AtomicBool` is initialised on first
//! access via a `OnceLock<bool>` so that the env lookup happens at most once
//! per process (matching the `RUSTDL_TRACE` / `hyper_trust_sat_min_ms`
//! conventions in `lib.rs`).  Because probes fire from rayon worker threads the
//! lock-free `AtomicBool` cache avoids the lock contention a `OnceLock<bool>`
//! alone would incur after initialisation.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Internal state for the one-time env lookup.
///
/// 0 = not-yet-initialised, 1 = disabled, 2 = enabled.
static RSS_STATE: AtomicU8 = AtomicU8::new(0);
/// Storage for the `OnceLock` init result, used to promote to the atomic.
static RSS_INIT: OnceLock<bool> = OnceLock::new();
/// Fast-path cache: set to `true` once we know the env var is present.
static RSS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Returns `true` when `RUSTDL_TRACE_RSS` is set to a non-empty value.
///
/// The result is computed once and cached — subsequent calls are a single
/// relaxed atomic load.
#[inline]
fn trace_rss_enabled() -> bool {
    // Fast path: already initialised.
    if RSS_STATE.load(Ordering::Relaxed) != 0 {
        return RSS_ENABLED.load(Ordering::Relaxed);
    }
    // Slow path: initialise via OnceLock (at most once, even under contention).
    let enabled = *RSS_INIT.get_or_init(|| {
        std::env::var_os("RUSTDL_TRACE_RSS").is_some_and(|v| !v.is_empty() && v != "0")
    });
    RSS_ENABLED.store(enabled, Ordering::Relaxed);
    RSS_STATE.store(if enabled { 2 } else { 1 }, Ordering::Relaxed);
    enabled
}

/// Read `/proc/self/statm` field 2 (resident pages) and convert to GB.
///
/// Returns `0.0` on any parse error or on non-Linux platforms.
///
/// `/proc/self/statm` reports counts in OS pages.  The page size is obtained
/// once via `/proc/self/smaps_rollup` (which gives RSS in kB and pages, letting
/// us derive it), but that is needlessly complex.  Instead we fall back to the
/// standard 4 KiB page, which is invariant on x86-64 and arm64 Linux.  This
/// function is a diagnostic probe — sub-percent accuracy is fine.
#[must_use]
fn read_rss_gb() -> f64 {
    #[cfg(target_os = "linux")]
    {
        // `/proc/self/statm` is a single line of whitespace-separated u64s.
        // Field 0 = VmSize pages, field 1 = VmRSS pages.
        let Ok(text) = std::fs::read_to_string("/proc/self/statm") else {
            return 0.0;
        };
        let mut fields = text.split_ascii_whitespace();
        let _vmsize = fields.next(); // field 0: skip
        let Some(rss_str) = fields.next() else {
            return 0.0;
        };
        let Ok(rss_pages) = rss_str.parse::<u64>() else {
            return 0.0;
        };
        // 4096 bytes per page is invariant on every x86-64 / arm64 Linux
        // kernel this binary targets.  Diagnostic-only: sub-percent precision
        // is fine even if a future port runs a 64 KiB-page kernel.
        // The cast loses at most 1 ULP at 2^52 pages (~16 PiB RSS) — negligible.
        #[allow(clippy::cast_precision_loss)]
        let gb = (rss_pages * 4096) as f64 / (1024.0 * 1024.0 * 1024.0);
        gb
    }
    #[cfg(not(target_os = "linux"))]
    {
        0.0
    }
}

/// Emit `[rss] <phase>=<N.N>GB` to stderr when `RUSTDL_TRACE_RSS` is set.
///
/// When the env var is absent this is a no-op (one relaxed atomic load).
pub(crate) fn probe(phase: &str) {
    if !trace_rss_enabled() {
        return;
    }
    let gb = read_rss_gb();
    eprintln!("[rss] {phase}={gb:.2}GB");
}

/// Read the `RUSTDL_TRACE_RSS_EVERY` env knob (default 100).
///
/// Cached via `OnceLock` — called once per probe site, but the site itself
/// only fires when the per-pair counter hits the interval.
fn trace_rss_every() -> u64 {
    static EVERY: OnceLock<u64> = OnceLock::new();
    *EVERY.get_or_init(|| {
        std::env::var("RUSTDL_TRACE_RSS_EVERY")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(100)
    })
}

/// Emit `[rss] pair=<N> rss=<N.N>GB` every `RUSTDL_TRACE_RSS_EVERY` classes.
///
/// `pair_idx` is the 1-based ordinal of the class just completed in the
/// serial tier-merge phase (post-rayon collect).  We probe here — not inside
/// the rayon closure — to avoid interleaved output from concurrent threads.
/// The cost when disabled is one relaxed atomic load per call.
pub(crate) fn probe_pair(pair_idx: u64) {
    if !trace_rss_enabled() {
        return;
    }
    let every = trace_rss_every();
    if !pair_idx.is_multiple_of(every) {
        return;
    }
    let gb = read_rss_gb();
    eprintln!("[rss] pair={pair_idx} rss={gb:.2}GB");
}

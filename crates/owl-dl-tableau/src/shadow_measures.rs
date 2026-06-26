//! Read-only measures over the shadow-dep probe's clash records.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::hyper::{ClashRecord, DepSetSnapshot};
use std::collections::HashMap;

pub struct Histogram {
    pub min: u32,
    pub median: u32,
    pub p90: u32,
    pub max: u32,
    pub mean: f64,
}

impl Histogram {
    fn from_samples(xs: &[u32]) -> Histogram {
        if xs.is_empty() {
            return Histogram {
                min: 0,
                median: 0,
                p90: 0,
                max: 0,
                mean: 0.0,
            };
        }
        let mut v = xs.to_vec();
        v.sort_unstable();
        let pick = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
        Histogram {
            min: v[0],
            median: pick(0.5),
            p90: pick(0.9),
            max: v[v.len() - 1],
            mean: xs.iter().map(|&x| f64::from(x)).sum::<f64>() / xs.len() as f64,
        }
    }
}

pub struct ShadowReport {
    pub n_clashes: usize,
    pub bjgap_real: Histogram,
    pub bjgap_shadow: Histogram,
    pub reusable_nogood_frac: f64,
    pub distinct_nogoods: usize,
    pub revisit_frac: f64,
    pub revisit_context_shared_frac: f64,
}

// bjgap = branch_depth - highest + 1 (levels skipped; 1 = useless). No highest
// (EMPTY deps) => the clash is context-free => gap = branch_depth (jump to root).
fn bjgap(depth: u32, snap: &DepSetSnapshot) -> u32 {
    match snap.highest {
        Some(h) => depth.saturating_sub(h).saturating_add(1),
        None => depth.saturating_add(1),
    }
}

pub fn analyze(records: &[ClashRecord]) -> ShadowReport {
    let real: Vec<u32> = records
        .iter()
        .map(|r| bjgap(r.branch_depth, &r.real))
        .collect();
    let shadow: Vec<u32> = records
        .iter()
        .map(|r| bjgap(r.branch_depth, &r.shadow))
        .collect();
    // reusable NOGOOD = the precise shadow dep-SET (the actual nogood) recurs across
    // >=2 records. Keyed on the shadow levels, NOT the state — this is the
    // caching/CDCL signal (a context-independent nogood reusable across branches).
    let nogood_key = |r: &ClashRecord| -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        r.shadow.levels.hash(&mut h);
        h.finish()
    };
    let mut ngc: HashMap<u64, usize> = HashMap::new();
    for r in records {
        *ngc.entry(nogood_key(r)).or_default() += 1;
    }
    let distinct_nogoods = ngc.len();
    let reusable: usize = ngc.values().filter(|&&c| c >= 2).copied().sum();
    let reusable_nogood_frac = if records.is_empty() {
        0.0
    } else {
        reusable as f64 / records.len() as f64
    };
    // revisited STATE = the clash node's label-set (clash_label_key) recurs. Distinct
    // from nogood reuse: a state can recur under different nominal contexts.
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for r in records {
        *counts.entry(r.clash_label_key).or_default() += 1;
    }
    let revisited: usize = counts.values().filter(|&&c| c >= 2).copied().sum();
    let revisit_frac = if records.is_empty() {
        0.0
    } else {
        revisited as f64 / records.len() as f64
    };
    // context-sharing: of revisited keys, fraction whose shadow dep-set highest matches
    // across occurrences (same nominal context => cacheable; differing => reuse-trap).
    let mut by_key: HashMap<u64, Vec<Option<u32>>> = HashMap::new();
    for r in records {
        by_key
            .entry(r.clash_label_key)
            .or_default()
            .push(r.shadow.highest);
    }
    let (mut shared, mut total) = (0usize, 0usize);
    for (_k, hs) in by_key.iter().filter(|(_, h)| h.len() >= 2) {
        total += hs.len();
        let first = hs[0];
        shared += hs.iter().filter(|&&h| h == first).count();
    }
    let revisit_context_shared_frac = if total == 0 {
        0.0
    } else {
        shared as f64 / total as f64
    };
    ShadowReport {
        n_clashes: records.len(),
        bjgap_real: Histogram::from_samples(&real),
        bjgap_shadow: Histogram::from_samples(&shadow),
        reusable_nogood_frac,
        distinct_nogoods,
        revisit_frac,
        revisit_context_shared_frac,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(depth: u32, real_hi: Option<u32>, shadow_hi: Option<u32>, key: u64) -> ClashRecord {
        ClashRecord {
            branch_depth: depth,
            real: DepSetSnapshot {
                highest: real_hi,
                count: real_hi.map_or(0, |_| 1),
                levels: real_hi.into_iter().collect(),
            },
            shadow: DepSetSnapshot {
                highest: shadow_hi,
                count: shadow_hi.map_or(0, |_| 1),
                levels: shadow_hi.into_iter().collect(),
            },
            clash_label_key: key,
        }
    }
    #[test]
    fn bjgap_and_reuse_are_computed() {
        // Two clashes at depth 10: real highest=10 (bjgap 1 = useless),
        // shadow highest=2 (bjgap 9 = precise backjump). Same nogood key twice => reusable.
        let recs = vec![rec(10, Some(10), Some(2), 7), rec(10, Some(10), Some(2), 7)];
        let r = analyze(&recs);
        assert_eq!(r.n_clashes, 2);
        assert_eq!(r.bjgap_real.median, 1); // 10 - 10 + 1
        assert_eq!(r.bjgap_shadow.median, 9); // 10 - 2 + 1
        assert!(r.reusable_nogood_frac > 0.0); // key 7 recurs
        assert_eq!(r.distinct_nogoods, 1);
    }
}

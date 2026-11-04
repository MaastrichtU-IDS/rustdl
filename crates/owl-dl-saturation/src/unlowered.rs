//! Where the EL lowering drops, or weakens, a construct it was handed (layer 2 of the
//! pure-EL guard, after #440).
//!
//! The pure-EL gates (`is_pure_el`, `saturator_complete_fragment`) certify a closure
//! complete on the strength of the saturator's rules. Every #435/#436-style bug (the
//! "D10" class: the D10 bot filler, `And(..) ⊑ ∃R.B`, the SWEET domain trigger, #110,
//! #119, #435, #436) was a gate-admitted construct that the lowering silently turned into
//! no rule, or a weaker one. Each was found only by a peer disagreement after the fact.
//! This module makes the lowering report those cases itself, so a test can assert there
//! are none on certified inputs, with no oracle.
//!
//! # How sites are placed
//!
//! A site records in the **caller**, after every fallback has been tried, never inside a
//! helper whose `None` a caller recovers from. Placed there, a helper that degrades
//! silently (returns less than it was asked for) is caught by the caller's check of its
//! output. That is how the #114 sabotage of `domain_heads_el` is caught: see
//! `domain-heads-lossy`. Weakening counts: `top_weakened_existential_rhs` turns an
//! unlowerable `∃r.C` into `∃r.⊤`, which is sound but loses `C`, so it records as
//! `rhs-exists-weakened`.
//!
//! Vacuous inputs need no rule and are not recorded: `C ⊑ ⊤`, `⊥ ⊑ C`,
//! `Domain/Range(r, ⊤)`. `ConceptPool::and` flattens nested `⊓`, drops `⊤` and absorbs
//! `⊥`, so an EL conjunct is always atomic or a non-inverse `∃`.
//!
//! # Cost when no recorder is installed
//!
//! One relaxed atomic load per axiom in the lowering loops, plus one per drop branch
//! actually taken and one per site whose check would cost work (those test [`active`]
//! first, so they walk no conjuncts). There is no thread-local access, no allocation and no env read, and
//! nothing here touches the saturation loop. Lowering is single-threaded, so the
//! recorder lives in a thread-local that the diagnostic entry points install around the
//! real lowering code.
//!
//! # Limit
//!
//! A drop path nobody instruments is invisible. A new `return None` or `_ => {}` in the
//! lowering should record here. Role-level drops (chains longer than 2, inverse chains)
//! and the nominal passes are out of scope: the gates reject those inputs.

use owl_dl_core::ConceptId;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One construct the lowering dropped or weakened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlowered {
    /// Index into `InternalOntology::axioms` of the axiom being lowered.
    pub axiom: usize,
    /// Which site recorded it (a stable kebab-case name; see the module doc).
    pub site: &'static str,
    /// The concept that was dropped or weakened (the whole consequent where the site
    /// cannot single out a part); `None` for a site that drops a whole non-concept
    /// axiom, such as an inverse-role assertion.
    pub concept: Option<ConceptId>,
}

/// How many recorders are installed, on any thread. Zero ⟹ every site is one load.
static RECORDERS: AtomicUsize = AtomicUsize::new(0);

struct Recorder {
    axiom: usize,
    out: Vec<Unlowered>,
}

thread_local! {
    static REC: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

#[inline]
fn recording() -> bool {
    RECORDERS.load(Ordering::Relaxed) != 0
}

/// Whether any recorder is installed. A site whose check itself costs work (walking a
/// concept's conjuncts) tests this first, so it costs nothing when no one records.
#[inline]
pub(crate) fn active() -> bool {
    recording()
}

/// Note which axiom the lowering is on. One relaxed load when nothing records.
#[inline]
pub(crate) fn set_axiom(axiom: usize) {
    if recording() {
        set_axiom_slow(axiom);
    }
}

#[cold]
fn set_axiom_slow(axiom: usize) {
    REC.with(|r| {
        if let Some(rec) = r.borrow_mut().as_mut() {
            rec.axiom = axiom;
        }
    });
}

/// Record a dropped or weakened construct. One relaxed load when nothing records.
#[inline]
pub(crate) fn note(site: &'static str, concept: ConceptId) {
    if recording() {
        note_slow(site, Some(concept));
    }
}

/// Record a dropped axiom that has no concept to point at.
#[inline]
pub(crate) fn note_axiom(site: &'static str) {
    if recording() {
        note_slow(site, None);
    }
}

#[cold]
fn note_slow(site: &'static str, concept: Option<ConceptId>) {
    REC.with(|r| {
        if let Some(rec) = r.borrow_mut().as_mut() {
            let axiom = rec.axiom;
            rec.out.push(Unlowered {
                axiom,
                site,
                concept,
            });
        }
    });
}

/// Installs a recorder on this thread for its lifetime; panic-safe.
struct Installed;

impl Installed {
    fn new() -> Self {
        REC.with(|r| {
            debug_assert!(
                r.borrow().is_none(),
                "nested unlowered recorders on one thread would lose the outer one's records"
            );
            *r.borrow_mut() = Some(Recorder {
                axiom: 0,
                out: Vec::new(),
            });
        });
        RECORDERS.fetch_add(1, Ordering::Relaxed);
        Installed
    }

    fn take(self) -> Vec<Unlowered> {
        let out = REC
            .with(|r| {
                r.borrow_mut()
                    .as_mut()
                    .map(|rec| std::mem::take(&mut rec.out))
            })
            .unwrap_or_default();
        drop(self);
        out
    }
}

impl Drop for Installed {
    fn drop(&mut self) {
        // `try_with`: teardown during thread exit must not panic.
        let _ = REC.try_with(|r| {
            if let Ok(mut slot) = r.try_borrow_mut() {
                *slot = None;
            }
        });
        RECORDERS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Run `f` with a recorder installed on this thread and return what it recorded.
pub(crate) fn recorded<T>(f: impl FnOnce() -> T) -> (T, Vec<Unlowered>) {
    let guard = Installed::new();
    let value = f();
    (value, guard.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_is_empty() -> bool {
        REC.with(|r| r.borrow().is_none())
    }

    #[test]
    fn notes_without_a_recorder_go_nowhere() {
        set_axiom(7);
        note_axiom("before");
        assert!(slot_is_empty());
        let ((), out) = recorded(|| note_axiom("during"));
        assert_eq!(out.iter().map(|u| u.site).collect::<Vec<_>>(), ["during"]);
        assert_eq!(
            out[0].axiom, 0,
            "the recorder starts at axiom 0, not a stale index"
        );
        note_axiom("after");
        assert!(slot_is_empty());
    }

    #[test]
    fn a_panic_inside_the_recorder_leaves_no_recorder_behind() {
        let caught = std::panic::catch_unwind(|| {
            recorded(|| {
                note_axiom("lost");
                panic!("lowering panicked");
            })
        });
        assert!(caught.is_err());
        assert!(slot_is_empty());
        let ((), out) = recorded(|| note_axiom("fresh"));
        assert_eq!(out.iter().map(|u| u.site).collect::<Vec<_>>(), ["fresh"]);
    }
}

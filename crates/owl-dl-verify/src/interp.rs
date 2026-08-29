//! The interpretation interface the evaluator sees.
//!
//! Deliberately knows nothing about how a model is built: `eval.rs` is generic
//! over this trait so it cannot reach the saturation engine it checks.

use owl_dl_core::{ClassId, RoleId};

/// An element of an interpretation's domain.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Element(u32);

impl Element {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A finite first-order interpretation over rustdl's class and role ids.
pub trait Interpretation {
    fn domain_size(&self) -> usize;
    fn elements(&self) -> impl Iterator<Item = Element> + '_;
    /// Is `e` in the extension of atomic class `c`?
    fn in_concept(&self, e: Element, c: ClassId) -> bool;
    /// Successors of `e` under `r`, INCLUDING edges held by any sub-role of `r`.
    ///
    /// Returns an owned `Vec` rather than a slice because the sub-role union is
    /// not a stored contiguous run; promising `&[Element]` would force
    /// materialising the sub-role closure, which the builder deliberately avoids.
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element>;
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool;
    /// Every edge of `r` (incl. sub-role edges), for whole-extension axioms.
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)>;
    fn num_roles(&self) -> usize;
}

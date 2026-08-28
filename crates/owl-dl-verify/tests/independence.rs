//! `eval.rs` must not reference the saturation engine. This is the crate's
//! reason to exist: an evaluator sharing code with the engine could hide the
//! very bug it is built to find.

#[test]
fn eval_module_does_not_reference_the_saturation_engine() {
    let src = include_str!("../src/eval.rs");
    assert!(
        !src.contains("owl_dl_saturation"),
        "eval.rs must stay engine-blind; found an owl_dl_saturation reference"
    );
}

#[test]
fn eval_module_has_no_wildcard_match_arm() {
    let src = include_str!("../src/eval.rs");
    assert!(
        !src.contains("_ =>"),
        "a wildcard arm silently skips axiom/concept forms; unhandled forms must \
         yield Unresolved instead"
    );
}

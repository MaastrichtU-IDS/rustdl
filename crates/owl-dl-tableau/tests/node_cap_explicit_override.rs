//! An explicit `RUSTDL_MAX_NODES` also governs deadline-free single queries
//! (#363 A2): users and tests that set it keep exact control, and the larger
//! `query_max_nodes()` default applies only when it is unset. The caps are cached
//! once per process, so this is the only test in its binary.

#[test]
#[allow(unsafe_code)]
fn an_explicit_max_nodes_wins_over_the_query_default() {
    // SAFETY: set_var is unsafe under edition 2024; this is the only test in the
    // binary and runs before anything reads the cached caps.
    unsafe {
        std::env::set_var("RUSTDL_MAX_NODES", "123");
        std::env::remove_var("RUSTDL_QUERY_MAX_NODES");
    }
    assert_eq!(owl_dl_tableau::max_nodes(), Some(123));
    assert_eq!(owl_dl_tableau::query_max_nodes(), Some(123));
}

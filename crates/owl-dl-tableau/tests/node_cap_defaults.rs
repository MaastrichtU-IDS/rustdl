//! Pins the live-node cap defaults (#35 v4, #363 A2).
//!
//! `max_nodes()` (classify and every deadline-bounded path) defaults to 50000.
//! `query_max_nodes()` (deadline-free single queries: `sat`, `subclass`,
//! `instance` with no probe deadline) defaults to 500000: at 50000 those queries
//! reported `unknown` on 3 of 43 measurable random ORE ontologies that finish
//! with a 200k–500k cap. Both are cached once per process, so this binary must
//! not set `RUSTDL_MAX_NODES` or `RUSTDL_QUERY_MAX_NODES`; the explicit-override
//! rule is pinned in its own binary, `node_cap_explicit_override.rs`.
#![allow(clippy::unwrap_used)]

#[test]
fn node_cap_defaults_and_the_thread_override() {
    assert!(
        std::env::var_os("RUSTDL_MAX_NODES").is_none()
            && std::env::var_os("RUSTDL_QUERY_MAX_NODES").is_none(),
        "run this test with neither cap variable set"
    );
    assert_eq!(owl_dl_tableau::max_nodes(), Some(50_000));
    assert_eq!(owl_dl_tableau::query_max_nodes(), Some(500_000));

    // Without an override, searches use the classify-wide cap.
    assert_eq!(owl_dl_tableau::effective_max_nodes(), Some(50_000));
    {
        let _outer = owl_dl_tableau::NodeCapOverride::enter(Some(7));
        assert_eq!(owl_dl_tableau::effective_max_nodes(), Some(7));
        {
            let _inner = owl_dl_tableau::NodeCapOverride::enter(None);
            assert_eq!(owl_dl_tableau::effective_max_nodes(), None);
        }
        assert_eq!(
            owl_dl_tableau::effective_max_nodes(),
            Some(7),
            "inner guard restores"
        );
        // A spawned thread does not inherit the override; callers pass it on.
        let seen = std::thread::spawn(owl_dl_tableau::effective_max_nodes)
            .join()
            .unwrap();
        assert_eq!(seen, Some(50_000));
    }
    assert_eq!(
        owl_dl_tableau::effective_max_nodes(),
        Some(50_000),
        "outer guard restores"
    );
}

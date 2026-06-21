import rustdl


def test_materialize_object_property_assertions(fixtures_dir):
    # hasFather ⊑ hasParent ⊑ hasAncestor, hasAncestor transitive,
    # hasFather(alice, bob) and hasParent(bob, carol) asserted.
    fixture = fixtures_dir / "abox" / "property_materialization.ofn"
    triples = rustdl.materialize_inferred_object_property_assertions(str(fixture))
    assert isinstance(triples, list)
    got = {(s, p, o) for (s, p, o) in triples}

    F = "http://t/hasFather"
    P = "http://t/hasParent"
    A = "http://t/hasAncestor"
    alice, bob, carol = "http://t/alice", "http://t/bob", "http://t/carol"

    expected = {
        (alice, F, bob),   # asserted
        (alice, P, bob),   # hasFather ⊑ hasParent
        (alice, A, bob),   # ⊑ hasAncestor
        (bob, P, carol),   # asserted
        (bob, A, carol),   # hasParent ⊑ hasAncestor
        (alice, A, carol), # transitivity of hasAncestor
    }
    missing = expected - got
    assert not missing, f"missing entailed assertions: {missing}"


def test_materialize_object_property_assertions_no_false_positives(fixtures_dir):
    fixture = fixtures_dir / "abox" / "property_materialization.ofn"
    got = {
        (s, p, o)
        for (s, p, o) in rustdl.materialize_inferred_object_property_assertions(str(fixture))
    }
    F = "http://t/hasFather"
    alice, bob, carol = "http://t/alice", "http://t/bob", "http://t/carol"
    # hasFather is only asserted alice->bob; nothing entails the reverse
    # or a hasFather edge to carol.
    assert (bob, F, alice) not in got
    assert (alice, F, carol) not in got
    # No probe/internal IRIs should leak into the output.
    assert all(not p.startswith("urn:rustdl:") for (_, p, _) in got)

import os, rustdl
SEED = os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")

def test_seed_is_clean():
    c = rustdl.classify(SEED)
    assert c.inconsistent is False
    assert c.unsatisfiable == []

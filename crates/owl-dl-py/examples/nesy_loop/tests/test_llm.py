import pytest
from nesy_loop.llm import ScriptedLLM

def test_scripted_returns_in_order():
    m = ScriptedLLM(["a", "b"])
    assert m.propose("x") == "a"
    assert m.propose("y") == "b"

def test_scripted_exhausted_raises():
    m = ScriptedLLM(["only"])
    m.propose("x")
    with pytest.raises(IndexError):
        m.propose("y")

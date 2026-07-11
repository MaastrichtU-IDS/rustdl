import pytest
from nesy_loop.llm import ScriptedLLM, _first_text_block

def test_scripted_returns_in_order():
    m = ScriptedLLM(["a", "b"])
    assert m.propose("x") == "a"
    assert m.propose("y") == "b"

def test_scripted_exhausted_raises():
    m = ScriptedLLM(["only"])
    m.propose("x")
    with pytest.raises(IndexError):
        m.propose("y")


class _Blk:
    def __init__(self, type, text=None): self.type = type; self.text = text

def test_first_text_block_skips_non_text():
    assert _first_text_block([_Blk("thinking"), _Blk("text", " hi ")]) == " hi "

def test_first_text_block_no_text_raises():
    with pytest.raises(ValueError):
        _first_text_block([_Blk("thinking")])

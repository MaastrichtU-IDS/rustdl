"""Diagnosis/Root/etc. result-object mechanics (no native needed)."""
import json

from rustdl import Diagnosis, Root, Derived, Inconsistency


def _sample():
    return Diagnosis(
        consistent=True,
        unsatisfiable=("urn:Bad",),
        roots=(Root(iri="urn:Bad", justification=("Bad ⊑ ⊥",), repairs=(("ax",),), derives=("urn:Sub",)),),
        derived=(Derived(iri="urn:Sub", roots=("urn:Bad",)),),
        inconsistency=None,
    )


def test_attribute_access():
    d = _sample()
    assert d.consistent is True
    assert d.roots[0].iri == "urn:Bad"
    assert d.roots[0].justification == ("Bad ⊑ ⊥",)
    assert d.derived[0].roots == ("urn:Bad",)


def test_dict_compat():
    d = _sample()
    assert d["consistent"] is True
    assert d["roots"][0]["justification"] == ("Bad ⊑ ⊥",)
    assert "unsatisfiable" in d
    assert "inconsistency" not in d
    import pytest
    with pytest.raises(KeyError):
        _ = d["inconsistency"]
    assert set(dict(d)) == {"consistent", "unsatisfiable", "roots", "derived"}


def test_to_dict_json():
    d = _sample()
    js = json.loads(json.dumps(d.to_dict()))
    assert js["roots"][0]["justification"] == ["Bad ⊑ ⊥"]   # tuples → lists
    assert "inconsistency" not in js


def test_inconsistent_shape():
    di = Diagnosis(consistent=False, unsatisfiable=(), roots=(), derived=(),
                   inconsistency=Inconsistency(justification=("a",), repairs=(("b",),)))
    assert "inconsistency" in di
    assert di.inconsistency.justification == ("a",)
    assert di["inconsistency"]["repairs"] == (("b",),)
    assert json.dumps(di.to_dict())


def test_frozen():
    import pytest
    d = _sample()
    with pytest.raises(Exception):
        d.roots[0].iri = "x"  # frozen

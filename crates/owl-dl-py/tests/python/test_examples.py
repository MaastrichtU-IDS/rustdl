"""Bundled example ontologies — must work fully offline (shipped in the wheel)."""
import os
import rustdl
from rustdl import examples


def test_all_three_examples_materialize():
    for fn in (examples.pizza, examples.sulo, examples.sio):
        path = fn()
        assert os.path.exists(path)
        assert os.path.getsize(path) > 0


def test_pizza_classifies_complete():
    # ontostart pizza: 88 classes, no unsat, classifies instantly + complete.
    r = rustdl.classify(examples.pizza())
    assert len(r.classes) == 88
    assert len(r.unsatisfiable) == 0
    assert r.complete is True
    ns = examples.PIZZA_NS
    assert r.is_subclass(ns + "BoxedPizza", ns + "Pizza")
    # cross-ontology: aligned to SULO upper ontology
    assert r.is_subclass(ns + "BakingStartTime", examples.SULO_NS + "StartTime")


def test_sulo_classifies():
    r = rustdl.classify(examples.sulo())
    assert len(r.classes) == 17
    assert r.complete is True


def test_namespace_constants_are_strings():
    for ns in (examples.PIZZA_NS, examples.SULO_NS, examples.SIO_NS):
        assert isinstance(ns, str) and ns

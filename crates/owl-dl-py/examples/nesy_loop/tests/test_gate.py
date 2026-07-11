import os
from nesy_loop import gate
SEED = open(os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")).read()
P = "http://ex.org/pizza#"

def test_apply_edit_inserts_before_close():
    out = gate.apply_edit(SEED, "SubClassOf(:A :B)")
    assert out.count("Ontology(") == 1
    assert out.rstrip().endswith(")")
    assert "SubClassOf(:A :B)" in out

def test_good_edit_passes(tmp_path):
    r = gate.check(SEED, "SubClassOf(:Tomato :Topping)", set(), str(tmp_path))
    assert r.ok and r.parse_error is None and r.new_unsat == []

def test_bad_edit_caught_with_justification_and_repair(tmp_path):
    axiom = "SubClassOf(:CheeseAndVeg :CheeseTopping) SubClassOf(:CheeseAndVeg :VegetableTopping) Declaration(Class(:CheeseAndVeg))"
    r = gate.check(SEED, axiom, set(), str(tmp_path))
    assert r.ok is False
    assert f"{P}CheeseAndVeg" in r.new_unsat
    assert any("Disjoint" in a for a in r.justification)
    assert len(r.repairs) >= 1

def test_parse_error_reported(tmp_path):
    r = gate.check(SEED, "this is not an axiom", set(), str(tmp_path))
    assert r.ok is False and r.parse_error is not None

def test_feedback_mentions_class_and_fix(tmp_path):
    axiom = "Declaration(Class(:X)) SubClassOf(:X :CheeseTopping) SubClassOf(:X :VegetableTopping)"
    r = gate.check(SEED, axiom, set(), str(tmp_path))
    fb = gate.format_feedback(r)
    assert "unsatisfiable" in fb.lower() and "X" in fb

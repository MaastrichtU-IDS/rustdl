import os
from nesy_loop import loop
from nesy_loop.llm import ScriptedLLM
SEED = open(os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")).read()

def test_good_edit_accepted_first_try(tmp_path):
    llm = ScriptedLLM(["SubClassOf(:Tomato :Topping)"])
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=2, workdir=str(tmp_path))
    assert r.proposed == 1 and r.clashes_caught == 0
    assert r.turns[-1].accepted is True and r.final_unsat == 0

def test_bad_then_repaired(tmp_path):
    # First proposal clashes (subclass of both disjoint); revision fixes it.
    bad = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping) SubClassOf(:Q :VegetableTopping)"
    good = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping)"
    llm = ScriptedLLM([bad, good])
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=2, workdir=str(tmp_path))
    assert r.clashes_caught == 1 and r.fixed_after_repair == 1
    assert r.turns[0].accepted is False and r.turns[1].accepted is True
    assert r.turns[0].feedback and "unsatisfiable" in r.turns[0].feedback.lower()
    assert r.turns[0].rejection == "clash"
    assert r.final_unsat == 0

def test_unrepaired_clash_counts(tmp_path):
    bad = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping) SubClassOf(:Q :VegetableTopping)"
    llm = ScriptedLLM([bad, bad])  # never fixes
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=1, workdir=str(tmp_path))
    assert r.clashes_caught == 1 and r.fixed_after_repair == 0
    assert r.turns[-1].accepted is False

def test_malformed_then_good(tmp_path):
    llm = ScriptedLLM(["this is not an axiom", "SubClassOf(:Tomato :Topping)"])
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=2, workdir=str(tmp_path))
    assert r.malformed == 1
    assert r.clashes_caught == 0
    assert r.fixed_after_repair == 0
    assert r.turns[0].rejection == "parse"
    assert r.turns[-1].accepted is True
    assert r.final_unsat == 0

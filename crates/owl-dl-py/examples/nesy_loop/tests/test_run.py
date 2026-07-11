import json, os
from nesy_loop import run
from nesy_loop.loop import LoopResult, Turn

def test_write_transcript_jsonl(tmp_path):
    res = LoopResult(turns=[Turn(0, 0, "SubClassOf(:A :B)", True, None)], proposed=1)
    p = os.path.join(tmp_path, "t.jsonl")
    run.write_transcript(res, p)
    rows = [json.loads(l) for l in open(p)]
    assert rows[0]["axiom"] == "SubClassOf(:A :B)" and rows[0]["accepted"] is True

def test_metrics_markdown_has_counts():
    res = LoopResult(proposed=5, clashes_caught=2, fixed_after_repair=2, malformed=1, final_unsat=0)
    md = run.metrics_markdown(res)
    assert "| 5 |" in md and "clashes caught" in md.lower() and "malformed" in md.lower()

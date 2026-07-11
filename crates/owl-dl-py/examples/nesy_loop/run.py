from __future__ import annotations
import argparse, json, os, sys
from dataclasses import asdict
from .loop import run_loop, LoopResult
from .llm import ScriptedLLM, AnthropicLLM, LocalLLM


def write_transcript(res: LoopResult, path: str) -> None:
    with open(path, "w") as f:
        for t in res.turns:
            f.write(json.dumps(asdict(t)) + "\n")


def metrics_markdown(res: LoopResult) -> str:
    # Counters are per-edit-slot, not a partition of `proposed`: a slot can
    # contribute to both "edits with a clash" and "edits malformed" (see
    # loop.py), and `final_unsat` is a structural invariant (always 0, since
    # only clash-free edits are ever accepted into the ontology).
    return (
        "| edits proposed | edits with a clash | fixed after repair | edits malformed | residual new-unsat |\n"
        "|---:|---:|---:|---:|---:|\n"
        f"| {res.proposed} | {res.clashes_caught} | {res.fixed_after_repair} | {res.malformed} | {res.final_unsat} |\n"
    )


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", default=os.path.join(os.path.dirname(__file__), "fixtures", "seed.ofn"))
    ap.add_argument("--n-edits", type=int, default=8)
    ap.add_argument("--max-revisions", type=int, default=2)
    ap.add_argument("--scripted", default=None, help="JSON file with a list of replies (offline)")
    ap.add_argument("--local", action="store_true", help="use a local OpenAI-compatible endpoint (e.g. Ollama)")
    ap.add_argument("--model", default=None, help="model id (local or Anthropic)")
    ap.add_argument("--base-url", default="http://localhost:11434/v1", help="local endpoint base URL")
    ap.add_argument("--temperature", type=float, default=0.4, help="sampling temperature for --local")
    ap.add_argument("--task", default=None, help="override the per-edit instruction given to the LLM")
    ap.add_argument("--out", default="out")
    a = ap.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)
    with open(a.seed) as f:
        base = f.read()
    if a.scripted:
        with open(a.scripted) as f:
            llm = ScriptedLLM(json.load(f))
    elif a.local:
        llm = LocalLLM(model=a.model or "qwen2.5:32b-instruct",
                       base_url=a.base_url, temperature=a.temperature)
    else:
        llm = AnthropicLLM(model=a.model) if a.model else AnthropicLLM()
    res = run_loop(base, llm, a.n_edits, a.max_revisions, a.out, task=a.task)
    write_transcript(res, os.path.join(a.out, "transcript.jsonl"))
    with open(os.path.join(a.out, "metrics.md"), "w") as f:
        f.write(metrics_markdown(res))
    print(metrics_markdown(res))
    return 0


if __name__ == "__main__":
    sys.exit(main())

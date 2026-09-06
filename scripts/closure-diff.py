#!/usr/bin/env python3
"""Transitive-closure diff between two `rustdl classify --json` outputs.

Why this exists (see #110 fix-wave review, Finding 4): the corpus sweep's
headline "0 lost entailments" (docs/benchmarks/2026-09-06-conjunctive-filler-sweep.md
§8) was produced by a comparator that lived only in a scratchpad, so the method
claim was unverifiable from the repo record. This is that comparator, committed,
alongside the instrument for the sweep's other half
(`crates/owl-dl-reasoner/examples/fragment_probe.rs`).

`direct_subsumptions` in `classify --json` is the Hasse (direct-parent)
relation, not the full closure — see CLAUDE.md's "direct-vs-closure trap"
(docs/2026-08-XX and MEMORY.md `direct-vs-closure-trap.md`): reasoning progress
RESTRUCTURES that relation (proving a class unsat elides its rows; proving two
classes equivalent collapses them), it does not just extend it. So the only
valid comparison is over the full transitive closure, and `equivalent_groups`
must be expanded into real graph edges before computing it — an equivalence
group is a CYCLE (every member subsumes every other), and this project has
already shipped one false "3 lost entailments" finding on `ore_ont_778` from a
comparator that memoised an `ancestors(x)` result under a cycle-detection
stack: on a cycle, that memoisation returns a WRONG (too-small) answer for
whichever node is visited first, and then silently reuses that wrong answer
for every later node that shares part of the cycle.

The fix here is structural, not a patched special case: every closure query is
a FRESH, unmemoised BFS from that one node. Nothing computed for node A is ever
reused when computing node B's closure, so there is no stale partial result to
leak across a cycle. `--self-test` demonstrates this directly by also running
the WRONG (memoised, cycle-detecting) algorithm on the same fixture and
asserting the two disagree — i.e. it does not merely assert this script's
answer is self-consistent, it shows the naive alternative is measurably wrong
on exactly the input shape (a cycle) this script exists to get right.

Usage:
    python3 scripts/closure-diff.py before.json after.json
    python3 scripts/closure-diff.py --self-test
"""

from __future__ import annotations

import collections
import json
import sys
from typing import Dict, Iterable, List, Set, Tuple

Graph = Dict[str, Set[str]]
Pair = Tuple[str, str]


# ---------------------------------------------------------------------------
# Graph construction
# ---------------------------------------------------------------------------


def build_graph(data: dict) -> Tuple[Graph, Set[str]]:
    """Build a directed subsumption graph from one `classify --json` payload.

    Edge `x -> y` means "x is (directly or, after expansion, transitively via
    an equivalence cycle) known to be subsumed by y". Two sources of edges:

    * `direct_subsumptions`: the Hasse relation, `[sub, sup]` pairs — direct
      edge `sub -> sup`.
    * `equivalent_groups`: each group is expanded into a full cycle (every
      ordered pair within the group gets an edge both ways), because
      `EquivalentClasses(A, B, C)` means each of A, B, C subsumes the other
      two. This is the expansion step the docstring above calls load-bearing:
      skipping it, or approximating a group as "connected but not a clique",
      under-counts exactly the way the historical `ore_ont_778` bug did.
    """
    adj: Graph = collections.defaultdict(set)
    nodes: Set[str] = set()

    for pair in data.get("direct_subsumptions", []):
        sub, sup = pair[0], pair[1]
        adj[sub].add(sup)
        nodes.add(sub)
        nodes.add(sup)

    for group in data.get("equivalent_groups", []):
        members = list(group)
        for a in members:
            nodes.add(a)
        for a in members:
            for b in members:
                if a != b:
                    adj[a].add(b)

    for cls in data.get("unsatisfiable", []):
        nodes.add(cls)

    return adj, nodes


# ---------------------------------------------------------------------------
# Closure computation — fresh per-node BFS, NO shared memoisation
# ---------------------------------------------------------------------------


def bfs_closure(start: str, adj: Graph) -> Set[str]:
    """Every node reachable from `start` via `adj`, excluding `start` itself.

    Deliberately allocates a fresh `visited` set on every call and shares no
    state with any other call. This is what makes it correct on a cycle: a
    node partway through a cycle is simply visited again and skipped (it is
    already in *this* call's `visited`), never short-circuited by a result
    computed for a different starting node.
    """
    visited: Set[str] = set()
    queue = collections.deque(adj.get(start, ()))
    while queue:
        n = queue.popleft()
        if n == start or n in visited:
            continue
        visited.add(n)
        queue.extend(adj.get(n, ()))
    return visited


def closure_pairs(adj: Graph, nodes: Iterable[str]) -> Set[Pair]:
    """All entailed `(x, y)` pairs, `x != y`, over the whole graph."""
    pairs: Set[Pair] = set()
    for x in nodes:
        for y in bfs_closure(x, adj):
            pairs.add((x, y))
    return pairs


# ---------------------------------------------------------------------------
# The WRONG algorithm, kept only so --self-test can show it disagrees.
# ---------------------------------------------------------------------------


def _buggy_memoized_ancestors(
    node: str, adj: Graph, memo: Dict[str, Set[str]], stack: Set[str]
) -> Set[str]:
    """Deliberately WRONG: a DFS that memoises per-node results GLOBALLY
    (across different starting nodes) and treats hitting a node already on
    the current recursion stack as "a cycle, nothing more to add here".

    This reproduces the historical `ore_ont_778` defect: the first node
    processed inside a cycle gets a memoised result computed WHILE its
    cycle-mates were still open (so it silently misses whatever those
    cycle-mates would have contributed), and every later query for a node
    inside that cycle reuses the stale, too-small memoised value instead of
    recomputing. `--self-test` calls this ONLY to demonstrate the disagreement
    — nothing in the real comparator (`closure_pairs`/`bfs_closure` above)
    shares memoisation across nodes.
    """
    if node in memo:
        return memo[node]
    if node in stack:
        return set()
    stack.add(node)
    result: Set[str] = set()
    for succ in adj.get(node, ()):
        result.add(succ)
        result |= _buggy_memoized_ancestors(succ, adj, memo, stack)
    stack.discard(node)
    memo[node] = result
    return result


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def load_closure_json(path: str) -> dict:
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def diff(before: dict, after: dict) -> int:
    before_adj, before_nodes = build_graph(before)
    after_adj, after_nodes = build_graph(after)

    before_pairs = closure_pairs(before_adj, before_nodes)
    after_pairs = closure_pairs(after_adj, after_nodes)

    gained = sorted(after_pairs - before_pairs)
    lost = sorted(before_pairs - after_pairs)

    print(f"before closure: {len(before_pairs)} pairs "
          f"({len(before.get('unsatisfiable', []))} unsatisfiable, "
          f"{len(before.get('equivalent_groups', []))} equivalent groups)")
    print(f"after  closure: {len(after_pairs)} pairs "
          f"({len(after.get('unsatisfiable', []))} unsatisfiable, "
          f"{len(after.get('equivalent_groups', []))} equivalent groups)")
    print(f"gained: {len(gained)}")
    print(f"lost:   {len(lost)}")

    max_show = 50
    if gained:
        print("\n-- gained pairs (sub, sup) --")
        for sub, sup in gained[:max_show]:
            print(f"  + {sub}  ⊑  {sup}")
        if len(gained) > max_show:
            print(f"  ... ({len(gained) - max_show} more)")
    if lost:
        print("\n-- lost pairs (sub, sup) --")
        for sub, sup in lost[:max_show]:
            print(f"  - {sub}  ⊑  {sup}")
        if len(lost) > max_show:
            print(f"  ... ({len(lost) - max_show} more)")

    return 0


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def _self_test() -> int:
    ok = True

    # --- Test 1: a directed 3-cycle plus a tail, then extending the tail. ---
    # A -> B -> C -> A (a genuine cycle, e.g. what an equivalent_groups
    # expansion of {A,B,C} would produce), plus C -> D.
    #
    # Hand-computed (excluding self):
    #   closure(A) = {B, C, D}  (A->B->C->A revisits A, excluded; C->D)  = 3
    #   closure(B) = {C, A, D}                                          = 3
    #   closure(C) = {A, B, D}                                          = 3
    #   closure(D) = {}                                                  = 0
    #   total = 9
    before1 = {
        "unsatisfiable": [],
        "equivalent_groups": [],
        "direct_subsumptions": [["A", "B"], ["B", "C"], ["C", "A"], ["C", "D"]],
    }
    adj1, nodes1 = build_graph(before1)
    pairs1 = closure_pairs(adj1, nodes1)
    if len(pairs1) != 9:
        print(f"SELF-TEST FAIL: 3-cycle+tail closure = {len(pairs1)}, expected 9")
        ok = False
    else:
        print("SELF-TEST PASS: 3-cycle (A->B->C->A) + C->D closure = 9")

    # Extend the tail: D -> E. Every node that already reached D now also
    # reaches E (+1 each for A, B, C, D), and E reaches nothing new.
    #   closure(A..C) each +1 = 4 each (3 nodes -> +3)
    #   closure(D) = {E} = 1 (+1)
    #   closure(E) = {} = 0
    #   total = 13, gained = 4, lost = 0
    after1 = {
        "unsatisfiable": [],
        "equivalent_groups": [],
        "direct_subsumptions": [
            ["A", "B"], ["B", "C"], ["C", "A"], ["C", "D"], ["D", "E"],
        ],
    }
    adj1b, nodes1b = build_graph(after1)
    pairs1b = closure_pairs(adj1b, nodes1b)
    gained1 = pairs1b - pairs1
    lost1 = pairs1 - pairs1b
    if len(pairs1b) != 13 or len(gained1) != 4 or len(lost1) != 0:
        print(
            f"SELF-TEST FAIL: after adding D->E, closure={len(pairs1b)} "
            f"(expected 13), gained={len(gained1)} (expected 4), "
            f"lost={len(lost1)} (expected 0)"
        )
        ok = False
    else:
        print("SELF-TEST PASS: adding D->E: closure 9 -> 13, gained 4, lost 0")

    # --- Test 2: an equivalence group is a cycle, and only a FRESH per-node
    # BFS gets every member's closure right; a globally-memoised DFS with a
    # cycle-detection stack gets it wrong. ---
    #
    # equivalent_groups=[["P", "Q"]] plus P ⊑ X ⊑ Y.
    # Expanded graph: P->Q, Q->P, P->X, X->Y.
    #
    # Hand-computed correct closure (excluding self):
    #   closure(P) = {Q, X, Y} = 3
    #   closure(Q) = {P, X, Y} = 3   <- Q inherits X, Y ONLY via the cycle
    #   closure(X) = {Y}       = 1
    #   closure(Y) = {}        = 0
    #   total = 7
    equiv_fixture = {
        "unsatisfiable": [],
        "equivalent_groups": [["P", "Q"]],
        "direct_subsumptions": [["P", "X"], ["X", "Y"]],
    }
    adj2, nodes2 = build_graph(equiv_fixture)
    pairs2 = closure_pairs(adj2, nodes2)
    if len(pairs2) != 7:
        print(f"SELF-TEST FAIL: equivalence-cycle closure = {len(pairs2)}, expected 7")
        ok = False
    else:
        print("SELF-TEST PASS: equivalence group {P,Q} + P⊑X⊑Y closure = 7")

    q_closure = bfs_closure("Q", adj2)
    if q_closure != {"P", "X", "Y"}:
        print(f"SELF-TEST FAIL: closure(Q) = {q_closure}, expected {{P, X, Y}}")
        ok = False
    else:
        print("SELF-TEST PASS: closure(Q) = {P, X, Y} (inherited via the cycle)")

    # Now run the deliberately-WRONG globally-memoised DFS over the SAME
    # graph, in node order P, Q, X, Y, and show it disagrees with the correct
    # per-node closure — specifically that it UNDER-counts Q, because Q's
    # true ancestors were computed while P's DFS frame (part of the same
    # cycle) was still open on the stack.
    memo: Dict[str, Set[str]] = {}
    buggy = {n: _buggy_memoized_ancestors(n, adj2, memo, set()) for n in ["P", "Q", "X", "Y"]}
    buggy_total = sum(len(v) for v in buggy.values())
    correct_per_node = {n: len(bfs_closure(n, adj2)) for n in ["P", "Q", "X", "Y"]}
    if buggy_total == sum(correct_per_node.values()) and buggy["Q"] == {"P", "X", "Y"}:
        print(
            "SELF-TEST FAIL: the buggy memoised algorithm was expected to "
            "disagree with the correct one on this cycle, but it did not — "
            "the demonstration fixture no longer exercises the bug"
        )
        ok = False
    else:
        print(
            f"SELF-TEST PASS: buggy globally-memoised DFS disagrees with the "
            f"correct per-node BFS on the equivalence cycle "
            f"(buggy closure(Q)={sorted(buggy['Q'])}, "
            f"correct closure(Q)={sorted(bfs_closure('Q', adj2))}) — "
            f"this is why closure_pairs() never shares state across nodes"
        )

    if ok:
        print("\nself-test: ALL PASS")
        return 0
    print("\nself-test: FAILURES ABOVE")
    return 1


def main(argv: List[str]) -> int:
    if "--self-test" in argv:
        return _self_test()
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    before = load_closure_json(argv[1])
    after = load_closure_json(argv[2])
    return diff(before, after)


if __name__ == "__main__":
    sys.exit(main(sys.argv))

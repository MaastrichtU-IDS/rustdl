#!/usr/bin/env python3
"""closure-diff.py — transitive-closure comparator for `rustdl classify --json`.

Takes two `classify --json` output files (e.g. a BEFORE and AFTER run) and
reports:
  - the size of each side's full transitive-closure subsumption set
  - which (sub, sup) pairs were GAINED (in AFTER but not BEFORE)
  - which (sub, sup) pairs were LOST (in BEFORE but not AFTER)
  - unsatisfiable-class and equivalent-group diffs, as a sanity cross-check

Why this exists (read before trusting any output): `direct_subsumptions` in
`classify --json` is the Hasse (direct-parent) relation, not the full closure
-- and reasoning progress RESTRUCTURES it rather than only extending it:
proving a class unsatisfiable ELIDES its rows, proving classes equivalent
COLLAPSES them into one node. A naive line-count or set-diff over
`direct_subsumptions` therefore reads a *correct* improvement as a regression
just as often as a real one (see CLAUDE.md's "direct-vs-closure trap", hit
four times in this repo's history). This script computes the actual
transitive closure so a diff means what it says.

THE BUG THIS SCRIPT IS DESIGNED NOT TO REPRODUCE, stated plainly because it
already happened once in this repo: `equivalent_groups` are CYCLES (every
member mutually subsumes every other), and a transitive-closure computation
that memoises `ancestors(x)` behind a "currently visiting -> return empty"
cycle guard can cache an INCOMPLETE result for one member of an equivalence
group while still mid-cycle, and that incomplete memo then poisons every
later, unrelated query that reuses it -- silently under-counting entailments
for every group member except the one whose own direct edges happened to
reach the missing subsumer first. This produced a false "3 lost entailments"
finding (then "4 lost") on a real ontology in this project's history; a
correct per-node BFS on the same input gives the true answer, `lost = 0`.
`--self-test` reproduces the bug mechanism on a small fixture and shows the
buggy and correct algorithms disagreeing, so this instrument validates
itself rather than merely asserting it is correct.

The fix, applied throughout this file: every closure query starts a FRESH
BFS with its own local visited-set. No ancestor set is ever cached and reused
across different starting nodes -- the only "memoisation" is the adjacency
list itself (which is exact, not derived), so there is no stale partial
result to poison a later query.

Usage:
    python3 scripts/closure-diff.py before.json after.json
    python3 scripts/closure-diff.py --self-test
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Dict, FrozenSet, Iterable, List, Set, Tuple

Pair = Tuple[str, str]
Graph = Dict[str, Set[str]]

OWL_THING = "http://www.w3.org/2002/07/owl#Thing"
OWL_NOTHING = "http://www.w3.org/2002/07/owl#Nothing"


# ---------------------------------------------------------------------------
# Graph construction
# ---------------------------------------------------------------------------


def build_adjacency(
    direct_subsumptions: Iterable[Iterable[str]],
    equivalent_groups: Iterable[Iterable[str]],
) -> Graph:
    """Build the direct-edge adjacency (sub -> {sup, ...}).

    `direct_subsumptions` contributes one directed edge per row.
    `equivalent_groups` each contribute a CLIQUE: every ordered pair (a, b)
    with a != b in the group gets a bidirectional edge, so the closure
    correctly threads through the group in both directions and any chain
    that passes through it. This is the load-bearing requirement the task
    names explicitly -- a group is not a single representative node, it is
    every member mutually entailing every other.
    """
    adj: Graph = {}

    def add_edge(a: str, b: str) -> None:
        if a == b:
            return
        adj.setdefault(a, set()).add(b)
        adj.setdefault(b, set())  # ensure b exists as a node even if it has no out-edges

    for row in direct_subsumptions:
        sub, sup = row[0], row[1]
        add_edge(sub, sup)

    for group in equivalent_groups:
        members = list(group)
        for a in members:
            for b in members:
                if a != b:
                    add_edge(a, b)

    return adj


# ---------------------------------------------------------------------------
# Correct closure: a FRESH per-node BFS, no shared memoisation
# ---------------------------------------------------------------------------


def ancestors_bfs(start: str, adj: Graph) -> Set[str]:
    """All nodes reachable from `start` via the adjacency, excluding `start`
    itself. A plain BFS/DFS reachability walk handles cycles correctly by
    construction: `visited` is local to this call, so a node is expanded
    exactly once regardless of how many cycles pass through it, and there is
    no cross-call cache to poison a later, unrelated query.
    """
    visited: Set[str] = {start}
    queue: List[str] = [start]
    out: Set[str] = set()
    while queue:
        node = queue.pop()
        for nxt in adj.get(node, ()):
            if nxt not in visited:
                visited.add(nxt)
                out.add(nxt)
                queue.append(nxt)
    return out


def transitive_closure(adj: Graph) -> Set[Pair]:
    """The full transitive closure as a set of (sub, sup) pairs, sub != sup."""
    out: Set[Pair] = set()
    for node in adj:
        for anc in ancestors_bfs(node, adj):
            out.add((node, anc))
    return out


# ---------------------------------------------------------------------------
# The buggy alternative, kept ONLY so `--self-test` can demonstrate the
# mechanism this script avoids. Never used for a real diff.
# ---------------------------------------------------------------------------


def _buggy_memoized_ancestors_all(adj: Graph) -> Dict[str, Set[str]]:
    """A plausible-looking but WRONG closure computation: a recursive
    `ancestors(x)` memoised across ALL top-level queries, which gives up
    (returns the empty set) the moment it revisits a node already on the
    current call stack ("cycle detected") -- and then CACHES whatever it
    computed under that truncation. A later, independent query for a
    different node can reuse that cached, incomplete result verbatim.

    This is the historical bug, reproduced deliberately for `--self-test`.
    Do not import or reuse this function outside of the self-test.
    """
    memo: Dict[str, Set[str]] = {}

    def anc(node: str, stack: Set[str]) -> Set[str]:
        if node in memo:
            return memo[node]
        if node in stack:
            # "cycle detected" -- give up on this branch. This is the bug:
            # the caller has no way to know this answer is incomplete, and
            # if `node`'s own top-level result gets memoised while some
            # branch of its computation returned early like this, the
            # memoised value is silently wrong.
            return set()
        stack = stack | {node}
        result: Set[str] = set()
        for nxt in sorted(adj.get(node, ())):
            result.add(nxt)
            result |= anc(nxt, stack)
        memo[node] = result
        return result

    for node in sorted(adj):
        anc(node, set())
    return memo


# ---------------------------------------------------------------------------
# classify --json loading
# ---------------------------------------------------------------------------


class ClassifyResult:
    def __init__(self, path: str, data: dict):
        self.path = path
        self.consistent: bool = data.get("consistent", True)
        self.incomplete: bool = data.get("incomplete", False)
        self.unsatisfiable: Set[str] = set(data.get("unsatisfiable", []))
        self.equivalent_groups: List[List[str]] = [list(g) for g in data.get("equivalent_groups", [])]
        self.direct_subsumptions: List[List[str]] = [list(r) for r in data.get("direct_subsumptions", [])]
        self.adjacency: Graph = build_adjacency(self.direct_subsumptions, self.equivalent_groups)
        self.closure: Set[Pair] = transitive_closure(self.adjacency)

    def equivalent_group_key(self) -> Set[FrozenSet[str]]:
        return {frozenset(g) for g in self.equivalent_groups}


def load_classify_json(path: str) -> ClassifyResult:
    with open(path, "r", encoding="utf-8") as fh:
        data = json.load(fh)
    return ClassifyResult(path, data)


# ---------------------------------------------------------------------------
# Diff reporting
# ---------------------------------------------------------------------------


def format_pairs(pairs: Iterable[Pair], limit: int = 20) -> str:
    pairs = sorted(pairs)
    shown = pairs[:limit]
    lines = [f"    {sub}  ⊑  {sup}" for sub, sup in shown]
    if len(pairs) > limit:
        lines.append(f"    ... and {len(pairs) - limit} more")
    return "\n".join(lines) if lines else "    (none)"


def diff(before_path: str, after_path: str) -> int:
    before = load_classify_json(before_path)
    after = load_classify_json(after_path)

    gained = after.closure - before.closure
    lost = before.closure - after.closure

    print(f"before: {before_path}")
    print(f"  closure size: {len(before.closure)}")
    print(f"  unsatisfiable: {len(before.unsatisfiable)}")
    print(f"  equivalent groups: {len(before.equivalent_groups)}")
    print(f"  consistent={before.consistent} incomplete={before.incomplete}")
    print(f"after:  {after_path}")
    print(f"  closure size: {len(after.closure)}")
    print(f"  unsatisfiable: {len(after.unsatisfiable)}")
    print(f"  equivalent groups: {len(after.equivalent_groups)}")
    print(f"  consistent={after.consistent} incomplete={after.incomplete}")
    print()
    print(f"gained (in after, not before): {len(gained)}")
    print(format_pairs(gained))
    print(f"lost (in before, not after): {len(lost)}")
    print(format_pairs(lost))

    unsat_gained = after.unsatisfiable - before.unsatisfiable
    unsat_lost = before.unsatisfiable - after.unsatisfiable
    if unsat_gained or unsat_lost:
        print()
        print(f"unsatisfiable gained: {sorted(unsat_gained)}")
        print(f"unsatisfiable lost:   {sorted(unsat_lost)}")

    eq_before = before.equivalent_group_key()
    eq_after = after.equivalent_group_key()
    if eq_before != eq_after:
        print()
        print(f"equivalent groups only in before: {sorted(eq_before - eq_after)}")
        print(f"equivalent groups only in after:  {sorted(eq_after - eq_before)}")

    if lost:
        print()
        print(
            "NOTE: a 'lost' pair is not automatically a regression -- proving a class "
            "unsatisfiable or merging classes into a new equivalence group both "
            "legitimately remove pairs from the closure. Cross-check against "
            "unsatisfiable/equivalent-group diffs above before calling this a defect."
        )

    return 0


# ---------------------------------------------------------------------------
# --self-test
# ---------------------------------------------------------------------------


def self_test() -> int:
    # Fixture: a 5-member equivalence clique {G1..G5} (mirroring the real
    # ontology this bug was found on -- a 5-member equivalence group where
    # only ONE member carries the outgoing edge to something outside the
    # group) plus a single told edge G1 -> P. Every group member entails P
    # transitively via the clique, so the correct answer is that ALL of
    # G1..G5 have P in their ancestor set.
    members = ["G1", "G2", "G3", "G4", "G5"]
    direct_subsumptions = [["G1", "P"]]
    equivalent_groups = [members]
    adj = build_adjacency(direct_subsumptions, equivalent_groups)

    correct = transitive_closure(adj)
    expected = set()
    for m in members:
        for other in members:
            if m != other:
                expected.add((m, other))
        expected.add((m, "P"))
    assert correct == expected, (
        f"self-test FAILED: the correct (fresh-BFS) closure disagrees with the "
        f"hand-verified expected answer.\n  got:      {sorted(correct)}\n"
        f"  expected: {sorted(expected)}"
    )
    print("[ok] fresh-BFS closure matches the hand-verified expected answer "
          f"({len(correct)} pairs, every group member reaches P).")

    # Now run the buggy memoised-DFS-with-cycle-guard version and show it
    # disagrees -- specifically, it must fail to derive at least one
    # (member, P) pair for a member other than G1 itself (G1 always gets P
    # right because P is G1's own direct edge, explored before any recursive
    # call can memoise a truncated result for G1).
    buggy = _buggy_memoized_ancestors_all(adj)
    buggy_pairs: Set[Pair] = set()
    for node, ancs in buggy.items():
        for a in ancs:
            buggy_pairs.add((node, a))

    missing_from_buggy = correct - buggy_pairs
    p_misses = {(m, s) for (m, s) in missing_from_buggy if s == "P"}
    assert p_misses, (
        "self-test FAILED: expected the buggy memoized-DFS to lose at least one "
        "(member, P) entailment via the equivalence cycle, but it reproduced the "
        "correct closure -- the demonstration fixture no longer exercises the bug "
        "mechanism, or the buggy implementation was fixed and should be updated "
        "or removed."
    )
    print(
        f"[ok] the buggy memoized-DFS-with-cycle-guard DISAGREES with the correct "
        f"closure, missing {len(p_misses)} of the true (member, P) entailments via "
        f"the equivalence cycle: {sorted(p_misses)}"
    )
    print(
        "     (this is the same mechanism that produced a false '3 lost entailments' "
        "finding on a real ontology in this project's history -- see the module "
        "docstring)"
    )

    # And confirm the buggy version is not simply "always wrong" -- G1 itself
    # (whose direct edge to P is explored before any cycle truncation can
    # occur) must still be correct, which is what makes the failure mode
    # insidious (some entailments from the SAME axiom set are right, others
    # silently wrong, depending on iteration order).
    assert ("G1", "P") in buggy_pairs, (
        "self-test FAILED: expected G1 -> P to survive in the buggy version too "
        "(it is G1's own direct edge) -- if this fails the fixture needs revisiting"
    )
    print("[ok] G1 -> P (a direct, non-cyclic edge) is correct even in the buggy "
          "version -- confirming the failure is specific to cycle-truncated "
          "memoisation, not a wholesale bug.")

    print()
    print("self-test PASSED: the instrument validates itself.")
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("before", nargs="?", help="BEFORE classify --json file")
    parser.add_argument("after", nargs="?", help="AFTER classify --json file")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the built-in cyclic-fixture self-test and exit (no files needed)",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    if not args.before or not args.after:
        parser.error("both BEFORE and AFTER files are required unless --self-test is given")

    return diff(args.before, args.after)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

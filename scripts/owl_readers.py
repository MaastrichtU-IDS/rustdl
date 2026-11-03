"""Readers for peer reasoner output, shared by the laws harness and the generated net.

`cross-command-laws.py` and `scripts/gen/` both read Konclude's OWL/XML hierarchy and
robot/HermiT's functional-syntax output. One copy lives here so the two cannot drift.

Conventions every reader follows:
  * an unsat class is one equivalent to, or below, owl:Nothing, in every spelling
    (full IRI, the relative `Nothing` ROBOT writes against xml:base, `owl:Nothing`);
  * `owl:Thing` in the unsat set means the KB is inconsistent;
  * a class equivalent to owl:Thing is "Thing-equivalent";
  * closures are over named classes, with Thing/Nothing and reflexive pairs dropped.
Stdlib only.
"""
import re
from collections import defaultdict
from pathlib import Path

THING = "http://www.w3.org/2002/07/owl#Thing"
NOTHING = "http://www.w3.org/2002/07/owl#Nothing"
_THINGS = (THING, "Thing", "owl:Thing")
_NOTHINGS = (NOTHING, "Nothing", "owl:Nothing")


def is_thing(iri):
    return iri in _THINGS


def is_nothing(iri):
    return iri in _NOTHINGS


def closure(edges, equiv_groups=(), drop=()):
    """Transitive closure (fresh BFS per node: no shared memo, which is wrong on cycles
    and equivalence groups ARE cycles), minus reflexive, Thing/Nothing and `drop`."""
    g = defaultdict(set)
    for s, t in edges:
        g[s].add(t)
    for grp in equiv_groups:
        for a in grp:
            for b in grp:
                if a != b:
                    g[a].add(b)
    out = set()
    for s in list(g):
        seen, stack = set(), list(g[s])
        while stack:
            x = stack.pop()
            if x in seen:
                continue
            seen.add(x)
            stack.extend(g.get(x, ()))
        for t in seen:
            if t != s:
                out.add((s, t))
    bad = set(drop) | set(_THINGS) | set(_NOTHINGS)
    return {(s, t) for s, t in out if s not in bad and t not in bad}


def _norm(i):
    if is_thing(i):
        return THING
    if is_nothing(i):
        return NOTHING
    return i


class Hierarchy:
    """A peer's answer: consistency, unsat classes, Thing-equivalent classes, closure.

    `closure` excludes this answer's own unsat and Thing-equivalent classes. A diff
    between two answers must exclude the UNION of both sides' sets (see `pairs_without`).
    """

    def __init__(self, edges, groups, unsat):
        self.edges = [(_norm(s), _norm(t)) for s, t in edges]
        self.groups = [[_norm(m) for m in g] for g in groups]
        unsat = {_norm(u) for u in unsat}
        thing_equiv = set()
        for g in self.groups:
            if THING in g:
                thing_equiv |= set(g) - {THING}
        for s, t in self.edges:
            if s == THING and t != NOTHING:
                thing_equiv.add(t)  # Thing ⊑ C makes C Thing-equivalent
        self.consistent = THING not in unsat
        self.unsat = {u for u in unsat if u not in (THING, NOTHING)}
        self.thing_equiv = thing_equiv - {NOTHING} - self.unsat
        self._full = closure(self.edges, self.groups)

    def pairs_without(self, exclude):
        return {(s, t) for s, t in self._full if s not in exclude and t not in exclude}

    @property
    def closure(self):
        return self.pairs_without(self.unsat | self.thing_equiv)


def _split_unsat(edges, groups):
    """Route `C ⊑ Nothing` and Nothing-groups to unsat; the rest stays a hierarchy."""
    keep_e, keep_g, unsat = [], [], set()
    for s, t in edges:
        if is_nothing(t) and not is_nothing(s):
            unsat.add(_norm(s))
        else:
            keep_e.append((s, t))
    for g in groups:
        if any(is_nothing(m) for m in g):
            unsat |= {_norm(m) for m in g if not is_nothing(m)}
        else:
            keep_g.append(g)
    return keep_e, keep_g, unsat


def _owx_parts(text):
    edges, groups = [], []
    for kind, body in re.findall(r"<(SubClassOf|EquivalentClasses)>(.*?)</\1>", text, re.S):
        if re.search(r"<Object|<Data", body):
            continue
        iris = [a or b for a, b in re.findall(
            r'<Class\s+(?:IRI="([^"]*)"|abbreviatedIRI="([^"]*)")\s*/>', body)]
        if kind == "SubClassOf" and len(iris) == 2:
            edges.append(tuple(iris))
        elif kind == "EquivalentClasses" and len(iris) >= 2:
            groups.append(iris)
    return _split_unsat(edges, groups)


def owx_closure(path):
    """(closure, unsat) from an OWL/XML hierarchy (Konclude, or robot's OWX).

    The unsat set carries owl:Thing when the KB is inconsistent; the closure drops the
    unsat classes. Kept in this shape for cross-command-laws.py."""
    edges, groups, unsat = _owx_parts(Path(path).read_text(errors="replace"))
    return closure(edges, groups, drop=unsat), unsat


def owx_hierarchy(path):
    edges, groups, unsat = _owx_parts(Path(path).read_text(errors="replace"))
    return Hierarchy(edges, groups, unsat)


def owx_declared_classes(path):
    """Every class IRI the OWX mentions (declarations included)."""
    text = Path(path).read_text(errors="replace")
    return set(re.findall(r'<Class\s+IRI="([^"]*)"\s*/>', text))


_PFX = re.compile(r"Prefix\(\s*([\w.-]*):\s*=\s*<([^>]*)>\s*\)")
_TERM = r"(<[^>]*>|[\w.-]*:[^\s()]*)"


def _ofn_parts(text):
    pfx = {p: i for p, i in _PFX.findall(text)}

    def iri(term):
        if term.startswith("<"):
            return term[1:-1]
        p, _, local = term.partition(":")
        return pfx[p] + local if p in pfx else (term if p == "owl" else None)

    edges, groups = [], []
    for a, b in re.findall(rf"SubClassOf\(\s*{_TERM}\s+{_TERM}\s*\)", text):
        x, y = iri(a), iri(b)
        if x and y:
            edges.append((x, y))
    for body in re.findall(r"EquivalentClasses\(([^()]*)\)", text):
        members = [iri(m) for m in re.findall(_TERM, body)]
        if None in members or len(members) < 2:
            continue
        groups.append(members)
    return _split_unsat(edges, groups)


def ofn_named_closure(text):
    """(closure, unsat) from an OFN document (robot's output uses prefixed names;
    HermiT's `-c` output uses full IRIs). Kept in this shape for cross-command-laws.py."""
    edges, groups, unsat = _ofn_parts(text)
    return closure(edges, groups, drop=unsat), unsat


def ofn_hierarchy(text):
    edges, groups, unsat = _ofn_parts(text)
    return Hierarchy(edges, groups, unsat)

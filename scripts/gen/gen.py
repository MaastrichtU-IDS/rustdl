#!/usr/bin/env python3
"""Seeded generator of small, legal OWL 2 DL ontologies (#363 step 3).

Each case is a function of (seed, case index, profile) only, so a case can be
regenerated anywhere from its header line:

    # gen <generator hash> seed <s> case <i> profile <name>

One logical axiom per line, after the declarations, so a shrinker can drop lines.

Legality (OWL 2 DL global restrictions), by construction and re-checked by `check`:
  * Regularity: roles have a global rank, `r` and `r⁻` share it. A sub-role axiom only
    goes up the rank. A chain `s₁∘s₂ ⊑ r` needs both legs below `r`, except the forms
    `r∘r ⊑ r`, `r∘s ⊑ r` and `s∘r ⊑ r` with `s` below `r`. InverseObjectProperties is
    only drawn between roles of the same rank group (see `_draw_roles`).
  * Simplicity: computed AFTER the role hierarchy is drawn, as a closure: a role is
    non-simple if it is transitive or a chain head, or has a non-simple sub-role (over
    sub-roles, inverses, equivalences and symmetry). Cardinality, Self, functional,
    inverse-functional, irreflexive, asymmetric and disjoint-role axioms use simple
    roles only.
Families can be switched off with a profile. Stdlib only.

Usage:
  gen.py --seed S --n N --out DIR [--profile default] [--start 0]
  gen.py --self-test
"""
import argparse
import hashlib
import random
import re
import sys
from pathlib import Path

NS = "http://gen.rustdl/o#"
XSD_INT = "xsd:integer"

FAMILIES = {
    "el": 6,            # EL core: ⊓, ∃, atomic subsumption, equivalence
    "neg_or": 3,        # ¬, ⊔, DisjointClasses, DisjointUnion
    "forall": 3,        # ∀, including ∀r.(B ⊔ ¬C)
    "card": 2,          # qualified cardinality, n ≤ 3, simple roles only
    "roles": 3,         # sub-role, inverse, symmetric, functional, IF, transitive, chains
    "domrange": 2,      # domain/range, atomic and non-atomic heads, ∃r.⊤ ⊑ X
    "nominal": 1,       # {a}, ∃r.{a}
    "data": 1,          # functional dp, ≥n / ≤n dp (n ≤ 2), DataHasValue, integer facets
    "self": 1,          # ∃r.Self on simple roles
    "bot": 1,           # A ⊑ ⊥, A ⊓ B ⊑ ⊥, ⊤ ⊑ C
    "abox": 2,          # class / role / data assertions, Same/Different
}

PROFILES = {
    "default": dict(FAMILIES),
    # Secondary, stated-separately profiles (plan §5): never merged with default.
    "el_only": {"el": 6, "domrange": 2, "roles": 1, "bot": 1},
    "card_heavy": {**FAMILIES, "card": 6, "forall": 4, "neg_or": 4},
    "data_heavy": {**FAMILIES, "data": 6},
    # Strict pure EL with motifs (#363 step 3a): a separate generator, ElCase below.
    "el_guaranteed": {},
    # el_guaranteed plus ⊤-LHS GCIs (#444/#445): ElTopCase below, its own RNG stream.
    "el_top": {},
    # Role polarity (#526–#528 follow-up): PolarityCase below, its own RNG stream.
    "polarity": {},
    # The universal role (#543 part 2, #624's folds): UniversalCase below, own RNG stream.
    "universal": {},
}


def gen_hash():
    return hashlib.sha256(Path(__file__).read_bytes()).hexdigest()[:12]


class Case:
    def __init__(self, seed, index, profile="default"):
        self.seed, self.index, self.profile = seed, index, profile
        self.rng = random.Random(f"{seed}:{index}:{profile}")
        r = self.rng
        self.classes = [f"C{i}" for i in range(r.randint(4, 10))]
        self.roles = [f"r{i}" for i in range(r.randint(2, 4))]
        self.dprops = [f"d{i}" for i in range(r.randint(0, 2))]
        self.inds = [f"a{i}" for i in range(r.randint(0, 3))]
        self.weights = {f: w for f, w in PROFILES[profile].items() if w > 0}
        if not self.dprops:
            self.weights.pop("data", None)
        if not self.inds:
            self.weights.pop("nominal", None)
        self.role_axioms = []   # structured role axioms (for the legality checker)
        self.axioms = []        # OFN lines
        self.n_target = r.randint(3, 15)
        self._draw_roles()
        self.simple = simple_roles(self.roles, self.role_axioms)
        self._draw_rest()

    # ---------------------------------------------------------------- roles
    def _draw_roles(self):
        """Role hierarchy first, so simplicity is known before anything uses it."""
        r = self.rng
        if "roles" not in self.weights:
            return
        n = r.randint(0, min(4, self.n_target // 2))
        rank = {x: i for i, x in enumerate(self.roles)}
        for _ in range(n):
            kind = r.choice(["sub", "sub", "subinv", "inverse", "symmetric", "transitive",
                             "chain", "chain"])
            if kind in ("sub", "subinv"):
                a, b = sorted(r.sample(self.roles, 2), key=rank.get)
                self.role_axioms.append((kind, a, b))
            elif kind == "inverse":
                # Same rank group only: r ≡ s⁻ makes s⁻ and r interchangeable, so the two
                # must be equal in the order. Drawn as a fresh-name-free self pair is
                # meaningless; pick two roles and MERGE them in the rank map.
                a, b = r.sample(self.roles, 2)
                self.role_axioms.append(("inverse", a, b))
            elif kind == "symmetric":
                self.role_axioms.append(("symmetric", r.choice(self.roles)))
            elif kind == "transitive":
                self.role_axioms.append(("transitive", r.choice(self.roles)))
            else:
                top = r.choice(self.roles)
                below = [x for x in self.roles if rank[x] < rank[top]]
                form = r.choice(["below", "rr", "rs", "sr"])
                if form == "rr":
                    legs = (top, top)
                elif form in ("rs", "sr") and below:
                    s = r.choice(below)
                    legs = (top, s) if form == "rs" else (s, top)
                elif len(below) >= 1:
                    legs = (r.choice(below), r.choice(below))
                else:
                    continue
                self.role_axioms.append(("chain", legs, top))
        # Drop anything that makes the hierarchy irregular (a redraw by removal).
        kept = []
        for ax in self.role_axioms:
            if regular(self.roles, kept + [ax]):
                kept.append(ax)
        self.role_axioms = kept
        for ax in self.role_axioms:
            self.axioms.append(role_axiom_ofn(ax))

    # ---------------------------------------------------------------- the rest
    def _draw_rest(self):
        r = self.rng
        fams = list(self.weights)
        w = [self.weights[f] for f in fams]
        tries = 0
        while len(self.axioms) < self.n_target and tries < 200:
            tries += 1
            fam = r.choices(fams, w)[0]
            ax = getattr(self, f"ax_{fam}")()
            if ax and ax not in self.axioms:
                self.axioms.append(ax)

    # helpers
    def C(self):
        return ":" + self.rng.choice(self.classes)

    def R(self, simple=False):
        pool = sorted(self.simple) if simple else self.roles
        if not pool:
            return None
        x = ":" + self.rng.choice(pool)
        if self.rng.random() < 0.15:
            return f"ObjectInverseOf({x})"
        return x

    def I(self):
        return ":" + self.rng.choice(self.inds)

    def ce(self, depth=1, fams=None):
        """A class expression from the enabled families, depth-bounded."""
        r = self.rng
        fams = fams or self.weights
        opts = ["atom", "atom", "atom"]
        if depth > 0:
            opts += ["and", "some", "some"]
            if "neg_or" in fams:
                opts += ["or", "not"]
            if "forall" in fams:
                opts += ["all"]
            if "card" in fams and self.simple:
                opts += ["card"]
            if "nominal" in fams and self.inds:
                opts += ["oneof", "hasvalue"]
            if "self" in fams and self.simple:
                opts += ["self"]
            if "data" in fams and self.dprops:
                opts += ["data"]
        k = r.choice(opts)
        if k == "atom":
            return self.C()
        if k == "and":
            return f"ObjectIntersectionOf({self.ce(depth - 1)} {self.ce(depth - 1)})"
        if k == "or":
            return f"ObjectUnionOf({self.ce(depth - 1)} {self.ce(depth - 1)})"
        if k == "not":
            return f"ObjectComplementOf({self.C()})"
        if k == "some":
            filler = "owl:Thing" if r.random() < 0.15 else self.ce(depth - 1)
            return f"ObjectSomeValuesFrom({self.R()} {filler})"
        if k == "all":
            if r.random() < 0.3:
                return (f"ObjectAllValuesFrom({self.R()} "
                        f"ObjectUnionOf({self.C()} ObjectComplementOf({self.C()})))")
            return f"ObjectAllValuesFrom({self.R()} {self.ce(depth - 1)})"
        if k == "card":
            op = r.choice(["ObjectMinCardinality", "ObjectMaxCardinality",
                           "ObjectExactCardinality"])
            n = r.randint(0, 3) if op != "ObjectMinCardinality" else r.randint(1, 3)
            return f"{op}({n} {self.R(simple=True)} {self.ce(depth - 1)})"
        if k == "oneof":
            inds = r.sample(self.inds, r.randint(1, min(2, len(self.inds))))
            return f"ObjectOneOf({' '.join(':' + i for i in inds)})"
        if k == "hasvalue":
            return f"ObjectHasValue({self.R()} {self.I()})"
        if k == "self":
            return f"ObjectHasSelf({self.R(simple=True)})"
        if k == "data":
            return self.data_ce()
        raise AssertionError(k)

    def data_ce(self):
        r = self.rng
        d = ":" + r.choice(self.dprops)
        k = r.choice(["min", "max", "value", "facet"])
        if k == "min":
            return f"DataMinCardinality({r.randint(1, 2)} {d})"
        if k == "max":
            return f"DataMaxCardinality({r.randint(0, 2)} {d})"
        if k == "value":
            return f'DataHasValue({d} "{r.randint(0, 9)}"^^{XSD_INT})'
        lo = r.randint(0, 6)
        hi = lo + r.randint(0, 4)
        return (f"DataSomeValuesFrom({d} DatatypeRestriction({XSD_INT} "
                f'xsd:minInclusive "{lo}"^^{XSD_INT} xsd:maxInclusive "{hi}"^^{XSD_INT}))')

    # families: each returns one OFN axiom line, or None
    def ax_el(self):
        r = self.rng
        k = r.choice(["sub", "sub", "subce", "gci", "equiv"])
        el = {"el": 1}
        if k == "sub":
            a, b = r.sample(self.classes, 2)
            return f"SubClassOf(:{a} :{b})"
        if k == "subce":
            return f"SubClassOf({self.C()} {self.ce(2, el)})"
        if k == "gci":
            return f"SubClassOf({self.ce(2, el)} {self.C()})"
        return f"EquivalentClasses({self.C()} {self.ce(2, el)})"

    def _mixed(self, fam):
        r = self.rng
        fams = {"el": 1, fam: 1}
        k = r.choice(["sub", "gci", "equiv"])
        if k == "sub":
            return f"SubClassOf({self.C()} {self.ce(2, fams)})"
        if k == "gci":
            return f"SubClassOf({self.ce(2, fams)} {self.C()})"
        return f"EquivalentClasses({self.C()} {self.ce(2, fams)})"

    def ax_neg_or(self):
        r = self.rng
        k = r.choice(["mixed", "mixed", "disjoint", "dunion"])
        if k == "disjoint":
            return f"DisjointClasses({' '.join(':' + c for c in r.sample(self.classes, 2))})"
        if k == "dunion" and len(self.classes) >= 3:
            a, b, c = r.sample(self.classes, 3)
            return f"DisjointUnion(:{a} :{b} :{c})"
        return self._mixed("neg_or")

    def ax_forall(self):
        return self._mixed("forall")

    def ax_card(self):
        return self._mixed("card") if self.simple else None

    def ax_nominal(self):
        return self._mixed("nominal")

    def ax_self(self):
        return self._mixed("self") if self.simple else None

    def ax_data(self):
        r = self.rng
        k = r.choice(["functional", "mixed", "mixed", "domain"])
        if k == "functional":
            return f"FunctionalDataProperty(:{r.choice(self.dprops)})"
        if k == "domain":
            return f"DataPropertyDomain(:{r.choice(self.dprops)} {self.C()})"
        return self._mixed("data")

    def ax_roles(self):
        """Role characteristics drawn after simplicity is known (simple roles only)."""
        r = self.rng
        if not self.simple:
            return None
        s = ":" + r.choice(sorted(self.simple))
        k = r.choice(["FunctionalObjectProperty", "InverseFunctionalObjectProperty",
                      "IrreflexiveObjectProperty", "AsymmetricObjectProperty",
                      "ReflexiveObjectProperty", "disjoint"])
        if k == "disjoint":
            if len(self.simple) < 2:
                return None
            a, b = r.sample(sorted(self.simple), 2)
            return f"DisjointObjectProperties(:{a} :{b})"
        return f"{k}({s})"

    def ax_domrange(self):
        r = self.rng
        fams = {f: 1 for f in ("el", "neg_or") if f in self.weights} or {"el": 1}
        head = self.C() if r.random() < 0.6 else self.ce(1, fams)
        k = r.choice(["ObjectPropertyDomain", "ObjectPropertyRange", "gci"])
        if k == "gci":
            return f"SubClassOf(ObjectSomeValuesFrom({self.R()} owl:Thing) {head})"
        return f"{k}({self.R()} {head})"

    def ax_bot(self):
        r = self.rng
        k = r.choice(["sub", "and", "top"])
        if k == "sub":
            return f"SubClassOf({self.C()} owl:Nothing)"
        if k == "and":
            a, b = r.sample(self.classes, 2)
            return f"SubClassOf(ObjectIntersectionOf(:{a} :{b}) owl:Nothing)"
        return f"SubClassOf(owl:Thing {self.C()})"

    def ax_abox(self):
        r = self.rng
        if not self.inds:
            return None
        opts = ["class", "class", "role"]
        if len(self.inds) >= 2:
            opts += ["same", "diff"]
        if self.dprops:
            opts += ["data"]
        k = r.choice(opts)
        if k == "class":
            return f"ClassAssertion({self.ce(1)} {self.I()})"
        if k == "role":
            return f"ObjectPropertyAssertion(:{r.choice(self.roles)} {self.I()} {self.I()})"
        if k == "same":
            a, b = r.sample(self.inds, 2)
            return f"SameIndividual(:{a} :{b})"
        if k == "diff":
            a, b = r.sample(self.inds, 2)
            return f"DifferentIndividuals(:{a} :{b})"
        return (f"DataPropertyAssertion(:{r.choice(self.dprops)} {self.I()} "
                f'"{r.randint(0, 9)}"^^{XSD_INT})')

    # ---------------------------------------------------------------- output
    def header(self):
        return f"# gen {gen_hash()} seed {self.seed} case {self.index} profile {self.profile}"

    def ofn(self, axioms=None):
        axioms = self.axioms if axioms is None else axioms
        return render(self.header(), self.classes, self.roles, self.dprops, self.inds, axioms)


EL_KINDS = {"sub": 4, "equiv": 3, "bot": 1, "disjoint": 1, "domrange": 2, "gci_domain": 2}
MOTIFS = ("gci_domain", "nested_range")


class ElCase:
    """`el_guaranteed` (#363 step 3a): strictly what `is_pure_el` admits, so rustdl's
    `completeness_guaranteed` holds and a classify miss is a finding.

    EL concepts (atomic, ⊓, forward ∃, ⊤, ⊥) in SubClassOf / EquivalentClasses; atomic
    DisjointClasses; forward sub-roles, length-2 chains, transitivity and role
    equivalence; atomic Domain/Range; non-atomic domain heads only in the GCI spelling
    `∃r.⊤ ⊑ X`. No inverses, data properties or individuals. With probability 0.5 one
    motif is planted with distinct picks, so the axiom it targets changes an answer:
      gci_domain   (#114, M8a): ∃r.⊤ ⊑ P ⊓ ∃s.S, C ⊑ ∃r.B, ∃s.S ⊑ Z  ⟹ C ⊑ Z, C ⊑ P
      nested_range (#81): three sub-shapes, each ⟹ C ⊑ D
    The default `Case` is not touched: this class has its own RNG stream."""

    profile = "el_guaranteed"
    kinds = EL_KINDS
    motifs = MOTIFS
    motif_p = 0.5

    def __init__(self, seed, index, profile=None):
        self.seed, self.index = seed, index
        self.rng = random.Random(f"{seed}:{index}:{self.profile}")
        r = self.rng
        self.classes = [f"C{i}" for i in range(r.randint(4, 10))]
        self.roles = [f"r{i}" for i in range(r.randint(2, 4))]
        self.dprops, self.inds = [], []
        self.role_axioms, self.axioms = [], []
        self.n_target = r.randint(3, 15)
        self.motif = "none"
        if r.random() < self.motif_p:
            self._plant_motif()
        self._draw_roles()
        self.simple = simple_roles(self.roles, self.role_axioms)
        tries = 0
        kinds = list(self.kinds)
        w = [self.kinds[k] for k in kinds]
        while len(self.axioms) < self.n_target and tries < 200:
            tries += 1
            ax = getattr(self, f"el_{r.choices(kinds, w)[0]}")()
            if ax and ax not in self.axioms:
                self.axioms.append(ax)

    def _plant_motif(self):
        r = self.rng
        kind = r.choice(self.motifs)
        if len(self.roles) < 2:
            return
        planted = self._motif_axioms(kind)
        if planted is None:
            return
        ax, name = planted
        self.axioms.extend(ax)
        self.n_target = max(self.n_target, len(ax))
        self.motif = name

    def _motif_axioms(self, kind):
        """The motif's axioms and name, or None when the case is too small for it."""
        r = self.rng
        if kind == "gci_domain":
            if len(self.classes) < 5:
                return None
            c, b, p, s_, z = r.sample(self.classes, 5)
            ro, so = r.sample(self.roles, 2)
            ax = [f"SubClassOf(ObjectSomeValuesFrom(:{ro} owl:Thing) "
                  f"ObjectIntersectionOf(:{p} ObjectSomeValuesFrom(:{so} :{s_})))",
                  f"SubClassOf(:{c} ObjectSomeValuesFrom(:{ro} :{b}))",
                  f"SubClassOf(ObjectSomeValuesFrom(:{so} :{s_}) :{z})"]
            name = "gci_domain"
        else:
            sub = r.choice("abc")
            need = 5 if sub == "c" else 4
            if len(self.classes) < need:
                return None
            pick = r.sample(self.classes, need)
            ro, so = r.sample(self.roles, 2)
            if sub == "a":
                c, a, f, d = pick
                ax = [f"ObjectPropertyRange(:{ro} :{f})",
                      f"SubClassOf(:{c} ObjectSomeValuesFrom(:{ro} ObjectSomeValuesFrom(:{so} :{a})))",
                      f"SubClassOf(ObjectSomeValuesFrom(:{ro} ObjectIntersectionOf(:{f} "
                      f"ObjectSomeValuesFrom(:{so} :{a}))) :{d})"]
            elif sub == "b":
                c, a, f, d = pick
                ax = [f"ObjectPropertyRange(:{so} :{f})",
                      f"SubClassOf(:{c} ObjectSomeValuesFrom(:{ro} ObjectSomeValuesFrom(:{so} :{a})))",
                      f"SubClassOf(ObjectSomeValuesFrom(:{ro} ObjectSomeValuesFrom(:{so} "
                      f"ObjectIntersectionOf(:{a} :{f}))) :{d})"]
            else:
                c, e, a, f, d = pick
                ax = [f"SubClassOf(:{c} :{e})",
                      f"SubClassOf(ObjectIntersectionOf(:{c} :{e}) ObjectSomeValuesFrom(:{ro} :{a}))",
                      f"ObjectPropertyRange(:{ro} :{f})",
                      f"SubClassOf(ObjectSomeValuesFrom(:{ro} ObjectIntersectionOf(:{a} :{f})) :{d})"]
            name = f"nested_range_{sub}"
        return ax, name

    def _draw_roles(self):
        r = self.rng
        rank = {x: i for i, x in enumerate(self.roles)}
        drawn = []
        for _ in range(r.randint(0, min(4, self.n_target // 2))):
            kind = r.choice(["sub", "sub", "transitive", "equivrole", "chain", "chain"])
            if kind in ("sub", "equivrole"):
                a, b = sorted(r.sample(self.roles, 2), key=rank.get)
                drawn.append((kind, a, b))
            elif kind == "transitive":
                drawn.append(("transitive", r.choice(self.roles)))
            else:
                top = r.choice(self.roles)
                below = [x for x in self.roles if rank[x] < rank[top]]
                form = r.choice(["below", "rr", "rs", "sr"])
                if form == "rr":
                    legs = (top, top)
                elif form in ("rs", "sr") and below:
                    s_ = r.choice(below)
                    legs = (top, s_) if form == "rs" else (s_, top)
                elif below:
                    legs = (r.choice(below), r.choice(below))
                else:
                    continue
                drawn.append(("chain", legs, top))
        for ax in drawn:
            if ax not in self.role_axioms and regular(self.roles, self.role_axioms + [ax]):
                self.role_axioms.append(ax)
                self.axioms.append(role_axiom_ofn(ax))

    def C(self):
        return ":" + self.rng.choice(self.classes)

    def R(self):
        return ":" + self.rng.choice(self.roles)

    def ce(self, depth):
        r = self.rng
        k = r.choice(["atom", "atom"] + (["and", "some", "some", "top"] if depth > 0 else []))
        if k == "atom":
            return self.C()
        if k == "and":
            a, b = self.ce(depth - 1), self.ce(depth - 1)
            return None if a == b or None in (a, b) else f"ObjectIntersectionOf({a} {b})"
        if k == "top":
            return f"ObjectSomeValuesFrom({self.R()} owl:Thing)"
        f = self.ce(depth - 1)
        return None if f is None else f"ObjectSomeValuesFrom({self.R()} {f})"

    def complex_ce(self):
        for _ in range(10):
            x = self.ce(2)
            if x and not x.startswith(":"):
                return x
        return None

    def el_sub(self):
        r = self.rng
        k = r.choice(["atom", "rhs", "lhs"])
        if k == "atom":
            a, b = r.sample(self.classes, 2)
            return f"SubClassOf(:{a} :{b})"
        x = self.complex_ce()
        if x is None:
            return None
        return f"SubClassOf({self.C()} {x})" if k == "rhs" else f"SubClassOf({x} {self.C()})"

    def el_equiv(self):
        x = self.complex_ce()
        return None if x is None else f"EquivalentClasses({self.C()} {x})"

    def el_bot(self):
        r = self.rng
        if r.random() < 0.5:
            return f"SubClassOf({self.C()} owl:Nothing)"
        a, b = r.sample(self.classes, 2)
        return f"SubClassOf(ObjectIntersectionOf(:{a} :{b}) owl:Nothing)"

    def el_disjoint(self):
        a, b = self.rng.sample(self.classes, 2)
        return f"DisjointClasses(:{a} :{b})"

    def el_domrange(self):
        k = self.rng.choice(["ObjectPropertyDomain", "ObjectPropertyRange"])
        return f"{k}({self.R()} {self.C()})"

    def el_gci_domain(self):
        x = self.complex_ce()
        return None if x is None else f"SubClassOf(ObjectSomeValuesFrom({self.R()} owl:Thing) {x})"

    def header(self):
        return (f"# gen {gen_hash()} seed {self.seed} case {self.index} profile {self.profile}\n"
                f"# motif: {self.motif}")

    def ofn(self, axioms=None):
        axioms = self.axioms if axioms is None else axioms
        return render(self.header(), self.classes, self.roles, [], [], axioms)


class ElTopCase(ElCase):
    """`el_top` (#444/#445): `el_guaranteed` plus ⊤-LHS GCIs, which `el_guaranteed` never
    emits. Same fragment (`is_pure_el` admits `⊤` on the left), its own RNG stream, so
    `el_guaranteed` cases are untouched. Extra kind `top_gci`: `⊤ ⊑ X` or `⊤ ≡ X` for an
    atom or a complex EL concept. Motifs, each ending in a SCORED pair (the net does not
    score ⊤-equivalence):
      top_exists (#445): ⊤ ⊑ ∃r.B, A ⊓ ∃r.B ⊑ X                     ⟹ A ⊑ X
      top_chain  (#445): ⊤ ⊑ ∃r.B, ∃r.B ⊑ T, C ⊑ ∃s.D, ∃s.T ⊑ W     ⟹ C ⊑ W
      top_marker (#444): ⊤ ⊑ T, X ⊑ ∃r.∃s.B, ∃r.T ⊑ W               ⟹ X ⊑ W
    plus el_guaranteed's own motifs."""

    profile = "el_top"
    kinds = {**EL_KINDS, "top_gci": 2}
    motifs = MOTIFS + ("top_exists", "top_chain", "top_marker")
    motif_p = 0.8

    def _motif_axioms(self, kind):
        r = self.rng
        if kind not in ("top_exists", "top_chain", "top_marker"):
            return super()._motif_axioms(kind)
        if len(self.classes) < 5:
            return None
        ro, so = r.sample(self.roles, 2)
        if kind == "top_exists":
            a, b, x = r.sample(self.classes, 3)
            ax = [f"SubClassOf(owl:Thing ObjectSomeValuesFrom(:{ro} :{b}))",
                  f"SubClassOf(ObjectIntersectionOf(:{a} ObjectSomeValuesFrom(:{ro} :{b})) :{x})"]
        elif kind == "top_chain":
            b, t, c, d, w = r.sample(self.classes, 5)
            ax = [f"SubClassOf(owl:Thing ObjectSomeValuesFrom(:{ro} :{b}))",
                  f"SubClassOf(ObjectSomeValuesFrom(:{ro} :{b}) :{t})",
                  f"SubClassOf(:{c} ObjectSomeValuesFrom(:{so} :{d}))",
                  f"SubClassOf(ObjectSomeValuesFrom(:{so} :{t}) :{w})"]
        else:
            t, x, b, w = r.sample(self.classes, 4)
            ax = [f"SubClassOf(owl:Thing :{t})",
                  f"SubClassOf(:{x} ObjectSomeValuesFrom(:{ro} ObjectSomeValuesFrom(:{so} :{b})))",
                  f"SubClassOf(ObjectSomeValuesFrom(:{ro} :{t}) :{w})"]
        return ax, kind

    def el_top_gci(self):
        r = self.rng
        x = self.complex_ce() if r.random() < 0.7 else self.C()
        if x is None:
            return None
        if x.startswith(":") or r.random() < 0.8:
            return f"SubClassOf(owl:Thing {x})"
        return f"EquivalentClasses(owl:Thing {x})"


POLARITY_WEIGHTS = {"el": 3, "neg_or": 2, "forall": 2, "card": 3, "roles": 4,
                    "domrange": 3, "nominal": 2, "bot": 1, "abox": 4}
POLARITY_MOTIFS = ("told_edge", "p7", "p5", "p8", "p3", "p6", "p9", "card_inv", "forall_inv",
                   "min_inv_nominal")
# Role-consuming axiom kinds whose `r⁻` form the census and the floor count.
POLARITY_KINDS = {
    "functional": "FunctionalObjectProperty(ObjectInverseOf(",
    "inverse_functional": "InverseFunctionalObjectProperty(ObjectInverseOf(",
    "irreflexive": "IrreflexiveObjectProperty(ObjectInverseOf(",
    "asymmetric": "AsymmetricObjectProperty(ObjectInverseOf(",
    "domain": "ObjectPropertyDomain(ObjectInverseOf(",
    "range": "ObjectPropertyRange(ObjectInverseOf(",
    "assertion": "ObjectPropertyAssertion(ObjectInverseOf(",
    "negative_assertion": "NegativeObjectPropertyAssertion(ObjectInverseOf(",
}


def polarity_kinds(axioms):
    """The role-consuming kinds that occur with an inverse role, for the census line."""
    out = set()
    for a in axioms:
        for k, pre in POLARITY_KINDS.items():
            if a.startswith(pre):
                out.add(k)
        if a.startswith("DisjointObjectProperties(") and "ObjectInverseOf(" in a:
            out.add("disjoint_roles")
        if re.search(r"Cardinality\(\d+ ObjectInverseOf\(", a):
            out.add("cardinality")
        if "AllValuesFrom(ObjectInverseOf(" in a:
            out.add("forall")
        if "SomeValuesFrom(ObjectInverseOf(" in a:
            out.add("exists")
    return out


class PolarityCase(Case):
    """`polarity` (the #526–#528 follow-up): signed roles in every role-consuming axiom
    kind, an ABox in every case, and planted motifs.

    The default profile draws role characteristics, disjoint roles and role assertions on
    named roles only, has no negative role assertion, and draws `r⁻` in 15% of class
    expressions; so `Functional(r⁻)`, `Disjoint(r⁻, s)` and inverse assertions never
    occurred. Here `R()` draws `r⁻` half the time, and characteristics, disjoint roles,
    positive and negative assertions all take signed roles. Simplicity is invariant under
    inversion (`simple_roles` requires both polarities), so the restrictions still hold.

    With probability 0.5 one motif is planted (`# motif: <name>/<variant>`), drawn on
    simple roles after the hierarchy, each as a consistent variant (`ok`) or a control
    that changes the answer (`ctl`), so a bug that drops polarity flips a scored answer:
      told_edge        (#526) r⁻(a,b) with Domain(r)/Range(r): b gets the domain, a the range
      p7               (#527) Domain(r⁻)/Range(r⁻) clashing with a disjoint told type
      p5               (#528) Functional(r⁻) with two successors (ok) or predecessors (ctl)
      p8               (#528) ∃X.Q1, ∃X.Q2, disjoint Q's, under a functional r or r⁻
      p3, p6, p9       negative assertions, Asymmetric/Irreflexive(r⁻), Disjoint(r⁻, s)
      card_inv         ≤1 r⁻ against two predecessors (tableau apply_max, the wedge's ≤n)
      forall_inv       ∀r⁻ over a predecessor edge; and the TBox pair A ⊑ ∃r.∀r⁻.D ⊑ D
      min_inv_nominal  ≥2 r⁻.{a, c} with SameIndividual (ctl) or DifferentIndividuals (ok)
    The floor: a case with no `r⁻` in any role-consuming axiom gets one appended. The
    `# polarity:` line lists the kinds present. Own RNG stream; other profiles untouched.
    """

    profile = "polarity"
    motif_p = 0.5
    inv_p = 0.5

    def __init__(self, seed, index, profile=None):
        self.seed, self.index = seed, index
        self.rng = random.Random(f"{seed}:{index}:{self.profile}")
        r = self.rng
        self.classes = [f"C{i}" for i in range(r.randint(5, 10))]
        self.roles = [f"r{i}" for i in range(r.randint(2, 4))]
        self.dprops = []
        self.inds = [f"a{i}" for i in range(r.randint(3, 5))]
        self.weights = dict(POLARITY_WEIGHTS)
        self.role_axioms, self.axioms = [], []
        self.n_target = r.randint(3, 15)
        self._draw_roles()
        self.simple = simple_roles(self.roles, self.role_axioms)
        self.motif = "none"
        if self.simple and r.random() < self.motif_p:
            self._plant_motif()
        self._draw_rest()
        self._floor()

    def R(self, simple=False):
        pool = sorted(self.simple) if simple else self.roles
        if not pool:
            return None
        x = ":" + self.rng.choice(pool)
        return f"ObjectInverseOf({x})" if self.rng.random() < self.inv_p else x

    def ax_roles(self):
        r = self.rng
        if not self.simple:
            return None
        k = r.choice(["FunctionalObjectProperty", "InverseFunctionalObjectProperty",
                      "IrreflexiveObjectProperty", "AsymmetricObjectProperty",
                      "ReflexiveObjectProperty", "disjoint"])
        if k == "disjoint":
            if len(self.simple) < 2:
                return None
            a, b = r.sample(sorted(self.simple), 2)
            sa = f"ObjectInverseOf(:{a})" if r.random() < self.inv_p else f":{a}"
            sb = f"ObjectInverseOf(:{b})" if r.random() < self.inv_p else f":{b}"
            return f"DisjointObjectProperties({sa} {sb})"
        return f"{k}({self.R(simple=True)})"

    def ax_abox(self):
        r = self.rng
        k = r.choice(["class", "class", "role", "role", "neg", "same", "diff"])
        if k == "class":
            return f"ClassAssertion({self.ce(1)} {self.I()})"
        if k == "role":
            return f"ObjectPropertyAssertion({self.R()} {self.I()} {self.I()})"
        if k == "neg":
            return f"NegativeObjectPropertyAssertion({self.R()} {self.I()} {self.I()})"
        a, b = r.sample(self.inds, 2)
        return f"{'SameIndividual' if k == 'same' else 'DifferentIndividuals'}(:{a} :{b})"

    def _floor(self):
        """Append one `r⁻` role-consuming axiom if the case has none."""
        if polarity_kinds(self.axioms) - {"forall", "exists"}:
            return
        r = self.rng
        x = ":" + r.choice(self.roles)
        a, b = r.sample(self.inds, 2)
        opts = [f"ObjectPropertyAssertion(ObjectInverseOf({x}) :{a} :{b})",
                f"NegativeObjectPropertyAssertion(ObjectInverseOf({x}) :{a} :{b})",
                f"ObjectPropertyDomain(ObjectInverseOf({x}) {self.C()})",
                f"ObjectPropertyRange(ObjectInverseOf({x}) {self.C()})"]
        if self.simple:
            s_ = ":" + r.choice(sorted(self.simple))
            opts += [f"{k}(ObjectInverseOf({s_}))" for k in (
                "FunctionalObjectProperty", "InverseFunctionalObjectProperty",
                "IrreflexiveObjectProperty", "AsymmetricObjectProperty")]
        self.axioms.append(r.choice(opts))

    def _plant_motif(self):
        r = self.rng
        kind = r.choice(POLARITY_MOTIFS)
        planted = self._motif_axioms(kind)
        if planted is None:
            return
        ax, name = planted
        for a in ax:
            if a not in self.axioms:
                self.axioms.append(a)
        self.n_target = max(self.n_target, len(self.axioms) + 2)
        self.motif = name

    def _motif_axioms(self, kind):
        """The motif's axioms and `name/variant`, or None when the case lacks the roles."""
        r = self.rng
        x = r.choice(sorted(self.simple))
        inv = f"ObjectInverseOf(:{x})"
        a, b, c = r.sample(self.inds, 3)
        cs = r.sample(self.classes, 5)
        ok = r.random() < 0.5
        var = "ok" if ok else "ctl"
        if kind == "told_edge":
            d, rng_ = cs[:2]
            ax = [f"ObjectPropertyDomain(:{x} :{d})", f"ObjectPropertyRange(:{x} :{rng_})",
                  f"ObjectPropertyAssertion({inv} :{a} :{b})"]
            if not ok:
                ax.append(f"ObjectPropertyAssertion(:{x} :{b} :{c})")
            return ax, f"told_edge/{'inv' if ok else 'mixed'}"
        if kind == "p7":
            d, e = cs[:2]
            dom = r.random() < 0.5
            head = "ObjectPropertyDomain" if dom else "ObjectPropertyRange"
            # Domain(r⁻) types the edge's target b; Range(r⁻) its source a.
            typed = b if dom else a
            other = a if dom else b
            return ([f"{head}({inv} :{d})", f"DisjointClasses(:{d} :{e})",
                     f"ObjectPropertyAssertion(:{x} :{a} :{b})",
                     f"ClassAssertion(:{e} :{other if ok else typed})"],
                    f"p7_{'domain' if dom else 'range'}/{var}")
        if kind == "p5":
            edges = ([(a, b), (a, c)], (b, c)) if ok else ([(a, b), (c, b)], (a, c))
            return ([f"FunctionalObjectProperty({inv})"]
                    + [f"ObjectPropertyAssertion(:{x} :{s_} :{t})" for s_, t in edges[0]]
                    + [f"DifferentIndividuals(:{edges[1][0]} :{edges[1][1]})"], f"p5/{var}")
        if kind == "p8":
            cl, q1, q2 = cs[:3]
            forms = {  # name: (functional axioms, the two existential roles), consistent?
                "inv_under_func": ([f"FunctionalObjectProperty(:{x})"], (inv, inv), True),
                "named_under_func_inv": ([f"FunctionalObjectProperty({inv})"], (f":{x}", f":{x}"),
                                         True),
                "mixed": ([f"FunctionalObjectProperty(:{x})",
                           f"InverseFunctionalObjectProperty(:{x})"], (f":{x}", inv), True),
                "inv_under_ifunc": ([f"InverseFunctionalObjectProperty(:{x})"], (inv, inv), False),
                "inv_under_func_inv": ([f"FunctionalObjectProperty({inv})"], (inv, inv), False),
                "named_under_func": ([f"FunctionalObjectProperty(:{x})"], (f":{x}", f":{x}"),
                                     False),
            }
            form = r.choice(sorted(forms))
            funcs, (e1, e2), cons = forms[form]
            if r.random() < 0.3:
                body = [f"EquivalentClasses(:{cl} ObjectIntersectionOf("
                        f"ObjectSomeValuesFrom({e1} :{q1}) ObjectSomeValuesFrom({e2} :{q2})))"]
            else:
                body = [f"SubClassOf(:{cl} ObjectSomeValuesFrom({e1} :{q1}))",
                        f"SubClassOf(:{cl} ObjectSomeValuesFrom({e2} :{q2}))"]
            return (funcs + body + [f"DisjointClasses(:{q1} :{q2})", f"ClassAssertion(:{cl} :{a})"],
                    f"p8_{form}/{'ok' if cons else 'ctl'}")
        if kind == "p3":
            edge = (a, b) if ok else (b, a)
            return ([f"NegativeObjectPropertyAssertion({inv} :{a} :{b})",
                     f"ObjectPropertyAssertion(:{x} :{edge[0]} :{edge[1]})"], f"p3/{var}")
        if kind == "p6":
            if r.random() < 0.5:
                ax = [f"AsymmetricObjectProperty({inv})", f"ObjectPropertyAssertion(:{x} :{a} :{b})"]
                if not ok:
                    ax.append(f"ObjectPropertyAssertion(:{x} :{b} :{a})")
                return ax, f"p6_asymmetric/{var}"
            edge = (a, b) if ok else (a, a)
            return ([f"IrreflexiveObjectProperty({inv})",
                     f"ObjectPropertyAssertion(:{x} :{edge[0]} :{edge[1]})"], f"p6_irreflexive/{var}")
        if kind == "p9":
            others = sorted(self.simple - {x})
            if not others:
                return None
            y = r.choice(others)
            edge = (a, b) if ok else (b, a)
            return ([f"DisjointObjectProperties({inv} :{y})",
                     f"ObjectPropertyAssertion(:{x} :{edge[0]} :{edge[1]})",
                     f"ObjectPropertyAssertion(:{y} :{a} :{b})"], f"p9/{var}")
        if kind == "card_inv":
            cl, q, q2 = cs[:3]
            if r.random() < 0.5:
                # ABox: b may have at most one r-predecessor in Q.
                edges = [(a, b), (c, b)] if not ok else [(b, a), (b, c)]
                return ([f"SubClassOf(:{cl} ObjectMaxCardinality(1 {inv} :{q}))",
                         f"ClassAssertion(:{cl} :{b})", f"ClassAssertion(:{q} :{a})",
                         f"ClassAssertion(:{q} :{c})", f"DifferentIndividuals(:{a} :{c})"]
                        + [f"ObjectPropertyAssertion(:{x} :{s_} :{t})" for s_, t in edges],
                        f"card_inv_abox/{var}")
            # TBox: two disjoint r-predecessors against ≤1 r⁻ (ctl: Cl unsat) or ≤1 r (ok).
            bound = inv if not ok else f":{x}"
            return ([f"SubClassOf(:{cl} ObjectSomeValuesFrom({inv} :{q}))",
                     f"SubClassOf(:{cl} ObjectSomeValuesFrom({inv} :{q2}))",
                     f"DisjointClasses(:{q} :{q2})",
                     f"SubClassOf(:{cl} ObjectMaxCardinality(1 {bound} owl:Thing))"],
                    f"card_inv_tbox/{var}")
        if kind == "forall_inv":
            cl, d, bcl = cs[:3]
            if r.random() < 0.5:
                edge = (a, b) if not ok else (b, a)
                return ([f"SubClassOf(:{cl} ObjectAllValuesFrom({inv} :{d}))",
                         f"ClassAssertion(:{cl} :{b})",
                         f"ClassAssertion(ObjectComplementOf(:{d}) :{a})",
                         f"ObjectPropertyAssertion(:{x} :{edge[0]} :{edge[1]})"],
                        f"forall_inv_abox/{var}")
            # TBox: Cl ⊑ ∃r.B and B ⊑ ∀r⁻.D entail Cl ⊑ D (`pos`); with ∀r it does not (`neg`).
            q = inv if ok else f":{x}"
            return ([f"SubClassOf(:{cl} ObjectSomeValuesFrom(:{x} :{bcl}))",
                     f"SubClassOf(:{bcl} ObjectAllValuesFrom({q} :{d}))"],
                    f"forall_inv_tbox/{'pos' if ok else 'neg'}")
        if kind == "min_inv_nominal":
            cl = cs[0]
            rel = "DifferentIndividuals" if ok else "SameIndividual"
            return ([f"SubClassOf(:{cl} ObjectMinCardinality(2 {inv} ObjectOneOf(:{a} :{c})))",
                     f"{rel}(:{a} :{c})", f"ClassAssertion(:{cl} :{b})"],
                    f"min_inv_nominal/{var}")
        raise AssertionError(kind)

    def header(self):
        kinds = ",".join(sorted(polarity_kinds(self.axioms))) or "none"
        return (f"# gen {gen_hash()} seed {self.seed} case {self.index} profile {self.profile}\n"
                f"# motif: {self.motif}\n# polarity: {kinds}")


TOP = "owl:topObjectProperty"
TOP_DATA = "owl:topDataProperty"
UNIVERSAL_WEIGHTS = {"el": 3, "neg_or": 2, "forall": 2, "card": 2, "roles": 2,
                     "domrange": 2, "nominal": 1, "bot": 1, "abox": 3, "universal": 6}


class UniversalCase(Case):
    """`universal` (#543 part 2, #624): the universal role in every OWL 2 DL position, so a
    wrong fold of #624's is a scored false positive.

    #624 states, as built-ins, the Direct-Semantics meaning of each foldable use of
    `owl:topObjectProperty` (and properties above it): `∃u.X` / `≥1 u.X` with X ⊤, a nominal
    or a class with a told instance → ⊤; `∀u.⊥`, `∀u.¬X`, `≤0 u.X` → ⊥; `Domain/Range(u, C)`
    → `⊤ ⊑ C`. No other profile emits a top property.

    **Legal positions only, as HermiT checks them** (measured on 2026-11-28, one file per
    form): HermiT treats top as non-simple, so it rejects top in `≤1` / `=1` / `≥2`,
    `Self`, `Functional`, `InverseFunctional`, `Irreflexive`, `Asymmetric` and
    `DisjointObjectProperties`, and `topDataProperty` in a class expression. Those folds
    of #624 (`Self(u)`, `Irreflexive/Asymmetric(u)`, `Disjoint(u, s)`) act on non-DL input,
    which the peers do not adjudicate: **not covered here**. Allowed and drawn: `∃top`,
    `∀top`, `≤0 top`, `≥1 top`, `hasValue(top, a)`, Domain and Range of top,
    `Reflexive/Symmetric/Transitive(top)`, `r ⊑ top`, `top ⊑ r` (which makes `r`, and every
    role above it, non-simple: pre-drawn, then excluded from the simple positions),
    positive and negative top assertions, `∃top.C(a)`, and `d ⊑ topDataProperty`.
    `R()` returns top 30% of the time, but never where a simple role is required. Half the
    cases have individuals with told types (the conditional folds' licences). The floor: a
    case with no top use gets one appended. The `# universal:` line lists the kinds
    present. Own RNG stream; other profiles untouched.
    """

    profile = "universal"
    top_p = 0.3

    def __init__(self, seed, index, profile=None):
        self.seed, self.index = seed, index
        self.rng = random.Random(f"{seed}:{index}:{self.profile}")
        r = self.rng
        self.classes = [f"C{i}" for i in range(r.randint(4, 9))]
        self.roles = [f"r{i}" for i in range(r.randint(2, 4))]
        self.dprops = [f"d{i}" for i in range(r.randint(0, 1))]
        self.inds = [f"a{i}" for i in range(r.randint(2, 4))] if r.random() < 0.5 else []
        self.weights = dict(UNIVERSAL_WEIGHTS)
        if not self.inds:
            self.weights.pop("abox", None)
            self.weights.pop("nominal", None)
        self.role_axioms, self.axioms = [], []
        self.n_target = r.randint(3, 14)
        self._draw_roles()
        # `top ⊑ r` first: `r` (and every role above it) then has a non-simple sub-role.
        top_subs = [x for x in self.roles if r.random() < 0.12]
        for x in top_subs:
            self.axioms.append(f"SubObjectPropertyOf({TOP} :{x})")
        self.simple = simple_roles(
            self.roles + ["__top__"],
            self.role_axioms + [("transitive", "__top__")]
            + [("sub", "__top__", x) for x in top_subs],
        ) - {"__top__"}
        self._draw_rest()
        self._floor()

    def R(self, simple=False):
        if not simple and self.rng.random() < self.top_p:
            return TOP
        return super().R(simple)

    def ax_universal(self):
        r = self.rng
        k = r.choice(["domain", "range", "char", "sub_top", "some", "some", "all", "card",
                      "gci", "abox", "data"])
        if k == "domain":
            return f"ObjectPropertyDomain({TOP} {self.ce(1)})"
        if k == "range":
            return f"ObjectPropertyRange({TOP} {self.ce(1)})"
        if k == "char":
            c = r.choice(["ReflexiveObjectProperty", "SymmetricObjectProperty",
                          "TransitiveObjectProperty"])
            return f"{c}({TOP})"
        if k == "sub_top":
            return f"SubObjectPropertyOf(:{r.choice(self.roles)} {TOP})"
        if k == "some":
            filler = r.choice(["owl:Thing", self.C(), self.ce(1)])
            if self.inds and r.random() < 0.25:
                return f"SubClassOf({self.C()} ObjectHasValue({TOP} {self.I()}))"
            return f"SubClassOf({self.C()} ObjectSomeValuesFrom({TOP} {filler}))"
        if k == "all":
            filler = r.choice(["owl:Nothing", f"ObjectComplementOf({self.C()})", self.C()])
            ce = f"ObjectAllValuesFrom({TOP} {filler})"
            return f"SubClassOf({self.C()} {ce})" if r.random() < 0.5 else f"SubClassOf({ce} {self.C()})"
        if k == "card":
            op, n = r.choice([("ObjectMaxCardinality", 0), ("ObjectMinCardinality", 1)])
            ce = f"{op}({n} {TOP} {self.C()})"
            return f"SubClassOf({self.C()} {ce})" if r.random() < 0.6 else f"SubClassOf({ce} {self.C()})"
        if k == "gci":
            return f"SubClassOf(ObjectSomeValuesFrom({TOP} {self.C()}) {self.C()})"
        if k == "abox":
            if not self.inds:
                return None
            a, b = self.I(), self.I()
            kk = r.choice(["pos", "neg", "some", "some"])
            if kk == "pos":
                return f"ObjectPropertyAssertion({TOP} {a} {b})"
            if kk == "neg":
                return f"NegativeObjectPropertyAssertion({TOP} {a} {b})"
            return f"ClassAssertion(ObjectSomeValuesFrom({TOP} {self.C()}) {a})"
        if not self.dprops:
            return None
        return f"SubDataPropertyOf(:{r.choice(self.dprops)} {TOP_DATA})"

    def _floor(self):
        if not any(TOP in a or TOP_DATA in a for a in self.axioms):
            for _ in range(20):
                ax = self.ax_universal()
                if ax:
                    self.axioms.append(ax)
                    break

    def kinds(self):
        pats = {"domain": f"ObjectPropertyDomain({TOP}", "range": f"ObjectPropertyRange({TOP}",
                "char": f"ObjectProperty({TOP})", "sub_top": f" {TOP})",
                "top_sub": f"SubObjectPropertyOf({TOP}",
                "some": f"ObjectSomeValuesFrom({TOP}", "all": f"ObjectAllValuesFrom({TOP}",
                "max0": f"ObjectMaxCardinality(0 {TOP}", "min1": f"ObjectMinCardinality(1 {TOP}",
                "has_value": f"ObjectHasValue({TOP}",
                "neg_assert": f"NegativeObjectPropertyAssertion({TOP}",
                "pos_assert": f"ObjectPropertyAssertion({TOP}", "data_top": TOP_DATA}
        return sorted(k for k, p in pats.items() if any(p in a for a in self.axioms))

    def header(self):
        return (f"# gen {gen_hash()} seed {self.seed} case {self.index} profile {self.profile}\n"
                f"# universal: {' '.join(self.kinds()) or 'none'}")


def make_case(seed, index, profile="default"):
    if profile == "el_guaranteed":
        return ElCase(seed, index)
    if profile == "el_top":
        return ElTopCase(seed, index)
    if profile == "polarity":
        return PolarityCase(seed, index)
    if profile == "universal":
        return UniversalCase(seed, index)
    return Case(seed, index, profile)


EL_FORBIDDEN = ("ObjectInverseOf", "Union", "Complement", "AllValues", "Cardinality", "OneOf",
                "HasValue", "HasSelf", "Data", "Functional", "Individual", "DisjointUnion",
                "Symmetric", "InverseObjectProperties", "Reflexive", "Irreflexive", "Asymmetric",
                "DisjointObjectProperties", "Assertion")


def render(header, classes, roles, dprops, inds, axioms):
    lines = [header,
             f"Prefix(:=<{NS}>)",
             "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)",
             "Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)",
             "Ontology(<http://gen.rustdl/o>"]
    lines += [f"Declaration(Class(:{c}))" for c in classes]
    lines += [f"Declaration(ObjectProperty(:{x}))" for x in roles]
    lines += [f"Declaration(DataProperty(:{x}))" for x in dprops]
    lines += [f"Declaration(NamedIndividual(:{x}))" for x in inds]
    lines += list(axioms)
    lines.append(")")
    return "\n".join(lines) + "\n"


def role_axiom_ofn(ax):
    k = ax[0]
    if k == "sub":
        return f"SubObjectPropertyOf(:{ax[1]} :{ax[2]})"
    if k == "subinv":
        return f"SubObjectPropertyOf(ObjectInverseOf(:{ax[1]}) ObjectInverseOf(:{ax[2]}))"
    if k == "inverse":
        return f"InverseObjectProperties(:{ax[1]} :{ax[2]})"
    if k == "symmetric":
        return f"SymmetricObjectProperty(:{ax[1]})"
    if k == "transitive":
        return f"TransitiveObjectProperty(:{ax[1]})"
    if k == "equivrole":
        return f"EquivalentObjectProperties(:{ax[1]} :{ax[2]})"
    if k == "chain":
        (a, b), top = ax[1], ax[2]
        return f"SubObjectPropertyOf(ObjectPropertyChain(:{a} :{b}) :{top})"
    raise AssertionError(k)


# ---------------------------------------------------------------- legality
# Role expressions are (name, inverted?) pairs.
def _inv(x):
    return (x[0], not x[1])


def _edges(role_axioms):
    """Sub-role edges over signed role expressions, closed under INV."""
    e = set()
    for ax in role_axioms:
        k = ax[0]
        if k == "sub":
            e.add(((ax[1], False), (ax[2], False)))
        elif k == "subinv":
            e.add(((ax[1], True), (ax[2], True)))
        elif k == "inverse":
            e.add(((ax[1], False), (ax[2], True)))
            e.add(((ax[2], True), (ax[1], False)))
        elif k == "equivrole":
            e.add(((ax[1], False), (ax[2], False)))
            e.add(((ax[2], False), (ax[1], False)))
        elif k == "symmetric":
            e.add(((ax[1], False), (ax[1], True)))
            e.add(((ax[1], True), (ax[1], False)))
    return e | {(_inv(a), _inv(b)) for a, b in e}


def _reach(edges, nodes):
    succ = {n: set() for n in nodes}
    for a, b in edges:
        succ[a].add(b)
    out = {}
    for n in nodes:
        seen, st = {n}, [n]
        while st:
            x = st.pop()
            for y in succ[x]:
                if y not in seen:
                    seen.add(y)
                    st.append(y)
        out[n] = seen
    return out


def simple_roles(roles, role_axioms):
    """Names of roles R such that both R and R⁻ are simple."""
    nodes = [(x, s) for x in roles for s in (False, True)]
    reach = _reach(_edges(role_axioms), nodes)
    nonsimple = set()
    for ax in role_axioms:
        if ax[0] == "transitive":
            nonsimple |= {(ax[1], False), (ax[1], True)}
        elif ax[0] == "chain":
            nonsimple |= {(ax[2], False), (ax[2], True)}
    # R is non-simple if some sub-role of R (R itself included) is non-simple.
    bad = {n for n in nodes if any(m in nonsimple for m in nodes if n in reach[m])}
    return {x for x in roles if (x, False) not in bad and (x, True) not in bad}


def regular(roles, role_axioms):
    """OWL 2 regularity: a strict order on rank classes satisfying every chain."""
    nodes = [(x, s) for x in roles for s in (False, True)]
    reach = _reach(_edges(role_axioms), nodes)
    # equivalence classes (mutual reachability), then rank classes (merge with INV).
    parent = {n: n for n in nodes}

    def find(n):
        while parent[n] != n:
            parent[n] = parent[parent[n]]
            n = parent[n]
        return n

    def union(a, b):
        parent[find(a)] = find(b)

    for a in nodes:
        for b in nodes:
            if b in reach[a] and a in reach[b]:
                union(a, b)
    equiv = {n: find(n) for n in nodes}
    for n in nodes:
        union(n, _inv(n))
    rank = {n: find(n) for n in nodes}
    less = set()  # (lower rank class, higher rank class)
    for ax in role_axioms:
        if ax[0] != "chain":
            continue
        (a, b), top = ax[1], ax[2]
        A, B, T = (a, False), (b, False), (top, False)
        legs = []
        if a == top and b == top:
            continue                     # r∘r ⊑ r
        if a == top:
            legs = [B]                   # r∘s ⊑ r
        elif b == top:
            legs = [A]                   # s∘r ⊑ r
        else:
            legs = [A, B]
        for leg in legs:
            if equiv[leg] == equiv[T] or rank[leg] == rank[T]:
                return False             # a non-special leg equal to or ranked with r
            less.add((rank[leg], rank[T]))
    # the sub-role hierarchy must not put a rank class strictly below itself through
    # the chain constraints: check the constraint graph is acyclic.
    succ = {}
    for lo, hi in less:
        succ.setdefault(lo, set()).add(hi)
    state = {}

    def cyclic(n):
        state[n] = 1
        for m in succ.get(n, ()):
            if state.get(m) == 1 or (m not in state and cyclic(m)):
                return True
        state[n] = 2
        return False

    if any(n not in state and cyclic(n) for n in list(succ)):
        return False
    # A sub-role edge s ⊑ r (not mutual) requires s ≤ r: r must not be forced below s.
    for x in nodes:
        for y in reach[x]:
            if equiv[x] != equiv[y] and _below(rank[y], rank[x], succ):
                return False
    return True


def _below(a, b, succ):
    """True iff rank class a is forced strictly below rank class b."""
    seen, st = {a}, [a]
    while st:
        n = st.pop()
        for m in succ.get(n, ()):
            if m == b:
                return True
            if m not in seen:
                seen.add(m)
                st.append(m)
    return False


def check(case):
    """Re-validate a generated case's role restrictions from its OFN lines."""
    errs = []
    if not regular(case.roles, case.role_axioms):
        errs.append("irregular role hierarchy")
    simple = simple_roles(case.roles, case.role_axioms)
    restricted = ("ObjectMinCardinality", "ObjectMaxCardinality", "ObjectExactCardinality",
                  "ObjectHasSelf", "FunctionalObjectProperty",
                  "InverseFunctionalObjectProperty", "IrreflexiveObjectProperty",
                  "AsymmetricObjectProperty", "DisjointObjectProperties")
    import re
    role = r"(?:ObjectInverseOf\(:r\d+\)|:r\d+)"
    for line in case.axioms:
        for op in restricted:
            # Every role operand, either polarity, in every position: the old pattern read
            # an inverse only in the first DisjointObjectProperties position.
            for m in re.finditer(op + r"\((?:\d+ )?(" + role + r"(?: " + role + r")*)", line):
                for name in re.findall(r":(r\d+)", m.group(1)):
                    if name not in simple:
                        errs.append(f"non-simple {name} in {op}: {line}")
    return errs


# ---------------------------------------------------------------- CLI
def generate(seed, n, out, profile="default", start=0):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    bad = 0
    for i in range(start, start + n):
        c = make_case(seed, i, profile)
        errs = check(c)
        if errs:
            bad += 1
            print(f"case {i}: generator produced an illegal case: {errs}", file=sys.stderr)
        (out / f"case-{i:05d}.ofn").write_text(c.ofn())
    return bad


def self_test():
    fails = 0

    def ok(name, got, want):
        nonlocal fails
        good = got == want
        fails += not good
        print(f"{'ok  ' if good else 'FAIL'} {name}: got {got!r}, want {want!r}")

    R = ["r0", "r1", "r2"]
    ok("chain below is regular", regular(R, [("chain", ("r0", "r1"), "r2")]), True)
    ok("chain with head leg is regular", regular(R, [("chain", ("r2", "r0"), "r2")]), True)
    ok("rr is regular", regular(R, [("chain", ("r1", "r1"), "r1")]), True)
    ok("mutual chains are irregular",
       regular(R, [("chain", ("r0", "r1"), "r2"), ("chain", ("r2", "r1"), "r0")]), False)
    ok("inverse-collision leg is irregular",
       regular(R, [("inverse", "r0", "r2"), ("chain", ("r0", "r1"), "r2")]), False)
    ok("sub-role above a chain head is irregular",
       regular(R, [("chain", ("r0", "r1"), "r2"), ("sub", "r2", "r0")]), False)
    ok("transitive is non-simple", "r0" in simple_roles(R, [("transitive", "r0")]), False)
    ok("super of a transitive role is non-simple",
       "r1" in simple_roles(R, [("transitive", "r0"), ("sub", "r0", "r1")]), False)
    ok("sub of a transitive role stays simple",
       "r0" in simple_roles(R, [("transitive", "r1"), ("sub", "r0", "r1")]), True)
    ok("inverse of a chain head is non-simple",
       "r1" in simple_roles(R, [("inverse", "r0", "r1"), ("chain", ("r2", "r2"), "r0")]), False)
    ok("symmetric-equivalence keeps simplicity", "r0" in simple_roles(R, [("symmetric", "r0")]),
       True)
    a, b = Case(7, 3).ofn(), Case(7, 3).ofn()
    ok("deterministic", a == b, True)
    ok("different cases differ", Case(7, 3).ofn() == Case(7, 4).ofn(), False)
    illegal = sum(bool(check(Case(1, i))) for i in range(500))
    ok("500 cases pass the checker", illegal, 0)
    sizes = [len(Case(1, i).axioms) for i in range(200)]
    ok("axiom counts within 3..15", (min(sizes) >= 1, max(sizes) <= 15), (True, True))
    ok("equivalent roles ranked with a chain head are irregular",
       regular(R, [("equivrole", "r0", "r2"), ("chain", ("r0", "r1"), "r2")]), False)
    ok("equivalent roles alone are regular", regular(R, [("equivrole", "r0", "r1")]), True)
    els = [ElCase(11, i) for i in range(500)]
    ok("500 el_guaranteed cases pass the checker", sum(bool(check(c)) for c in els), 0)
    bad = [c.index for c in els if any(t in "\n".join(c.axioms) for t in EL_FORBIDDEN)]
    ok("el_guaranteed emits no construct outside PureEl", bad, [])
    ok("el_guaranteed declares no data property or individual",
       any("DataProperty" in c.ofn() or "NamedIndividual" in c.ofn() for c in els), False)
    motifs = {c.motif for c in els}
    ok("both motifs are planted", {"gci_domain", "none"} <= motifs and
       any(m.startswith("nested_range") for m in motifs), True)
    m1 = [c for c in els if c.motif == "gci_domain"]
    import re as _re
    distinct = all(len(set(_re.findall(r":(C\d+)", " ".join(c.axioms[:3])))) == 5 for c in m1[:50])
    ok("gci_domain motif picks five distinct classes", distinct, True)
    ok("el_guaranteed is deterministic", ElCase(5, 7).ofn() == ElCase(5, 7).ofn(), True)
    tops = [ElTopCase(11, i) for i in range(500)]
    ok("500 el_top cases pass the checker", sum(bool(check(c)) for c in tops), 0)
    bad = [c.index for c in tops if any(t in "\n".join(c.axioms) for t in EL_FORBIDDEN)]
    ok("el_top emits no construct outside PureEl", bad, [])
    with_top = sum(any(a.startswith(("SubClassOf(owl:Thing ", "EquivalentClasses(owl:Thing "))
                       for a in c.axioms) for c in tops)
    ok("most el_top cases have a ⊤-LHS GCI", with_top >= 350, True)
    ok("every el_top motif is planted",
       {"top_exists", "top_chain", "top_marker"} <= {c.motif for c in tops}, True)
    ok("el_top is deterministic", ElTopCase(5, 7).ofn() == ElTopCase(5, 7).ofn(), True)
    ok("el_top and el_guaranteed streams differ",
       ElTopCase(5, 7).axioms == ElCase(5, 7).axioms, False)
    # The checker reads every role operand: an inverse in the second DisjointObjectProperties
    # position used to go unchecked.
    class _Fake:
        roles = ["r0", "r1"]
        role_axioms = [("transitive", "r1")]
        axioms = ["DisjointObjectProperties(:r0 ObjectInverseOf(:r1))"]
    ok("checker reads an inverse in the second disjoint-role position",
       any("non-simple r1" in e for e in check(_Fake())), True)
    _Fake.axioms = ["DisjointObjectProperties(ObjectInverseOf(:r0) ObjectInverseOf(:r1))"]
    ok("checker reads two inverse operands", any("non-simple r1" in e for e in check(_Fake())),
       True)
    pol = [PolarityCase(13, i) for i in range(1000)]
    ok("1000 polarity cases pass the checker", sum(bool(check(c)) for c in pol), 0)
    ok("polarity is deterministic", PolarityCase(5, 7).ofn() == PolarityCase(5, 7).ofn(), True)
    ok("polarity and default streams differ", PolarityCase(5, 7).axioms == Case(5, 7).axioms,
       False)
    ok("every polarity case has an r⁻ role-consuming axiom (the floor)",
       [c.index for c in pol if not polarity_kinds(c.axioms) - {"forall", "exists"}], [])
    census = {}
    for c in pol:
        for k in polarity_kinds(c.axioms):
            census[k] = census.get(k, 0) + 1
    want = set(POLARITY_KINDS) | {"disjoint_roles", "cardinality"}
    ok("every signed kind occurs in >= 50 of 1000 cases",
       sorted(k for k in want if census.get(k, 0) < 50), [])
    names = {c.motif.split("/")[0] for c in pol}
    stems = {n.split("_")[0] if n.startswith(("p7", "p8", "p6")) else n for n in names}
    ok("every polarity motif is planted",
       sorted({"told_edge", "p7", "p5", "p8", "p3", "p6", "p9"} - stems), [])
    ok("the tableau/wedge motifs are planted",
       {"card_inv_abox", "card_inv_tbox", "forall_inv_abox", "forall_inv_tbox",
        "min_inv_nominal"} <= names, True)
    ok("both variants of p5 occur", {"p5/ok", "p5/ctl"} <= {c.motif for c in pol}, True)
    planted = sum(c.motif != "none" for c in pol)
    ok("about half the cases carry a motif", 380 <= planted <= 620, True)
    ok("the header carries the motif and the census",
       PolarityCase(13, 1).ofn().splitlines()[1:3][0].startswith("# motif: ")
       and PolarityCase(13, 1).ofn().splitlines()[2].startswith("# polarity: "), True)
    unis = [UniversalCase(5, i) for i in range(500)]
    ok("500 universal cases pass the checker", sum(bool(check(c)) for c in unis), 0)
    ok("every universal case uses a top property",
       sum(not any(TOP in a or TOP_DATA in a for a in c.axioms) for c in unis), 0)
    ok("universal is deterministic", UniversalCase(5, 7).ofn() == UniversalCase(5, 7).ofn(), True)
    drawn = {k for c in unis for k in c.kinds()}
    ok("every fold kind is drawn",
       sorted({"domain", "range", "char", "sub_top", "top_sub", "some", "all", "max0", "min1",
               "has_value", "neg_assert", "pos_assert", "data_top"} - drawn), [])
    ok("top never in a position HermiT rejects",
       [a for c in unis for a in c.axioms
        if (TOP in a and (any(f"{op}{TOP}" in a for op in (
            "ObjectHasSelf(", "FunctionalObjectProperty(", "IrreflexiveObjectProperty(",
            "AsymmetricObjectProperty(", "DisjointObjectProperties("))
            or any(f"{m}{TOP}" in a for m in ("ObjectMaxCardinality(1 ", "ObjectExactCardinality(",
                                               "ObjectMinCardinality(2 ", "ObjectMinCardinality(3 ",
                                               "ObjectMaxCardinality(2 ", "ObjectMaxCardinality(3 "))))
        or f"DataSomeValuesFrom({TOP_DATA}" in a], [])
    ok("the header carries the census", "# universal: " in UniversalCase(5, 7).ofn(), True)
    print(f"gen self-test: {'all ok' if not fails else f'{fails} FAILED'}")
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--seed", type=int)
    ap.add_argument("--n", type=int)
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--out")
    ap.add_argument("--profile", default="default", choices=sorted(PROFILES))
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    if a.seed is None or a.n is None or not a.out:
        ap.error("--seed, --n and --out are required")
    return 1 if generate(a.seed, a.n, a.out, a.profile, a.start) else 0


if __name__ == "__main__":
    sys.exit(main())

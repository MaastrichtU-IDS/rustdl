"""Type stubs for rustdl — sound OWL 2 DL (SROIQ) reasoner. See __init__.py."""

from collections.abc import Mapping
from typing import Optional

from . import examples as examples

__version__: str

# ── result types ────────────────────────────────────────────────────────────

class Classification:
    """Result of `classify` — the computed subsumption hierarchy."""

    @property
    def classes(self) -> list[str]:
        """Every declared class IRI."""
        ...
    @property
    def unsatisfiable(self) -> list[str]:
        """Classes proved equivalent to owl:Nothing."""
        ...
    @property
    def inconsistent(self) -> bool:
        """True iff the ontology is inconsistent (every class unsatisfiable)."""
        ...
    @property
    def timed_out_pairs(self) -> int:
        """Number of subsumption pairs that hit the per-pair timeout."""
        ...
    @property
    def complete(self) -> bool:
        """True iff no pair timed out (the hierarchy is exact)."""
        ...
    def is_subclass(self, sub: str, sup: str) -> bool:
        """True iff `sub ⊑ sup` is entailed."""
        ...
    def equivalent_classes(self, cls: str) -> list[str]:
        """Classes equivalent to `cls`."""
        ...
    def direct_subsumers(self, cls: str) -> list[str]:
        """Direct (Hasse-parent) super-classes of `cls`."""
        ...
    def subclasses_of(self, cls: str) -> list[str]:
        """All D with D ⊑ cls (reflexive + proper)."""
        ...
    def superclasses_of(self, cls: str) -> list[str]:
        """All D with cls ⊑ D (reflexive + proper)."""
        ...
    def __repr__(self) -> str: ...

class IncompleteClassificationWarning(UserWarning):
    """Emitted when classification hit the per-pair timeout (sound but possibly
    incomplete)."""

class IncompleteQueryWarning(UserWarning):
    """Emitted by the budgeted inferred queries (`disjoint_classes`,
    `same_individuals`, `different_individuals`, `object_property_values`)
    when the per-pair budget/probe was exhausted (sound but possibly
    incomplete)."""

class Root(Mapping[str, object]):
    iri: str
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    derives: tuple[str, ...]
    def to_dict(self) -> dict: ...

class Derived(Mapping[str, object]):
    iri: str
    roots: tuple[str, ...]
    def to_dict(self) -> dict: ...

class Inconsistency(Mapping[str, object]):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    def to_dict(self) -> dict: ...

class Diagnosis(Mapping[str, object]):
    consistent: bool
    unsatisfiable: tuple[str, ...]
    roots: tuple[Root, ...]
    derived: tuple[Derived, ...]
    inconsistency: Optional[Inconsistency]
    def to_dict(self) -> dict: ...

# ── exceptions ──────────────────────────────────────────────────────────────

class RustdlError(Exception):
    """Base exception for all rustdl errors."""

class ParseError(RustdlError):
    """OWL parser failure."""

class UnsupportedAxiomError(RustdlError):
    """An axiom uses a construct rustdl does not support."""

class UnknownClassError(RustdlError):
    """A queried IRI is not a declared class in the ontology."""

# ── core reasoning ──────────────────────────────────────────────────────────

def classify(
    path: str,
    *,
    per_pair_timeout_ms: int = 100,
    global_timeout_ms: int = 60000,
    saturation_only: bool = False,
    global_deadline_ms: int | None = None,
) -> Classification:
    """Classify the ontology at `path` (format auto-detected from the
    extension: ``.ofn`` | ``.owx`` | ``.owl`` | ``.rdf`` | ``.omn``).

    Bounded by default: ``per_pair_timeout_ms`` caps each ``sub ⊓ ¬sup``
    pair and ``global_timeout_ms`` caps the TOTAL wall (the backstop against
    wine-class hangs). Cutting is sound (no false subsumptions; real ones may
    be missing — check ``Classification.complete`` / ``.timed_out_pairs``).
    Set either to ``0`` to disable that bound; both ``0`` ⇒ unbounded/complete.
    ``global_deadline_ms`` is a deprecated alias for ``global_timeout_ms``."""
    ...

def classify_bytes(
    data: bytes,
    *,
    format: str,
    per_pair_timeout_ms: int = 100,
    global_timeout_ms: int = 60000,
    saturation_only: bool = False,
    global_deadline_ms: int | None = None,
) -> Classification:
    """Like `classify`, from in-memory bytes with explicit `format`
    (one of ``"ofn"``, ``"owx"``, ``"rdf-xml"``, ``"omn"``).
    ``global_deadline_ms`` is a deprecated alias for ``global_timeout_ms``."""
    ...

def is_consistent(path: str) -> bool:
    """True iff the ontology is consistent."""
    ...

def is_class_satisfiable(path: str, class_iri: str) -> bool:
    """True iff `class_iri` is satisfiable (not ⊑ ⊥)."""
    ...

def is_subclass_of(path: str, sub: str, sup: str) -> bool:
    """True iff `sub ⊑ sup` is entailed."""
    ...

def is_instance_of(path: str, class_iri: str, individual_iri: str) -> bool:
    """True iff `individual_iri` is an instance of `class_iri`."""
    ...

def instances_of(path: str, class_iri: str) -> list[str]:
    """Named individuals entailed to be instances of `class_iri`."""
    ...

def realize(path: str) -> dict[str, list[str]]:
    """Map each named individual to its most-specific entailed types."""
    ...

# ── inferred disjointness ───────────────────────────────────────────────────

def disjoint_classes(path: str) -> list[tuple[str, str]]:
    """Entailed disjoint named-class pairs (C ⊓ D unsatisfiable). Bounded by a
    1s per-pair deadline; emits `IncompleteQueryWarning` if the budget was
    exhausted."""
    ...

def disjoint_object_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint object property pairs."""
    ...

def disjoint_data_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint data property pairs."""
    ...

# ── inferred property hierarchy ─────────────────────────────────────────────

def object_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for object properties."""
    ...

def data_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for data properties."""
    ...

# ── inferred same/different individuals ─────────────────────────────────────

def same_individuals(path: str) -> list[list[str]]:
    """Groups of individuals proven equal (asserted + functional-forced + entailed).
    Bounded by a 1s per-pair deadline; emits `IncompleteQueryWarning` whenever any
    extension probe beyond the sound-complete seed ran."""
    ...

def different_individuals(path: str) -> list[tuple[str, str]]:
    """Pairs of individuals proven distinct ({a}⊓{b} unsatisfiable). Bounded by a
    1s per-pair deadline; emits `IncompleteQueryWarning` if the budget was
    exhausted."""
    ...

# ── inferred property values ────────────────────────────────────────────────

def object_property_values(path: str) -> list[tuple[str, str, str]]:
    """Inferred object property values (subject, property, object) over named
    individuals. Bounded by a 1s per-pair deadline; emits `IncompleteQueryWarning`
    if the budget was exhausted."""
    ...

def data_property_values(path: str) -> list[tuple[str, str, str, str]]:
    """Inferred data property values (subject, property, lexical, datatype)."""
    ...

# ── complex class-expression queries ────────────────────────────────────────

def class_expression_satisfiable(path: str, ce: str) -> bool:
    """True iff the Manchester-syntax class expression `ce` is satisfiable
    w.r.t. the ontology at `path` (resolved against the ontology's own prefix
    map). Emits `IncompleteQueryWarning` if the verdict is a sound
    under-approximation."""
    ...

def class_expression_entailed_subclass(path: str, sub_ce: str, sup_ce: str) -> bool:
    """True iff `sub_ce ⊑ sup_ce` is entailed w.r.t. the ontology at `path`
    (Manchester-syntax class expressions, resolved against the ontology's own
    prefix map). Emits `IncompleteQueryWarning` if the verdict is a sound
    under-approximation."""
    ...

def class_expression_instances(path: str, ce: str) -> list[str]:
    """Named individuals provably in the Manchester-syntax class expression
    `ce` w.r.t. the ontology at `path` (resolved against the ontology's own
    prefix map). Emits `IncompleteQueryWarning` if the result is a sound
    under-approximation."""
    ...

# ── conversion diagnostics ──────────────────────────────────────────────────

def dropped_axioms(path: str) -> dict[str, int]:
    """Kinds and counts of axioms conversion could not represent (a sound
    under-approximation)."""
    ...

# ── inference materialization ───────────────────────────────────────────────

def materialize_inferred_subclass_axioms(path: str) -> list[tuple[str, str]]:
    """Every entailed (sub, sup) class-subsumption pair."""
    ...

def materialize_inferred_class_assertions(path: str) -> list[tuple[str, str]]:
    """Every entailed (class, individual) most-specific class assertion."""
    ...

def materialize_inferred_property_assertions(path: str) -> list[tuple[str, str, str]]:
    """Inferred object property assertions (subject, property, object) over named
    individuals."""
    ...

def materialize_inferred_data_property_assertions(
    path: str,
) -> list[tuple[str, str, str, str, str]]:
    """Inferred data property assertions (subject, property, lexical, datatype, lang)."""
    ...

def materialize_inferred_subobjectproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred object property subsumption pairs (sub, sup)."""
    ...

def materialize_inferred_subdataproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred data property subsumption pairs (sub, sup)."""
    ...

def materialize_existential_successors(path: str) -> list[tuple[str, str, str, str]]:
    """Entailed existential successors as (subject, property, witness_blank, filler_class)
    — a blank-node representation of entailed `a : ∃R.C` (not ground triples)."""
    ...

# ── explanation & debugging ─────────────────────────────────────────────────

def justify(path: str, query: list[str]) -> list[str]:
    """One minimal justification (Manchester axioms) for `query`
    (e.g. ["unsat", c] / ["subclass", s, t] / ["inconsistent"])."""
    ...

def justify_all(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Up to `max` minimal justifications for `query`."""
    ...

def diagnose(path: str) -> tuple[bool, list[str], list[tuple[str, list[str]]]]:
    """(consistent, root_unsat_iris, [(derived_iri, [root_iri, ...]), ...])."""
    ...

def repair(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Minimal axiom-removal sets (Manchester) that break `query`."""
    ...

def render_manchester(path: str) -> list[str]:
    """Every logical axiom of the ontology at `path` as Manchester strings."""
    ...

def debug(path: str) -> Diagnosis:
    """One-call ontology diagnosis: consistency + root/derived unsat +
    per-root justifications + repairs."""
    ...

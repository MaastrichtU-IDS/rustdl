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
    def depth_capped_pairs(self) -> int:
        """Of `timed_out_pairs`, the probes cut by the tableau's search-depth
        limit rather than a timeout; a larger timeout does not recover them."""
        ...
    @property
    def no_verdict_pairs(self) -> int:
        """Of `timed_out_pairs`, probes with NO deadline that ended in
        `NoVerdict` (#462): an internal limit, likely a reasoner bug (#449).
        A `NoVerdict` on a probe with a deadline is the routine search-depth
        cut (`depth_capped_pairs`), not counted here."""
        ...
    @property
    def complete(self) -> bool:
        """True iff no pair timed out. `True` does NOT mean no entailment is
        missing: that is the stronger `completeness_guaranteed`. See also
        `wedge_hierarchy_blind`: a result can be `complete` (no timeout) and
        still miss entailments."""
        ...
    @property
    def completeness_guaranteed(self) -> bool:
        """True iff the hierarchy is guaranteed complete (no missed
        subsumptions): pure EL, no timeout, no `wedge_hierarchy_blind`, no
        `wedge_clauses_refused`, no `derived_same_undecided`, no
        `consistency_undetermined` and no dropped axiom. Stronger than
        `complete`, which only says no pair timed out."""
        ...
    @property
    def prep_timed_out(self) -> bool:
        """True iff the global deadline ran out during preparation, so only a
        partial saturation closure is reported and consistency was not fully
        checked (#162). Implies `complete is False`."""
        ...
    @property
    def conversion_timed_out(self) -> bool:
        """True iff, with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, the global deadline
        ran out during conversion, so only part of the ontology was read and
        axioms past the cut were neither reasoned over nor counted as dropped
        (#162). Implies `prep_timed_out`."""
        ...
    @property
    def consistency_undetermined(self) -> bool:
        """True iff the inconsistency pre-check reached no verdict (#162), or
        classify's consistency probe was admitted and gave up (#446), so
        `inconsistent is False` means "no clash found", not "proven
        consistent"."""
        ...
    @property
    def relies_on_trusted_consistency(self) -> bool:
        """True iff `inconsistent is False` rests on a consistency-wedge model
        that no main-tableau run verified (#429). Not a give-up; `classify`
        emits `TrustedConsistencyWarning`."""
        ...
    @property
    def wedge_hierarchy_blind(self) -> bool:
        """True iff the wedge ran without role-hierarchy matching (Layer A
        off) over a non-trivial role hierarchy, so entailments needing
        sub-role, inverse or symmetric reasoning may be missing (#214)."""
        ...
    @property
    def wedge_clauses_refused(self) -> int:
        """How many clauses the wedge cannot match. A refused clause never
        fires, so entailments that need it may be missing (#353)."""
        ...
    @property
    def derived_same_undecided(self) -> int:
        """How many candidate individual equalities classify did not decide,
        so entailments that need them may be missing (#354)."""
        ...
    @property
    def matrix_closure_added_pairs(self) -> int:
        """How many pairs the transitive closure of the recorded relation added
        (#448, `RUSTDL_CLASSIFY_CLOSE_MATRIX`, default on): pairs recorded
        "not subsumed" that recorded subsumptions imply. Each is sound; `complete`
        is unchanged. 0 when the closure is off (`=0`)."""
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
    """Emitted when the hierarchy may be missing subsumptions (sound but
    possibly incomplete): a timeout fired, the wedge ran without
    role-hierarchy matching (`Classification.wedge_hierarchy_blind`), the
    wedge refused clauses (`Classification.wedge_clauses_refused`), or
    candidate individual equalities went undecided
    (`Classification.derived_same_undecided`)."""

class IncompleteQueryWarning(UserWarning):
    """Emitted by the budgeted inferred queries (`disjoint_classes`,
    `same_individuals`, `different_individuals`, `object_property_values`)
    when the per-pair budget/probe was exhausted (sound but possibly
    incomplete).

    #510: when an instance probe was cut by the tableau's decision-depth limit
    (`is_instance_of`, `instances_of`, `class_expression_instances`, `realize`, the
    two realize-backed `materialize_*` calls, and the explanation calls' instance
    checks), the message says so: a longer `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or a
    larger node cap does not help there, and `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` (the
    deadline-free probe, with no time limit) may. When a deadline or the node cap cut
    an `is_instance_of` probe after it had hit that limit (#510 C: early abandon off,
    or a loaded host), the message says both remedies may help."""

class DirectTypesUnverifiedWarning(UserWarning):
    """Emitted by `realize` when two most-specific types of an individual could
    not be compared, so a listed type may be a superclass of another (#388). Not
    an `IncompleteQueryWarning`: no type is missing."""

class WitnessPruneWarning(UserWarning):
    """Emitted by `realize`, `materialize_inferred_class_assertions` and
    `materialize_existential_successors` when the pseudo-model prune answered
    "not a type" from a witness with a known reason to lack an entailed type
    (#387). A type may be missing although no probe was cut. Its absence means
    no known reason for loss, not a guarantee."""

class StructuralGapWarning(UserWarning):
    """Emitted by the structural property queries (`object_property_hierarchy`,
    `data_property_hierarchy`, `disjoint_object_properties`,
    `disjoint_data_properties`, `data_property_values` and the matching
    `materialize_inferred_*` functions) when the ontology has constructs they do
    not reason with (#389, #390). Not an `IncompleteQueryWarning`: no budget was
    cut."""

class TrustedConsistencyWarning(UserWarning):
    """Emitted when an answer rests on a consistency verdict the wedge found but
    did not prove: by `is_consistent` for that verdict itself (#392), and by
    `is_class_satisfiable`, `is_subclass_of`, `class_expression_satisfiable` and
    `class_expression_entailed_subclass` when the answer was computed without the
    ABox on its strength (#401). Not an `IncompleteQueryWarning`: nothing was
    cut."""

class ConsistencyUndeterminedWarning(UserWarning):
    """Emitted when a positive answer's KB consistency stayed undetermined (#369),
    so it may be the vacuous answer of an inconsistent KB: by
    `is_class_satisfiable` ("unsatisfiable"), `is_subclass_of` (entailed),
    `is_instance_of`,
    `instances_of`, `same_individuals`, `different_individuals`, the
    `class_expression_*` queries, `object_property_values`,
    `data_property_values`, the property disjointness queries and the
    `materialize_inferred_*` property functions. Not an
    `IncompleteQueryWarning`: nothing was cut."""

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
    """True iff the ontology is consistent. A give-up (no clash found within
    budget) returns `True` and emits `IncompleteQueryWarning`. A `True` resting
    only on the wedge's unproved model emits `TrustedConsistencyWarning` (#392)."""
    ...

def is_class_satisfiable(path: str, class_iri: str) -> bool:
    """True iff `class_iri` is satisfiable (not ⊑ ⊥). If the probe gave up (node
    cap), returns `True` and emits `IncompleteQueryWarning`. Raises `RustdlError`
    on a KB found inconsistent (#428), except for `owl:Thing`, which is the
    consistency check; emits `ConsistencyUndeterminedWarning` on an
    "unsatisfiable" whose KB consistency stayed undetermined. A positive answer whose consistency check's own probe stalled (likely a
    reasoner bug, #449) also emits `IncompleteQueryWarning` with that text (#493)."""
    ...

def is_subclass_of(path: str, sub: str, sup: str) -> bool:
    """True iff `sub ⊑ sup` is entailed. If the probe gave up (node cap),
    returns `False` and emits `IncompleteQueryWarning`. Raises `RustdlError` on a
    KB found inconsistent (#428), except for `owl:Thing ⊑ owl:Nothing`, which is
    the consistency check; emits `ConsistencyUndeterminedWarning` on an entailed
    answer whose KB consistency stayed undetermined. A positive answer whose consistency check's own probe stalled (likely a
    reasoner bug, #449) also emits `IncompleteQueryWarning` with that text (#493)."""
    ...

def is_instance_of(path: str, class_iri: str, individual_iri: str) -> bool:
    """True iff `individual_iri` is an instance of `class_iri`. If the probe was
    cut, returns `False` and emits `IncompleteQueryWarning`. A decision-depth cut (or a
    budget cut after a depth-limit hit, #510 C) is first retried on the deep path (`RUSTDL_INSTANCE_DEPTH_FALLBACK_MS`, #510). A
    probe that ended in NoVerdict on an internal limit (likely a reasoner bug, #449) raises
    `RustdlError` instead (#493). A positive answer whose consistency check's own probe stalled (likely a
    reasoner bug, #449) also emits `IncompleteQueryWarning` with that text (#493)."""
    ...

def instances_of(path: str, class_iri: str) -> list[str]:
    """Named individuals entailed to be instances of `class_iri`. Emits
    `IncompleteQueryWarning` when a probe was cut (members may be missing); for
    a probe that ended in NoVerdict on an internal limit (likely a reasoner bug,
    #449) the warning says so, and the members are still returned (#493)."""
    ...

def realize(path: str) -> dict[str, list[str]]:
    """Map each named individual to its most-specific entailed types. Emits
    `IncompleteQueryWarning` when an instance probe was cut, and
    `DirectTypesUnverifiedWarning` when two most-specific types could not be
    compared, and `WitnessPruneWarning` when the pseudo-model prune may have
    dropped an entailed type. A probe that ended in NoVerdict on an internal
    limit (likely a reasoner bug, #449) is reported by `IncompleteQueryWarning`
    with that text; the types are still returned (#493)."""
    ...

# ── inferred disjointness ───────────────────────────────────────────────────

def disjoint_classes(path: str) -> list[tuple[str, str]]:
    """Entailed disjoint named-class pairs (C ⊓ D unsatisfiable). Bounded by a
    1s per-pair deadline; emits `IncompleteQueryWarning` if the budget was
    exhausted."""
    ...

def disjoint_object_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint object property pairs. Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

def disjoint_data_properties(path: str) -> list[tuple[str, str]]:
    """Told-disjoint data property pairs. Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

# ── inferred property hierarchy ─────────────────────────────────────────────

def object_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for object properties. Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

def data_property_hierarchy(path: str) -> tuple[list[list[str]], list[tuple[str, str]]]:
    """(equivalent_groups, direct_subsumptions) for data properties. Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
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

def data_property_values(path: str) -> list[tuple[str, str, str, str, str]]:
    """Inferred data property values (subject, property, lexical, datatype, lang).

    `lang` is the language tag for `rdf:langString` values and EMPTY otherwise.
    It is part of the literal's IDENTITY, not decoration: `"bonjour"@fr` and
    `"bonjour"@de` are distinct literals. Until issue #72 the tag was dropped
    before dedup, which merged such rows and lost an assertion outright when
    they shared a subject.

    Structural; emits `StructuralGapWarning` when the ontology has constructs it
    ignores.
    """
    ...

# ── complex class-expression queries ────────────────────────────────────────

def class_expression_satisfiable(path: str, ce: str) -> bool:
    """True iff the Manchester-syntax class expression `ce` is satisfiable
    w.r.t. the ontology at `path` (resolved against the ontology's own prefix
    map). If the probe gave up (node cap), returns `True` and emits
    `IncompleteQueryWarning`; an answer off pure EL does not warn. A positive answer whose consistency check's own probe stalled (likely a
    reasoner bug, #449) also emits `IncompleteQueryWarning` with that text (#493)."""
    ...

def class_expression_entailed_subclass(path: str, sub_ce: str, sup_ce: str) -> bool:
    """True iff `sub_ce ⊑ sup_ce` is entailed w.r.t. the ontology at `path`
    (Manchester-syntax class expressions, resolved against the ontology's own
    prefix map). If the probe gave up (node cap), returns `False` and emits
    `IncompleteQueryWarning`; an answer off pure EL does not warn. A positive answer whose consistency check's own probe stalled (likely a
    reasoner bug, #449) also emits `IncompleteQueryWarning` with that text (#493)."""
    ...

def class_expression_instances(path: str, ce: str) -> list[str]:
    """Named individuals provably in the Manchester-syntax class expression
    `ce` w.r.t. the ontology at `path` (resolved against the ontology's own
    prefix map). Emits `IncompleteQueryWarning` when an instance probe was cut
    (members may be missing; those returned are sound); an answer off pure EL
    does not warn."""
    ...

# ── conversion diagnostics ──────────────────────────────────────────────────

def dropped_axioms(path: str) -> dict[str, int]:
    """Kinds and counts of axioms conversion could not represent (a sound
    under-approximation)."""
    ...

# ── inference materialization ───────────────────────────────────────────────

def materialize_inferred_subclass_axioms(path: str) -> list[tuple[str, str]]:
    """Every entailed (sub, sup) class-subsumption pair, read off `classify`. Emits
    the same `IncompleteClassificationWarning`s as `classify`."""
    ...

def materialize_inferred_class_assertions(path: str) -> list[tuple[str, str]]:
    """A (class, individual) pair for each most-specific entailed type, as `realize`
    reports them (not every entailed type). Emits `IncompleteQueryWarning` when a
    probe was cut, `DirectTypesUnverifiedWarning` when a listed type may be a
    superclass of another, and `WitnessPruneWarning` when the pseudo-model prune
    may have dropped a type."""
    ...

def materialize_inferred_property_assertions(path: str) -> list[tuple[str, str, str]]:
    """Inferred object property assertions (subject, property, object) over named
    individuals. Emits `ConsistencyUndeterminedWarning` (#369)."""
    ...

def materialize_inferred_data_property_assertions(
    path: str,
) -> list[tuple[str, str, str, str, str]]:
    """Inferred data property assertions (subject, property, lexical, datatype, lang). Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

def materialize_inferred_subobjectproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred object property subsumption pairs (sub, sup). Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

def materialize_inferred_subdataproperty_axioms(path: str) -> list[tuple[str, str]]:
    """Inferred data property subsumption pairs (sub, sup). Structural; emits
    `StructuralGapWarning` when the ontology has constructs it ignores."""
    ...

def materialize_existential_successors(path: str) -> list[tuple[str, str, str, str]]:
    """Entailed existential successors as (subject, property, witness_blank, filler_class)
    — a blank-node representation of entailed `a : ∃R.C` (not ground triples). Emits
    `IncompleteQueryWarning` when a probe of the `realize` run underneath was cut, and
    `WitnessPruneWarning` when its pseudo-model prune may have dropped a type."""
    ...

# ── explanation & debugging ─────────────────────────────────────────────────

def justify(path: str, query: list[str]) -> list[str]:
    """One minimal justification (Manchester axioms) for `query`
    (e.g. ["unsat", c] / ["subclass", s, t] / ["inconsistent"]). Emits
    `IncompleteQueryWarning` when a reasoner check gave up: an empty list then
    means unknown, and a justification may not be minimal."""
    ...

def justify_all(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Up to `max` minimal justifications for `query`. Emits
    `IncompleteQueryWarning` when a reasoner check gave up (see `justify`;
    justifications may also be missing)."""
    ...

def diagnose(path: str) -> tuple[bool, list[str], list[tuple[str, list[str]]]]:
    """(consistent, root_unsat_iris, [(derived_iri, [root_iri, ...]), ...]).
    Emits `IncompleteQueryWarning` when a reasoner check under it gave up
    (a class's unsatisfiability check, or the consistency check of an
    all-classes-unsatisfiable ontology)."""
    ...

def repair(path: str, query: list[str], max: int = 10) -> list[list[str]]:
    """Minimal axiom-removal sets (Manchester) that break `query`, each verified.
    Emits `IncompleteQueryWarning` when a reasoner check gave up: an empty list
    may then mean unknown, and repairs may be missing."""
    ...

def render_manchester(path: str) -> list[str]:
    """Every logical axiom of the ontology at `path` as Manchester strings."""
    ...

class PreparedOntology:
    """A loaded ontology with per-ontology justification state precomputed once
    (parse + axiom split + fragment classification). Reuse it to justify many
    queries without re-doing that full-ontology work on each call. Use it from
    the thread that created it."""
    def justify(self, query: list[str]) -> list[str]:
        """One minimal justification (Manchester axioms) for `query`. Emits
        `IncompleteQueryWarning` when a reasoner check gave up."""
        ...
    def justify_all(self, query: list[str], max: int = 10) -> list[list[str]]:
        """Up to `max` minimal justifications for `query`. Emits
        `IncompleteQueryWarning` when a reasoner check gave up."""
        ...

def prepare(path: str) -> PreparedOntology:
    """Load the ontology at `path` and precompute its justification state."""
    ...

def prepare_bytes(data: bytes, *, format: str) -> PreparedOntology:
    """Like `prepare`, from in-memory bytes (`format`: "ofn"|"owx"|"rdf-xml"|"omn")."""
    ...

def debug(path: str) -> Diagnosis:
    """One-call ontology diagnosis: consistency + root/derived unsat +
    per-root justifications + repairs."""
    ...

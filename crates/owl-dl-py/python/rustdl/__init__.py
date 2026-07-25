"""
rustdl — sound, performant OWL 2 DL (SROIQ) reasoner.

Python bindings for the rustdl Rust crate. Install via
`pip install rustdl`; import as `import rustdl`. See
https://github.com/MaastrichtU-IDS/rustdl for the full project.
"""

import warnings as _warnings

# Native extension built by PyO3 + maturin
from rustdl._native import (
    __version__ as __version__,
    Classification as Classification,
    classify as _classify_native,
    classify_bytes as _classify_bytes_native,
    is_consistent as is_consistent,
    is_class_satisfiable as is_class_satisfiable,
    is_subclass_of as is_subclass_of,
    is_instance_of as is_instance_of,
    instances_of as instances_of,
    realize as realize,
    disjoint_classes as _disjoint_classes_native,
    disjoint_object_properties as disjoint_object_properties,
    disjoint_data_properties as disjoint_data_properties,
    object_property_hierarchy as object_property_hierarchy,
    data_property_hierarchy as data_property_hierarchy,
    same_individuals as _same_individuals_native,
    different_individuals as _different_individuals_native,
    object_property_values as _object_property_values_native,
    data_property_values as data_property_values,
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
    materialize_inferred_subclass_axioms as materialize_inferred_subclass_axioms,
    materialize_inferred_class_assertions as materialize_inferred_class_assertions,
    materialize_inferred_property_assertions as materialize_inferred_property_assertions,
    materialize_inferred_data_property_assertions as materialize_inferred_data_property_assertions,
    materialize_inferred_subobjectproperty_axioms as materialize_inferred_subobjectproperty_axioms,
    materialize_inferred_subdataproperty_axioms as materialize_inferred_subdataproperty_axioms,
    materialize_existential_successors as materialize_existential_successors,
    justify as justify,
    justify_all as justify_all,
    diagnose as diagnose,
    repair as repair,
    render_manchester as render_manchester,
)

from ._results import (
    Diagnosis as Diagnosis,
    Root as Root,
    Derived as Derived,
    Inconsistency as Inconsistency,
)

from . import examples as examples


def _subclasses_of(self: "Classification", cls: str) -> list[str]:
    """All classes D in the ontology with D ⊑ cls (reflexive + proper).

    Pure-Python helper. O(N) over Classification.classes per call.
    """
    return [d for d in self.classes if self.is_subclass(d, cls)]


def _superclasses_of(self: "Classification", cls: str) -> list[str]:
    """All classes D in the ontology with cls ⊑ D (reflexive + proper).

    Pure-Python helper. O(N) over Classification.classes per call.
    """
    return [d for d in self.classes if self.is_subclass(cls, d)]


# Bind onto the PyO3 class so the API is symmetric:
# `result.subclasses_of(...)` lives next to `result.is_subclass(...)`.
Classification.subclasses_of = _subclasses_of  # type: ignore[attr-defined]
Classification.superclasses_of = _superclasses_of  # type: ignore[attr-defined]


class IncompleteClassificationWarning(UserWarning):
    """Raised when classification hit a timeout (per-pair or global), so
    the returned hierarchy is a sound under-approximation (no false
    subsumptions, but real ones may be missing). Silence with the
    standard `warnings` module, or pass `per_pair_timeout_ms=0,
    global_timeout_ms=0` to classify for the complete (unbounded)
    result."""


def _warn_if_incomplete(result: "Classification") -> "Classification":
    n = result.timed_out_pairs
    if n:
        _warnings.warn(
            f"{n} class pair(s) exceeded the timeout and were recorded as "
            "'not subsumed' — this classification may be missing real subsumptions. "
            "It is still sound (no false subsumptions). Pass per_pair_timeout_ms=0, "
            "global_timeout_ms=0 for the complete (unbounded) result, or check "
            "result.complete / result.timed_out_pairs.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
    return result


class IncompleteQueryWarning(UserWarning):
    """Raised by the budgeted inferred queries (`disjoint_classes`,
    `same_individuals`, `different_individuals`, `object_property_values`)
    when the reasoner's per-pair budget/probe was exhausted, so the returned
    list is a sound under-approximation (no false pairs/groups/triples, but
    real ones may be missing). Mirrors `IncompleteClassificationWarning`'s
    convention for `classify`. Silence with the standard `warnings` module."""


def _warn_if_query_incomplete(name: str, incomplete: bool) -> None:
    if incomplete:
        _warnings.warn(
            f"{name} result may be incomplete (budget/fragment exhausted) — "
            "sound under-approximation: no false entries, but real ones may be "
            "missing.",
            IncompleteQueryWarning,
            stacklevel=3,
        )


def disjoint_classes(path):
    """Entailed disjoint named-class pairs `(c, d)` — `C ⊓ D` is proven
    unsatisfiable. Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`)."""
    pairs, incomplete = _disjoint_classes_native(path)
    _warn_if_query_incomplete("disjoint_classes", incomplete)
    return pairs


def same_individuals(path):
    """Entailed same-individual equivalence groups (asserted +
    functional-forced + entailed). Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`) — note this fires whenever ANY extension probe
    beyond the sound-complete seed ran, even if no new group was found."""
    groups, incomplete = _same_individuals_native(path)
    _warn_if_query_incomplete("same_individuals", incomplete)
    return groups


def different_individuals(path):
    """Entailed different-individual pairs `(a, b)` — `{a} ⊓ {b}` is proven
    unsatisfiable. Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`)."""
    pairs, incomplete = _different_individuals_native(path)
    _warn_if_query_incomplete("different_individuals", incomplete)
    return pairs


def object_property_values(path):
    """Inferred object property values `(subject, property, object)` over
    named individuals. Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`)."""
    triples, incomplete = _object_property_values_native(path)
    _warn_if_query_incomplete("object_property_values", incomplete)
    return triples


def _resolve_global_timeout(global_timeout_ms, global_deadline_ms):
    """Reconcile the canonical `global_timeout_ms` with the deprecated
    `global_deadline_ms` alias (kept working for backward compatibility)."""
    if global_deadline_ms is not None:
        _warnings.warn(
            "global_deadline_ms is deprecated; use global_timeout_ms instead.",
            DeprecationWarning,
            stacklevel=3,
        )
        return global_deadline_ms
    return global_timeout_ms


def classify(
    path,
    *,
    per_pair_timeout_ms=100,
    global_timeout_ms=60000,
    saturation_only=False,
    global_deadline_ms=None,
):
    """Classify the ontology at `path` (format auto-detected from the
    extension: .ofn / .owx / .owl / .rdf / .omn).

    Bounded by default so it can't hang on hard (wine-class) ontologies:
    `per_pair_timeout_ms` bounds each subsumption test (default 100), and
    `global_timeout_ms` bounds the TOTAL wall (default 60000 = 60s). Set
    either to `0` to disable that bound; both `0` = unbounded/complete.
    Pairs cut by a timeout are recorded as "not subsumed" — sound, but the
    result may be incomplete; an `IncompleteClassificationWarning` is
    emitted when that happens, and `result.complete` /
    `result.timed_out_pairs` report it. `saturation_only=True` skips the
    tableau (EL-closure under-approximation; fast).

    `global_deadline_ms` is a deprecated alias for `global_timeout_ms`."""
    global_timeout_ms = _resolve_global_timeout(global_timeout_ms, global_deadline_ms)
    return _warn_if_incomplete(
        _classify_native(
            path,
            per_pair_timeout_ms=per_pair_timeout_ms,
            global_deadline_ms=global_timeout_ms,
            saturation_only=saturation_only,
        )
    )


def classify_bytes(
    data,
    *,
    format,
    per_pair_timeout_ms=100,
    global_timeout_ms=60000,
    saturation_only=False,
    global_deadline_ms=None,
):
    """Like `classify`, but from in-memory `data` with an explicit
    `format` ("ofn" | "owx" | "rdf-xml" | "omn"). See `classify` for the
    timeout/completeness semantics. `global_deadline_ms` is a deprecated
    alias for `global_timeout_ms`."""
    global_timeout_ms = _resolve_global_timeout(global_timeout_ms, global_deadline_ms)
    return _warn_if_incomplete(
        _classify_bytes_native(
            data,
            format=format,
            per_pair_timeout_ms=per_pair_timeout_ms,
            global_deadline_ms=global_timeout_ms,
            saturation_only=saturation_only,
        )
    )


def debug(path):
    """One-call ontology diagnosis → a Diagnosis result object.

    Supports attribute access (d.consistent, d.roots[0].justification,
    d.inconsistency.repairs) AND legacy dict access
    (d["roots"][0]["justification"], dict(d), iteration). For JSON use
    json.dumps(d.to_dict()).

    Consistent ontology → Diagnosis(consistent=True, unsatisfiable=..., roots=[Root...],
    derived=[Derived...], inconsistency=None). Inconsistent → consistent=False, empties,
    inconsistency=Inconsistency(...). Read-only; sound by construction."""
    consistent, roots, derived = diagnose(path)
    if not consistent:
        return Diagnosis(
            consistent=False,
            unsatisfiable=(),
            roots=(),
            derived=(),
            inconsistency=Inconsistency(
                justification=tuple(justify(path, ["inconsistent"])),
                repairs=tuple(tuple(r) for r in repair(path, ["inconsistent"], 10)),
            ),
        )
    root_objs = tuple(
        Root(
            iri=r,
            justification=tuple(justify(path, ["unsat", r])),
            repairs=tuple(tuple(x) for x in repair(path, ["unsat", r], 10)),
            derives=tuple(d for (d, rs) in derived if r in rs),
        )
        for r in roots
    )
    return Diagnosis(
        consistent=True,
        unsatisfiable=tuple(list(roots) + [d for (d, _) in derived]),
        roots=root_objs,
        derived=tuple(Derived(iri=d, roots=tuple(rs)) for (d, rs) in derived),
        inconsistency=None,
    )


__all__ = [
    "__version__",
    "examples",
    "Classification",
    "IncompleteClassificationWarning",
    "IncompleteQueryWarning",
    "classify",
    "classify_bytes",
    "is_consistent",
    "is_class_satisfiable",
    "is_subclass_of",
    "is_instance_of",
    "instances_of",
    "realize",
    "disjoint_classes",
    "disjoint_object_properties",
    "disjoint_data_properties",
    "object_property_hierarchy",
    "data_property_hierarchy",
    "same_individuals",
    "different_individuals",
    "object_property_values",
    "data_property_values",
    "RustdlError",
    "ParseError",
    "UnsupportedAxiomError",
    "UnknownClassError",
    "materialize_inferred_subclass_axioms",
    "materialize_inferred_class_assertions",
    "materialize_inferred_property_assertions",
    "materialize_inferred_data_property_assertions",
    "materialize_inferred_subobjectproperty_axioms",
    "materialize_inferred_subdataproperty_axioms",
    "materialize_existential_successors",
    "justify",
    "justify_all",
    "diagnose",
    "repair",
    "render_manchester",
    "debug",
    "Diagnosis",
    "Root",
    "Derived",
    "Inconsistency",
]

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
    class_expression_satisfiable as _class_expression_satisfiable_native,
    class_expression_entailed_subclass as _class_expression_entailed_subclass_native,
    class_expression_instances as _class_expression_instances_native,
    dropped_axioms as dropped_axioms,
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
    prepare as prepare,
    prepare_bytes as prepare_bytes,
    PreparedOntology as PreparedOntology,
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
    """Raised when the returned hierarchy may be missing real subsumptions.
    It is still a sound under-approximation (no false subsumptions). Two
    causes, each with its own remedy:

    * a timeout fired (`result.complete is False`): pass
      `per_pair_timeout_ms=0, global_timeout_ms=0` for the unbounded result;
    * the global budget ran out during preparation
      (`result.prep_timed_out is True`): only a partial saturation closure is
      reported; give a larger `global_timeout_ms`, or leave
      `RUSTDL_HARD_GLOBAL_DEADLINE` unset;
    * the wedge ran without role-hierarchy matching
      (`result.wedge_hierarchy_blind is True`): Layer A was off, by
      `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0` or because the role closure exceeded
      `RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`; raising that cap trades
      time for the missing entailments.

    `result.consistency_undetermined` is reported but not warned about, matching
    the CLI, which notes it only in the banner.

    Silence with the standard `warnings` module."""


def _warn_if_incomplete(result: "Classification") -> "Classification":
    n = result.timed_out_pairs
    if result.prep_timed_out:
        # #162: the cut preparation is counted as one timed-out "pair" so that
        # `complete` reads False; report it as what it is (#228 review).
        n -= 1
        _warnings.warn(
            "the global budget ran out during preparation, so only a partial "
            "saturation closure is reported (it may be empty) and consistency "
            "was not checked. It is still sound (no false subsumptions); check "
            "result.prep_timed_out.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
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
    if result.wedge_hierarchy_blind:
        # #214: the wedge ran without its role-hierarchy matching (Layer A off),
        # so completeness is not guaranteed even with no timeout.
        _warnings.warn(
            "the classification ran without role-hierarchy matching (Layer A is "
            "off: RUSTDL_CLASSIFY_ROLE_HIERARCHY=0, or the role closure exceeded "
            "RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE), so entailments that need "
            "sub-role, inverse or symmetric reasoning may be missing. It is still "
            "sound (no false subsumptions); check result.wedge_hierarchy_blind.",
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


def class_expression_satisfiable(path, ce):
    """True iff the Manchester-syntax class expression `ce` is satisfiable
    w.r.t. the ontology at `path` (`ce` is resolved against the ontology's
    own prefix map, e.g. `:A`). Emits `IncompleteQueryWarning` when the
    verdict is a sound under-approximation (see `IncompleteQueryWarning`)."""
    holds, incomplete = _class_expression_satisfiable_native(path, ce)
    _warn_if_query_incomplete("class_expression_satisfiable", incomplete)
    return holds


def class_expression_entailed_subclass(path, sub_ce, sup_ce):
    """True iff `sub_ce ⊑ sup_ce` is entailed w.r.t. the ontology at `path`
    (both Manchester-syntax class expressions, resolved against the
    ontology's own prefix map). Emits `IncompleteQueryWarning` when the
    verdict is a sound under-approximation (see `IncompleteQueryWarning`)."""
    holds, incomplete = _class_expression_entailed_subclass_native(path, sub_ce, sup_ce)
    _warn_if_query_incomplete("class_expression_entailed_subclass", incomplete)
    return holds


def class_expression_instances(path, ce):
    """Named individuals provably in the Manchester-syntax class expression
    `ce` w.r.t. the ontology at `path` (resolved against the ontology's own
    prefix map). Emits `IncompleteQueryWarning` when the result is a sound
    under-approximation (see `IncompleteQueryWarning`)."""
    individuals, incomplete = _class_expression_instances_native(path, ce)
    _warn_if_query_incomplete("class_expression_instances", incomplete)
    return individuals


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
    `result.timed_out_pairs` report it. The same warning is emitted when the
    wedge ran without role-hierarchy matching (`result.wedge_hierarchy_blind`),
    which no timeout setting fixes. `saturation_only=True` skips the
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
    "class_expression_satisfiable",
    "class_expression_entailed_subclass",
    "class_expression_instances",
    "dropped_axioms",
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
    "prepare",
    "prepare_bytes",
    "PreparedOntology",
    "debug",
    "Diagnosis",
    "Root",
    "Derived",
    "Inconsistency",
]

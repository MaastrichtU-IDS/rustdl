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
    is_consistent as _is_consistent_native,
    is_class_satisfiable as _is_class_satisfiable_native,
    is_subclass_of as _is_subclass_of_native,
    is_instance_of as _is_instance_of_native,
    instances_of as _instances_of_native,
    realize as _realize_native,
    disjoint_classes as _disjoint_classes_native,
    disjoint_object_properties as _disjoint_object_properties_native,
    disjoint_data_properties as _disjoint_data_properties_native,
    object_property_hierarchy as _object_property_hierarchy_native,
    data_property_hierarchy as _data_property_hierarchy_native,
    same_individuals as _same_individuals_native,
    different_individuals as _different_individuals_native,
    object_property_values as _object_property_values_native,
    data_property_values as _data_property_values_native,
    class_expression_satisfiable as _class_expression_satisfiable_native,
    class_expression_entailed_subclass as _class_expression_entailed_subclass_native,
    class_expression_instances as _class_expression_instances_native,
    dropped_axioms as dropped_axioms,
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
    materialize_inferred_subclass_axioms as _materialize_inferred_subclass_axioms_native,
    materialize_inferred_class_assertions as _materialize_inferred_class_assertions_native,
    materialize_inferred_property_assertions as _materialize_inferred_property_assertions_native,
    materialize_inferred_data_property_assertions as _materialize_inferred_data_property_assertions_native,
    materialize_inferred_subobjectproperty_axioms as _materialize_inferred_subobjectproperty_axioms_native,
    materialize_inferred_subdataproperty_axioms as _materialize_inferred_subdataproperty_axioms_native,
    materialize_existential_successors as _materialize_existential_successors_native,
    justify as _justify_native,
    justify_all as _justify_all_native,
    diagnose as _diagnose_native,
    repair as _repair_native,
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
    It is still a sound under-approximation (no false subsumptions). Three
    causes, each with its own remedy:

    * a timeout fired (`result.complete is False`): pass
      `per_pair_timeout_ms=0, global_timeout_ms=0` for the unbounded result;
    * a probe hit the tableau's search-depth limit
      (`result.depth_capped_pairs > 0`, #288): a larger `per_pair_timeout_ms`
      does not help; the unbounded run has a far larger limit but may not terminate;
    * the global budget ran out during preparation
      (`result.prep_timed_out is True`): only a partial saturation closure is
      reported; give a larger `global_timeout_ms`, or `0` for no budget. With
      `RUSTDL_HARD_GLOBAL_DEADLINE=1` the cut can come earlier, during
      conversion (`result.conversion_timed_out is True`), and then only part
      of the ontology was read;
    * the wedge ran without role-hierarchy matching
      (`result.wedge_hierarchy_blind is True`): Layer A was off, by
      `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0` or because the role closure exceeded
      `RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`; raising that cap trades
      time for the missing entailments;
    * the wedge cannot match some clauses' bodies
      (`result.wedge_clauses_refused > 0`): those clauses never fire (#353);
    * candidate individual equalities went undecided
      (`result.derived_same_undecided > 0`): a probe was cut or capped (#354).

    `result.consistency_undetermined` is reported but not warned about, matching
    the CLI, which notes it only in the banner.

    Silence with the standard `warnings` module."""


def _warn_if_incomplete(result: "Classification") -> "Classification":
    n = result.timed_out_pairs
    if result.conversion_timed_out:
        # #162: a cut conversion is also a cut preparation, counted as one
        # timed-out "pair"; name the earlier cut, matching the CLI.
        n = max(n - 1, 0)
        _warnings.warn(
            "the global budget ran out during conversion "
            "(RUSTDL_HARD_GLOBAL_DEADLINE=1), so only part of the ontology was "
            "read. Axioms past the cut were neither reasoned over nor counted as "
            "dropped, and consistency was not checked. It is still sound (no "
            "false subsumptions); check result.conversion_timed_out.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
    elif result.prep_timed_out:
        # #162: the cut preparation is counted as one timed-out "pair" so that
        # `complete` reads False; report it as what it is (#228 review).
        n = max(n - 1, 0)
        _warnings.warn(
            "the global budget ran out during preparation, so only a partial "
            "saturation closure is reported and consistency was not fully "
            "checked. It is still sound (no false subsumptions); check "
            "result.prep_timed_out.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
    # #288: probes cut by the tableau's search-depth limit did not exceed a
    # timeout; name them separately. With none, the warning is unchanged.
    d = min(result.depth_capped_pairs, n)
    n -= d
    if d:
        _warnings.warn(
            f"{d} probe(s) hit the tableau's search-depth limit and were left "
            "undecided — this classification may be missing real subsumptions. "
            "It is still sound (no false subsumptions). A larger "
            "per_pair_timeout_ms does not recover these; per_pair_timeout_ms=0, "
            "global_timeout_ms=0 has a far larger limit, but may not terminate. "
            "Check result.complete / result.depth_capped_pairs.",
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
    if result.wedge_clauses_refused:
        # #353: refused clauses never fire, so completeness is not guaranteed even
        # with no timeout.
        _warnings.warn(
            f"the wedge cannot match the body of {result.wedge_clauses_refused} "
            "clause(s), so they never fire and entailments that need them may be "
            "missing. It is still sound (no false subsumptions); check "
            "result.wedge_clauses_refused.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
    if result.derived_same_undecided:
        # #354: equalities the KB may force were not decided, so the hierarchy may
        # be missing what they entail, with no timeout on any class pair.
        _warnings.warn(
            f"{result.derived_same_undecided} candidate individual equalit(ies) were "
            "not decided (budget or probe cap), so entailments that need them may "
            "be missing. It is still sound (no false subsumptions); check "
            "result.derived_same_undecided.",
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
    convention for `classify`.

    Also raised by the explanation calls (`justify`, `justify_all`, `repair`,
    `diagnose`, `debug`, and `PreparedOntology.justify` / `justify_all`) when a
    reasoner check under them gave up (#396): an empty result then means
    "unknown", not "not entailed"; a justification returned still entails but
    may not be minimal; justifications or repairs may be missing. Every repair
    returned is still verified.

    Silence with the standard `warnings` module."""


def _warn_if_query_incomplete(name: str, incomplete: bool) -> None:
    if incomplete:
        _warnings.warn(
            f"{name} result may be incomplete (budget/fragment exhausted) — "
            "sound under-approximation: no false entries, but real ones may be "
            "missing.",
            IncompleteQueryWarning,
            stacklevel=3,
        )


def _warn_if_explanation_incomplete(name: str, incomplete: bool) -> None:
    if incomplete:
        _warnings.warn(
            f"{name}: a reasoner check gave up (node cap, RUSTDL_MAX_NODES), so an "
            "empty result means unknown, not 'not entailed'; justifications "
            "returned still entail but may not be minimal, and justifications or "
            "repairs may be missing.",
            IncompleteQueryWarning,
            stacklevel=3,
        )


def justify(path, query):
    """One minimal justification (Manchester axioms) for `query` (CLI-style
    tokens, e.g. ["unsat", c] / ["subclass", s, t] / ["inconsistent"]). Empty if
    not entailed. Emits `IncompleteQueryWarning` when a reasoner check gave up:
    an empty list then means unknown, and a justification may not be minimal.
    Emits `TrustedConsistencyWarning` (#429) when a check answered "not entailed"
    on the consistency wedge's model alone: an empty list may then be wrong, and
    a justification may not be minimal."""
    axioms, incomplete, trusted = _justify_native(path, query)
    _warn_if_explanation_incomplete("justify", incomplete)
    _warn_if_oracle_trusted("justify", trusted)
    return axioms


def justify_all(path, query, max=10):
    """Up to `max` minimal justifications for `query`. Emits
    `IncompleteQueryWarning` when a reasoner check gave up (see `justify`; some
    justifications may also be missing)."""
    js, incomplete, trusted = _justify_all_native(path, query, max)
    _warn_if_explanation_incomplete("justify_all", incomplete)
    _warn_if_oracle_trusted("justify_all", trusted)
    return js


def diagnose(path):
    """(consistent, root_unsat_iris, [(derived_iri, [root_iri, ...]), ...]).
    Emits `IncompleteQueryWarning` when a reasoner check under it gave up: a
    class's unsatisfiability check (unsatisfiable classes may be missing), or
    the consistency check of an all-classes-unsatisfiable ontology
    (`consistent` is not witnessed). Emits `TrustedConsistencyWarning` (#429)
    when `consistent` rests on the consistency wedge's model alone."""
    consistent, roots, derived, incomplete, trusted = _diagnose_native(path)
    _warn_if_trusted_wedge_consistency("diagnose", trusted)
    if incomplete:
        _warnings.warn(
            "diagnose: a reasoner check gave up (node cap, RUSTDL_MAX_NODES): "
            "unsatisfiable classes may be missing, or, if every class is "
            "unsatisfiable, `consistent` is not witnessed and the ontology may be "
            "inconsistent.",
            IncompleteQueryWarning,
            stacklevel=2,
        )
    return consistent, roots, derived


def repair(path, query, max=10):
    """Minimal axiom-removal sets (Manchester) that break `query`, each verified.
    Emits `IncompleteQueryWarning` when a reasoner check gave up: an empty list
    may then mean unknown, and repairs may be missing. Emits
    `TrustedConsistencyWarning` (#429) when a check answered "not entailed" on
    the consistency wedge's model alone: a repair may then break the query only
    if that model is right."""
    repairs, incomplete, trusted = _repair_native(path, query, max)
    _warn_if_explanation_incomplete("repair", incomplete)
    _warn_if_oracle_trusted("repair", trusted)
    return repairs


def _prepared_justify(self: "PreparedOntology", query):
    """One minimal justification (Manchester axioms) for `query`. Emits
    `IncompleteQueryWarning` when a reasoner check gave up (see
    `rustdl.justify`)."""
    axioms, incomplete, trusted = self._justify_reporting(query)
    _warn_if_explanation_incomplete("PreparedOntology.justify", incomplete)
    _warn_if_oracle_trusted("PreparedOntology.justify", trusted)
    return axioms


def _prepared_justify_all(self: "PreparedOntology", query, max=10):
    """Up to `max` minimal justifications for `query`. Emits
    `IncompleteQueryWarning` when a reasoner check gave up (see
    `rustdl.justify_all`)."""
    js, incomplete, trusted = self._justify_all_reporting(query, max)
    _warn_if_explanation_incomplete("PreparedOntology.justify_all", incomplete)
    _warn_if_oracle_trusted("PreparedOntology.justify_all", trusted)
    return js


PreparedOntology.justify = _prepared_justify  # type: ignore[attr-defined]
PreparedOntology.justify_all = _prepared_justify_all  # type: ignore[attr-defined]


def is_consistent(path):
    """True iff the ontology at `path` is consistent. A give-up (budget or node
    cap, no clash witnessed) returns `True` and emits `IncompleteQueryWarning`.
    A `True` that rests on the consistency wedge's model alone (the wedge is not
    proven complete) emits `TrustedConsistencyWarning` (#392)."""
    consistent, incomplete, trusted = _is_consistent_native(path)
    _warn_if_query_incomplete("is_consistent", incomplete)
    if trusted:
        _warnings.warn(
            "is_consistent: consistent on the consistency wedge's model alone; the "
            "wedge is not proven complete and no main-tableau model was built "
            "(see TrustedConsistencyWarning).",
            TrustedConsistencyWarning,
            stacklevel=2,
        )
    return consistent


class TrustedConsistencyWarning(UserWarning):
    """An answer rests on the KB being consistent, and the only evidence for that
    is the wedge finding a model it does not prove complete. Nothing was cut, so
    this is NOT an `IncompleteQueryWarning`.

    - `is_consistent` (#392): the `True` IS that verdict.
      `RUSTDL_WEDGE_CONSISTENCY=0` decides on the main tableau instead.
    - `is_class_satisfiable`, `is_subclass_of`, `class_expression_satisfiable` and
      `class_expression_entailed_subclass` (#401): the answer was computed without
      the ABox, which is exact when the KB is consistent and has no nominals. If
      that consistency were ever wrong the answer could only be a miss
      ("satisfiable" / "not subsumed"), never a false positive.
      `RUSTDL_QUERY_TBOX_ONLY=0` seeds the ABox instead.
    - `classify`, `classify_bytes`, `realize`, `diagnose` and the
      `materialize_*` functions over them (#429): the classification took the
      ontology as consistent on the consistency wedge's model alone. If that model
      is wrong the ontology is inconsistent, every class is unsatisfiable, and
      realize should have raised.
    - `justify`, `justify_all`, `repair` (#429): an oracle call answered "not
      entailed" on a trusted wedge model. On a "not entailed" result the verdict
      rests on it; on a justification only its minimality can (every returned
      justification still entails)."""


def _warn_if_trusted_consistency(name: str, relies: bool) -> None:
    if relies:
        _warnings.warn(
            f"{name} was answered without the ABox; that relies on the KB being "
            "consistent, which the wedge found but did not prove "
            "(see TrustedConsistencyWarning).",
            TrustedConsistencyWarning,
            stacklevel=3,
        )


def _warn_if_trusted_wedge_consistency(name: str, trusted: bool, stacklevel: int = 3) -> None:
    # #429: the #392 fact reached through a classification (or justify's oracle).
    if trusted:
        _warnings.warn(
            f"{name} took the ontology as consistent on the consistency wedge's "
            "model alone, which is not proven complete; if that model is wrong the "
            "ontology is inconsistent (see TrustedConsistencyWarning).",
            TrustedConsistencyWarning,
            stacklevel=stacklevel,
        )


def _warn_if_oracle_trusted(name: str, trusted: bool) -> None:
    # #429: a justify / repair oracle call said "not entailed" on a trusted wedge model.
    if trusted:
        _warnings.warn(
            f"{name}: a reasoner check answered 'not entailed' on the consistency "
            "wedge's model alone, which is not proven complete. An empty result may "
            "then be wrong; a non-empty one still holds but may not be minimal, and a "
            "repair may break the query only if that model is right "
            "(see TrustedConsistencyWarning).",
            TrustedConsistencyWarning,
            stacklevel=3,
        )


def _warn_if_classification_trusted(result: "Classification") -> "Classification":
    _warn_if_trusted_wedge_consistency(
        "classify", result.relies_on_trusted_consistency, stacklevel=4
    )
    return result


class ConsistencyUndeterminedWarning(UserWarning):
    """Raised when a POSITIVE answer's KB consistency stayed undetermined (#369):
    the consistency wedge stalled or ran out of budget, so the answer may be the
    vacuous one of an inconsistent KB. Emitted by `is_class_satisfiable`
    ("unsatisfiable"), `is_subclass_of` (entailed), `is_instance_of`,
    `instances_of`, `same_individuals`, `different_individuals`,
    `class_expression_instances`, `class_expression_satisfiable` ("unsatisfiable"),
    `class_expression_entailed_subclass` (entailed), `object_property_values`,
    `data_property_values`, `disjoint_object_properties`,
    `disjoint_data_properties` and the `materialize_inferred_*` property functions
    (a non-empty answer). Ontologies with no individual in an ABox assertion
    (class, object property, same, different) and no nominal are not checked
    (the wedge needs a seed) and never warn. A KB found inconsistent raises `RustdlError` instead.
    Nothing was cut, so this is NOT an `IncompleteQueryWarning`.
    `RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL=1` also runs a bounded main-tableau
    check, at a cost."""


def _warn_if_consistency_undetermined(name: str, undetermined: bool) -> None:
    if undetermined:
        _warnings.warn(
            f"{name}: the KB's consistency could not be determined, so this "
            "positive answer may be the vacuous one of an inconsistent KB "
            "(see ConsistencyUndeterminedWarning).",
            ConsistencyUndeterminedWarning,
            stacklevel=3,
        )


def is_class_satisfiable(path, class_iri):
    """True iff `class_iri` is satisfiable (not ⊑ ⊥). If the probe gave up (node
    cap), returns `True` and emits `IncompleteQueryWarning`. Emits
    `TrustedConsistencyWarning` when the answer relies on a trusted consistency
    verdict, and `ConsistencyUndeterminedWarning` (#428) on an "unsatisfiable"
    whose KB consistency stayed undetermined. A KB found inconsistent raises
    `RustdlError`.

    `owl:Thing` / `owl:Nothing` are the built-in ⊤ / ⊥ (#427), declared or not:
    `owl:Nothing` is unsatisfiable, and `owl:Thing` is satisfiable iff the ontology
    is consistent (this IS the consistency check). For `owl:Thing`, a
    `TrustedConsistencyWarning` means what it means on `is_consistent`: the
    verdict rests on the consistency wedge's model alone (#392); no `ABox` was
    dropped, whatever the warning's text says."""
    sat, incomplete, trusted, undetermined = _is_class_satisfiable_native(path, class_iri)
    _warn_if_query_incomplete("is_class_satisfiable", incomplete)
    _warn_if_trusted_consistency("is_class_satisfiable", trusted)
    _warn_if_consistency_undetermined("is_class_satisfiable", undetermined)
    return sat


def is_subclass_of(path, sub, sup):
    """True iff `sub ⊑ sup` is entailed. If the probe gave up (node cap), returns
    `False` and emits `IncompleteQueryWarning`. Emits
    `ConsistencyUndeterminedWarning` (#428) on an entailed answer whose KB
    consistency stayed undetermined. A KB found inconsistent raises `RustdlError`.

    `owl:Thing` / `owl:Nothing` are the built-in ⊤ / ⊥ (#427): `C ⊑ owl:Thing` and
    `owl:Nothing ⊑ C` are always `True`, `C ⊑ owl:Nothing` is `C`'s
    unsatisfiability. An unknown IRI still raises, whatever the other argument."""
    subsumed, incomplete, trusted, undetermined = _is_subclass_of_native(path, sub, sup)
    _warn_if_query_incomplete("is_subclass_of", incomplete)
    _warn_if_trusted_consistency("is_subclass_of", trusted)
    _warn_if_consistency_undetermined("is_subclass_of", undetermined)
    return subsumed


def is_instance_of(path, class_iri, individual_iri):
    """True iff `individual_iri` is an instance of `class_iri`. If the probe was
    cut (deadline, node cap, depth limit), returns `False` and emits
    `IncompleteQueryWarning`. Emits `ConsistencyUndeterminedWarning` (#369).

    `owl:Thing` / `owl:Nothing` are the built-in ⊤ / ⊥ (#427). Like every class,
    they raise on an inconsistent ontology: `a : owl:Thing` holds anyway, but the
    error is how callers learn the ontology is broken, so ⊤ is no exception."""
    is_inst, incomplete, undetermined = _is_instance_of_native(
        path, class_iri, individual_iri
    )
    _warn_if_query_incomplete("is_instance_of", incomplete)
    _warn_if_consistency_undetermined("is_instance_of", undetermined)
    return is_inst


def instances_of(path, class_iri):
    """Named individuals entailed to be instances of `class_iri`. Emits
    `IncompleteQueryWarning` when a probe was cut (members may be missing), and
    `ConsistencyUndeterminedWarning` (#369)."""
    members, incomplete, undetermined = _instances_of_native(path, class_iri)
    _warn_if_query_incomplete("instances_of", incomplete)
    _warn_if_consistency_undetermined("instances_of", undetermined)
    return members


class DirectTypesUnverifiedWarning(UserWarning):
    """Raised by `realize` when some individual has two most-specific types
    whose subsumption could not be decided (#388): a listed type may be a
    superclass of another listed type. Every listed type is still a genuine
    type. Deliberately NOT an `IncompleteQueryWarning`, which means a type may
    be MISSING; this means one may be EXTRA. It is set when the check was cut by
    the per-pair budget or node cap, so more time
    (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`) can clear it; when the check is off
    (`RUSTDL_REALIZE_VERIFY_DIRECT=0`); and by `realize_saturation_only`, which
    runs no check."""


class WitnessPruneWarning(UserWarning):
    """Raised by `realize`, `materialize_inferred_class_assertions` and
    `materialize_existential_successors` when the pseudo-model prune answered
    "not a type" from a witness model with a known reason to lack an entailed
    type (#387): the witness search chose a disjunct or a merge, or the engine
    that built it set axioms aside. An entailed type may then be MISSING,
    although no probe was cut (so this is not an `IncompleteQueryWarning`).
    Its absence is not a guarantee: it means no known reason for loss, not a
    proof that none was dropped. `RUSTDL_PSEUDO_MODEL=0` probes every pair
    instead, which can be much slower."""


def _warn_if_witness_prune_may_drop(name: str, may_drop: bool) -> None:
    if may_drop:
        _warnings.warn(
            f"{name}: the pseudo-model prune answered 'not a type' from a "
            "witness model built by a choice (or with axioms set aside), so an "
            "entailed type may be missing; RUSTDL_PSEUDO_MODEL=0 probes every "
            "pair instead (slower).",
            WitnessPruneWarning,
            stacklevel=3,
        )


def realize(path):
    """Map each named individual to its most-specific entailed types. Emits
    `IncompleteQueryWarning` when an instance probe was cut (types may be
    missing), and `DirectTypesUnverifiedWarning` when two most-specific types
    of an individual could not be compared (a listed type may not be the most
    specific), and `WitnessPruneWarning` when the pseudo-model prune may have
    dropped an entailed type (#387). Emits `TrustedConsistencyWarning` when the
    classification underneath took the ontology as consistent on the
    consistency wedge's model alone (#429)."""
    types, incomplete, unverified, may_drop, trusted = _realize_native(path)
    _warn_if_query_incomplete("realize", incomplete)
    _warn_if_trusted_wedge_consistency("realize", trusted)
    _warn_if_direct_types_unverified("realize", unverified)
    _warn_if_witness_prune_may_drop("realize", may_drop)
    return types


def _warn_if_direct_types_unverified(name: str, unverified: bool) -> None:
    if unverified:
        _warnings.warn(
            f"{name}: some individual has two most-specific types whose "
            "subsumption could not be decided, so a listed type may be a "
            "superclass of another; every listed type is a genuine type.",
            DirectTypesUnverifiedWarning,
            stacklevel=3,
        )


def materialize_inferred_subclass_axioms(path):
    """Every entailed `(sub, sup)` pair of named classes, read off `classify`
    (reflexive pairs, `owl:Thing`/`owl:Nothing` and unsatisfiable subclasses
    left out). Emits the same `IncompleteClassificationWarning`s as `classify`
    when the classification may be missing subsumptions (#391). It runs an
    unbounded `classify` and takes no budget arguments, so a warning that
    suggests passing `per_pair_timeout_ms` / `global_timeout_ms` or checking
    `result` refers to `rustdl.classify`, which exposes both."""
    pairs, classification = _materialize_inferred_subclass_axioms_native(path)
    _warn_if_incomplete(classification)
    _warn_if_trusted_wedge_consistency(
        "materialize_inferred_subclass_axioms",
        classification.relies_on_trusted_consistency,
    )
    return pairs


def materialize_inferred_class_assertions(path):
    """A `(class, individual)` pair for each MOST-SPECIFIC entailed type of each
    named individual, as `realize` reports them (not every entailed type).
    Emits `IncompleteQueryWarning` when an instance probe was cut (types may be
    missing) and `DirectTypesUnverifiedWarning` when a listed type may be a
    superclass of another (#388, #391), and `WitnessPruneWarning` when the
    pseudo-model prune may have dropped a type (#387)."""
    pairs, incomplete, unverified, may_drop, trusted = (
        _materialize_inferred_class_assertions_native(path)
    )
    _warn_if_query_incomplete("materialize_inferred_class_assertions", incomplete)
    _warn_if_trusted_wedge_consistency("materialize_inferred_class_assertions", trusted)
    _warn_if_direct_types_unverified("materialize_inferred_class_assertions", unverified)
    _warn_if_witness_prune_may_drop("materialize_inferred_class_assertions", may_drop)
    return pairs


def materialize_existential_successors(path):
    """Entailed existential successors as `(subject, property, witness_blank,
    filler_class)`: a blank-node representation of entailed `a : ∃R.C`, not
    ground triples. Emits `IncompleteQueryWarning` when an instance probe of the
    `realize` run underneath was cut, so rows may be missing (#391), and
    `WitnessPruneWarning` when its pseudo-model prune may have dropped a type
    (#387)."""
    rows, incomplete, may_drop, trusted = _materialize_existential_successors_native(path)
    _warn_if_query_incomplete("materialize_existential_successors", incomplete)
    _warn_if_trusted_wedge_consistency("materialize_existential_successors", trusted)
    _warn_if_witness_prune_may_drop("materialize_existential_successors", may_drop)
    return rows


class StructuralGapWarning(UserWarning):
    """Raised by the STRUCTURAL property queries (`object_property_hierarchy`,
    `data_property_hierarchy`, `disjoint_object_properties`,
    `disjoint_data_properties`, `data_property_values` and the matching
    `materialize_inferred_*` functions) when the ontology contains constructs
    those closures do not reason with and that can entail a relation they would
    report (issues #389, #390). The message names them. Nothing was cut by a
    budget, so this is deliberately NOT an `IncompleteQueryWarning`: re-running
    with more time changes nothing. No warning does not prove completeness."""


def _warn_if_structural_gaps(name: str, gaps: "list[str]") -> None:
    if gaps:
        _warnings.warn(
            f"{name} is structural and this ontology has constructs it does not "
            f"reason with, so entailed entries may be missing: {', '.join(gaps)}.",
            StructuralGapWarning,
            stacklevel=3,
        )


def _structural(name, native):
    def wrapper(path):
        result, gaps, undetermined = native(path)
        _warn_if_structural_gaps(name, gaps)
        _warn_if_consistency_undetermined(name, undetermined)
        return result

    wrapper.__name__ = name
    wrapper.__qualname__ = name
    wrapper.__doc__ = (native.__doc__ or "") + (
        "\n\nEmits `StructuralGapWarning` when the ontology has constructs this "
        "structural closure ignores (see `StructuralGapWarning`), and "
        "`ConsistencyUndeterminedWarning` (#369)."
    )
    return wrapper


def materialize_inferred_property_assertions(path):
    """Every inferred object property assertion `(subject, property, object)`
    entailed over named individuals (asserted + derived via property hierarchy /
    inverse / symmetric / role chains / transitivity). Sound under-approximation
    (no anonymous-witness or disjunctive-derived edges). Raises if the ontology
    is inconsistent. Emits `ConsistencyUndeterminedWarning` (#369)."""
    rows, undetermined = _materialize_inferred_property_assertions_native(path)
    _warn_if_consistency_undetermined(
        "materialize_inferred_property_assertions", undetermined
    )
    return rows


disjoint_object_properties = _structural(
    "disjoint_object_properties", _disjoint_object_properties_native
)
disjoint_data_properties = _structural("disjoint_data_properties", _disjoint_data_properties_native)
data_property_values = _structural("data_property_values", _data_property_values_native)
materialize_inferred_data_property_assertions = _structural(
    "materialize_inferred_data_property_assertions",
    _materialize_inferred_data_property_assertions_native,
)
materialize_inferred_subobjectproperty_axioms = _structural(
    "materialize_inferred_subobjectproperty_axioms",
    _materialize_inferred_subobjectproperty_axioms_native,
)
materialize_inferred_subdataproperty_axioms = _structural(
    "materialize_inferred_subdataproperty_axioms",
    _materialize_inferred_subdataproperty_axioms_native,
)


def object_property_hierarchy(path):
    """Inferred object property hierarchy: `(equivalent_groups,
    direct_subsumptions)`. Structural; emits `StructuralGapWarning` when the
    ontology has constructs it ignores."""
    groups, direct, gaps = _object_property_hierarchy_native(path)
    _warn_if_structural_gaps("object_property_hierarchy", gaps)
    return groups, direct


def data_property_hierarchy(path):
    """Inferred data property hierarchy: `(equivalent_groups,
    direct_subsumptions)`. Structural; emits `StructuralGapWarning` when the
    ontology has constructs it ignores."""
    groups, direct, gaps = _data_property_hierarchy_native(path)
    _warn_if_structural_gaps("data_property_hierarchy", gaps)
    return groups, direct

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
    groups, incomplete, undetermined = _same_individuals_native(path)
    _warn_if_query_incomplete("same_individuals", incomplete)
    _warn_if_consistency_undetermined("same_individuals", undetermined)
    return groups


def different_individuals(path):
    """Entailed different-individual pairs `(a, b)` — `{a} ⊓ {b}` is proven
    unsatisfiable. Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`)."""
    pairs, incomplete, undetermined = _different_individuals_native(path)
    _warn_if_query_incomplete("different_individuals", incomplete)
    _warn_if_consistency_undetermined("different_individuals", undetermined)
    return pairs


def object_property_values(path):
    """Inferred object property values `(subject, property, object)` over
    named individuals. Bounded by a 1s per-pair deadline; emits
    `IncompleteQueryWarning` when the budget was exhausted (see
    `IncompleteQueryWarning`), and `ConsistencyUndeterminedWarning` (#369)."""
    triples, incomplete, undetermined = _object_property_values_native(path)
    _warn_if_query_incomplete("object_property_values", incomplete)
    _warn_if_consistency_undetermined("object_property_values", undetermined)
    return triples


def class_expression_satisfiable(path, ce):
    """True iff the Manchester-syntax class expression `ce` is satisfiable
    w.r.t. the ontology at `path` (`ce` is resolved against the ontology's
    own prefix map, e.g. `:A`). If the probe gave up (node cap), returns `True`
    and emits `IncompleteQueryWarning`, as `is_class_satisfiable` does; an
    answer off the pure-EL fragment no longer warns. Emits
    `ConsistencyUndeterminedWarning` (#369)."""
    holds, incomplete, trusted, undetermined = _class_expression_satisfiable_native(
        path, ce
    )
    _warn_if_query_incomplete("class_expression_satisfiable", incomplete)
    _warn_if_trusted_consistency("class_expression_satisfiable", trusted)
    _warn_if_consistency_undetermined("class_expression_satisfiable", undetermined)
    return holds


def class_expression_entailed_subclass(path, sub_ce, sup_ce):
    """True iff `sub_ce ⊑ sup_ce` is entailed w.r.t. the ontology at `path`
    (both Manchester-syntax class expressions, resolved against the
    ontology's own prefix map). If the probe gave up (node cap), returns `False`
    and emits `IncompleteQueryWarning`, as `is_subclass_of` does; an answer off
    the pure-EL fragment no longer warns. Emits
    `ConsistencyUndeterminedWarning` (#369)."""
    holds, incomplete, trusted, undetermined = (
        _class_expression_entailed_subclass_native(path, sub_ce, sup_ce)
    )
    _warn_if_query_incomplete("class_expression_entailed_subclass", incomplete)
    _warn_if_trusted_consistency("class_expression_entailed_subclass", trusted)
    _warn_if_consistency_undetermined(
        "class_expression_entailed_subclass", undetermined
    )
    return holds


def class_expression_instances(path, ce):
    """Named individuals provably in the Manchester-syntax class expression
    `ce` w.r.t. the ontology at `path` (resolved against the ontology's own
    prefix map). Emits `IncompleteQueryWarning` when an instance probe was cut
    (members may be missing; those returned are sound), as `instances_of`
    does; an answer off the pure-EL fragment no longer warns.
    Emits `ConsistencyUndeterminedWarning` (#369)."""
    individuals, incomplete, undetermined = _class_expression_instances_native(path, ce)
    _warn_if_query_incomplete("class_expression_instances", incomplete)
    _warn_if_consistency_undetermined("class_expression_instances", undetermined)
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
    return _warn_if_classification_trusted(_warn_if_incomplete(
        _classify_native(
            path,
            per_pair_timeout_ms=per_pair_timeout_ms,
            global_deadline_ms=global_timeout_ms,
            saturation_only=saturation_only,
        )
    ))


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
    return _warn_if_classification_trusted(_warn_if_incomplete(
        _classify_bytes_native(
            data,
            format=format,
            per_pair_timeout_ms=per_pair_timeout_ms,
            global_deadline_ms=global_timeout_ms,
            saturation_only=saturation_only,
        )
    ))


def debug(path):
    """One-call ontology diagnosis → a Diagnosis result object.

    Supports attribute access (d.consistent, d.roots[0].justification,
    d.inconsistency.repairs) AND legacy dict access
    (d["roots"][0]["justification"], dict(d), iteration). For JSON use
    json.dumps(d.to_dict()).

    Consistent ontology → Diagnosis(consistent=True, unsatisfiable=..., roots=[Root...],
    derived=[Derived...], inconsistency=None). Inconsistent → consistent=False, empties,
    inconsistency=Inconsistency(...). Read-only; sound by construction. Emits
    `IncompleteQueryWarning` when a reasoner check under it gave up (#396), through
    `diagnose`, `justify` and `repair`."""
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
    "DirectTypesUnverifiedWarning",
    "WitnessPruneWarning",
    "StructuralGapWarning",
    "TrustedConsistencyWarning",
    "ConsistencyUndeterminedWarning",
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

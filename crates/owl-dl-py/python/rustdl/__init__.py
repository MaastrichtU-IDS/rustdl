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
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
    materialize_inferred_subclass_axioms as materialize_inferred_subclass_axioms,
    materialize_inferred_class_assertions as materialize_inferred_class_assertions,
)


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
    """Raised when classification hit the per-pair timeout, so the
    returned hierarchy is a sound under-approximation (no false
    subsumptions, but real ones may be missing). Silence with the
    standard `warnings` module, or pass `per_pair_timeout_ms=0` to
    classify for the complete (unbounded) result."""


def _warn_if_incomplete(result: "Classification") -> "Classification":
    n = result.timed_out_pairs
    if n:
        _warnings.warn(
            f"{n} class pair(s) exceeded the per-pair timeout and were recorded as "
            "'not subsumed' — this classification may be missing real subsumptions. "
            "It is still sound (no false subsumptions). Pass per_pair_timeout_ms=0 "
            "for the complete (unbounded) result, or check result.complete / "
            "result.timed_out_pairs.",
            IncompleteClassificationWarning,
            stacklevel=3,
        )
    return result


def classify(path, *, per_pair_timeout_ms=1000, saturation_only=False):
    """Classify the ontology at `path` (format auto-detected from the
    extension: .ofn / .owx / .owl / .rdf).

    `per_pair_timeout_ms` bounds each subsumption test (default 1000;
    `0` = unbounded/complete). Pairs that exceed the budget are recorded
    as "not subsumed" — sound, but the result may be incomplete; an
    `IncompleteClassificationWarning` is emitted when that happens, and
    `result.complete` / `result.timed_out_pairs` report it.
    `saturation_only=True` skips the tableau (EL-closure under-
    approximation; fast)."""
    return _warn_if_incomplete(
        _classify_native(
            path,
            per_pair_timeout_ms=per_pair_timeout_ms,
            saturation_only=saturation_only,
        )
    )


def classify_bytes(data, *, format, per_pair_timeout_ms=1000, saturation_only=False):
    """Like `classify`, but from in-memory `data` with an explicit
    `format` ("ofn" | "owx" | "rdf-xml"). See `classify` for the
    timeout/completeness semantics."""
    return _warn_if_incomplete(
        _classify_bytes_native(
            data,
            format=format,
            per_pair_timeout_ms=per_pair_timeout_ms,
            saturation_only=saturation_only,
        )
    )


__all__ = [
    "__version__",
    "Classification",
    "IncompleteClassificationWarning",
    "classify",
    "classify_bytes",
    "is_consistent",
    "is_class_satisfiable",
    "is_subclass_of",
    "is_instance_of",
    "instances_of",
    "realize",
    "RustdlError",
    "ParseError",
    "UnsupportedAxiomError",
    "UnknownClassError",
    "materialize_inferred_subclass_axioms",
    "materialize_inferred_class_assertions",
]

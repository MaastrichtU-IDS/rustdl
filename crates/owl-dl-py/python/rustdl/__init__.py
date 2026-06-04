"""
rustdl — sound, performant OWL 2 DL (SROIQ) reasoner.

Python bindings for the rustdl Rust crate. Install via
`pip install rustdl`; import as `import rustdl`. See
https://github.com/MaastrichtU-IDS/rustdl for the full project.
"""

# Native extension built by PyO3 + maturin
from rustdl._native import (
    __version__ as __version__,
    Classification as Classification,
    classify as classify,
    classify_bytes as classify_bytes,
    is_consistent as is_consistent,
    is_class_satisfiable as is_class_satisfiable,
    is_subclass_of as is_subclass_of,
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
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


__all__ = [
    "__version__",
    "Classification",
    "classify",
    "classify_bytes",
    "is_consistent",
    "is_class_satisfiable",
    "is_subclass_of",
    "RustdlError",
    "ParseError",
    "UnsupportedAxiomError",
    "UnknownClassError",
]

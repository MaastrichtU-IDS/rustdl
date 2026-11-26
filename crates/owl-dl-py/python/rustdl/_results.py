"""Structured, dict-compatible result objects for rustdl.debug().

Frozen dataclasses that also implement collections.abc.Mapping, so both
attribute access (d.roots[0].justification) and the legacy dict access
(d["roots"][0]["justification"], dict(d), **d, iteration) work. For JSON use
`json.dumps(d.to_dict())` — a Mapping is not a dict to the json module."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass, fields
from typing import Optional


def _plain(value: object) -> object:
    """Recursively convert result objects/tuples to plain dict/list for JSON."""
    if isinstance(value, _MappingDataclass):
        return value.to_dict()
    if isinstance(value, tuple):
        return [_plain(v) for v in value]
    return value


class _MappingDataclass(collections.abc.Mapping):
    """Mixin: expose a frozen dataclass as a read-only Mapping over a key list.

    Subclasses override `_keys()` to return the keys present (matching the legacy
    dict — e.g. Diagnosis omits "inconsistency" when consistent)."""

    def _keys(self) -> tuple[str, ...]:
        return tuple(f.name for f in fields(self))  # type: ignore[arg-type]

    def __getitem__(self, key: str) -> object:
        if key in self._keys():
            return getattr(self, key)
        raise KeyError(key)

    def __iter__(self):
        return iter(self._keys())

    def __len__(self) -> int:
        return len(self._keys())

    def to_dict(self) -> dict:
        return {k: _plain(getattr(self, k)) for k in self._keys()}


@dataclass(frozen=True)
class Root(_MappingDataclass):
    iri: str
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    derives: tuple[str, ...]


@dataclass(frozen=True)
class Derived(_MappingDataclass):
    iri: str
    roots: tuple[str, ...]


@dataclass(frozen=True)
class Inconsistency(_MappingDataclass):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]


@dataclass(frozen=True)
class Diagnosis(_MappingDataclass):
    consistent: bool
    unsatisfiable: tuple[str, ...]
    roots: tuple[Root, ...]
    derived: tuple[Derived, ...]
    inconsistency: Optional[Inconsistency] = None

    def _keys(self) -> tuple[str, ...]:
        base = ("consistent", "unsatisfiable", "roots", "derived")
        return base if self.consistent else base + ("inconsistency",)


# #552: the expression queries' structured results. Field names match the Rust
# accessors (`CeVerdict`, `CeInstances`); the CLI's `--json` names the answer
# `satisfiable` / `entailed` / `instances` and has no `dropped`. `dropped` is a tuple
# of `(kind, count)` pairs sorted by kind, so a result is immutable and hashable;
# `dict(r.dropped)` gives the mapping.


@dataclass(frozen=True)
class ClassExpressionVerdict(_MappingDataclass):
    """A class-expression satisfiability or subsumption verdict and its signals."""

    holds: bool
    incomplete: bool
    completeness_guaranteed: bool
    relies_on_trusted_consistency: bool
    consistency_undetermined: bool
    no_verdict: bool
    dropped: tuple[tuple[str, int], ...]


@dataclass(frozen=True)
class ClassExpressionInstances(_MappingDataclass):
    """A class-expression instance query's members and its signals."""

    individuals: tuple[str, ...]
    incomplete: bool
    completeness_guaranteed: bool
    consistency_undetermined: bool
    no_verdict: bool
    depth_capped: bool
    dropped: tuple[tuple[str, int], ...]

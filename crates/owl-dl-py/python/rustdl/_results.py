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
    # #625: `justification` is guaranteed minimal (`Justifications.minimal`).
    minimal: bool = True


@dataclass(frozen=True)
class Derived(_MappingDataclass):
    iri: str
    roots: tuple[str, ...]


@dataclass(frozen=True)
class Inconsistency(_MappingDataclass):
    justification: tuple[str, ...]
    repairs: tuple[tuple[str, ...], ...]
    # #625: `justification` is guaranteed minimal (`Justifications.minimal`).
    minimal: bool = True


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
    # #543: a universal role (in the ontology or the query) is read as an ordinary
    # property, so a negative answer may be wrong; voids `completeness_guaranteed`.
    universal_role_ordinary: bool = False


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
    # #543: a universal role (in the ontology or the query) is read as an ordinary
    # property, so a negative answer may be wrong; voids `completeness_guaranteed`.
    universal_role_ordinary: bool = False


@dataclass(frozen=True)
class Justifications(_MappingDataclass):
    """#625: a justification query's answer and its signals (`justify_result`,
    `justify_all_result`, and their `PreparedOntology` forms).

    `justifications` holds each justification's axioms (Manchester strings); every
    one entails the query. `minimal[i]` is `Justification::minimal_guaranteed`: the
    ontology is pure EL or Horn (`in_fragment`), no subset probe gave up
    (`incomplete`), and the search did not run on the saturation-only consistency
    oracle (`saturation_oracle`, #584). `False` means minimality is not guaranteed,
    not that the set is not minimal. `minimal` does not cover
    `relies_on_trusted_consistency` (#429): a check that trusted the consistency
    wedge's model can only have kept an axiom, so a `True` there also rests on that
    model. `enumeration_complete`: no justification is known to be missing. For a
    single-justification query, `False` only when none was found and the search gave
    up. For `justify_all_result`: nothing gave up, the saturation-only oracle did not
    run, every justification is minimal, and fewer than `max` were found (exactly `max`
    may have been truncated, so it reads `False`; ask for more to tell)."""

    justifications: tuple[tuple[str, ...], ...]
    minimal: tuple[bool, ...]
    enumeration_complete: bool
    incomplete: bool
    relies_on_trusted_consistency: bool
    depth_capped: bool
    saturation_oracle: bool
    in_fragment: bool

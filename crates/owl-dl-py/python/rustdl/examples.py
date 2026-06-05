"""Bundled example ontologies for trying rustdl with no network access.

Three real OWL ontologies ship inside the wheel — gzip-compressed, ~200 KB
total — so they classify offline:

- `pizza()`  — the classic OWL 2 DL teaching ontology (small, SROIQ; a few
  hard satisfiability pairs make a good completeness demo).
- `sulo()`   — the Simple Upper-Level Ontology (tiny, classifies instantly).
- `sio()`    — the Semanticscience Integrated Ontology (~1600 classes;
  a realistic, larger workload — classification takes tens of seconds).

Each `*()` returns a filesystem path to pass straight to `rustdl.classify`.
On first use it decompresses the bundled `.owl.gz` into a per-user cache
directory (``$XDG_CACHE_HOME/rustdl/examples`` or ``~/.cache/rustdl/examples``)
and reuses it thereafter — no network, ever. Each `*_NS` constant is the
namespace, so a class IRI is the namespace plus the local name.
"""

import gzip
import hashlib
import os
from importlib import resources
from pathlib import Path

# Class IRIs are the namespace + local name, e.g. PIZZA_NS + "Margherita".
PIZZA_NS = (
    "https://raw.githubusercontent.com/owlcs/pizza-ontology/"
    "refs/heads/master/pizza.owl#"
)
# SULO classes are SULO_NS + local name, e.g. SULO_NS + "Object".
SULO_NS = "https://w3id.org/sulo/"
# SIO classes are SIO_NS + numeric local name, e.g. SIO_NS + "SIO_000006"
# ("process"). Note: the resource namespace is plain http, and is distinct
# from the document URL (semanticscience.org/ontology/sio.owl).
SIO_NS = "http://semanticscience.org/resource/"


def _cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(
        os.path.expanduser("~"), ".cache"
    )
    d = Path(base) / "rustdl" / "examples"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _materialize(name: str) -> str:
    """Decompress the bundled ``<name>.owl.gz`` into the user cache dir once
    and return the path to the decompressed ``.owl``.

    Idempotent and offline. The cache filename embeds a hash of the
    compressed bytes, so a wheel upgrade that changes the ontology yields a
    fresh cache file rather than reusing a stale one.
    """
    gz_bytes = (
        resources.files("rustdl").joinpath("data", f"{name}.owl.gz").read_bytes()
    )
    digest = hashlib.sha256(gz_bytes).hexdigest()[:12]
    out = _cache_dir() / f"{name}.{digest}.owl"
    if not out.exists():
        data = gzip.decompress(gz_bytes)
        # Atomic publish: write a pid-unique temp file, then rename into place
        # so concurrent callers never observe a partial file.
        tmp = out.with_name(f"{out.name}.tmp{os.getpid()}")
        tmp.write_bytes(data)
        os.replace(tmp, out)
    return str(out)


def pizza() -> str:
    """Path to the bundled pizza ontology (decompressed on first use).

        >>> import rustdl
        >>> result = rustdl.classify(rustdl.examples.pizza())
        >>> result.is_subclass(
        ...     rustdl.examples.PIZZA_NS + "Margherita",
        ...     rustdl.examples.PIZZA_NS + "Pizza",
        ... )
        True
    """
    return _materialize("pizza")


def sulo() -> str:
    """Path to the bundled SULO ontology (decompressed on first use).

    A tiny upper ontology that classifies in milliseconds:

        >>> import rustdl
        >>> result = rustdl.classify(rustdl.examples.sulo())
        >>> ns = rustdl.examples.SULO_NS
        >>> result.is_subclass(ns + "StartTime", ns + "Object")  # transitive
        True
    """
    return _materialize("sulo")


def sio() -> str:
    """Path to the bundled SIO ontology (decompressed on first use).

    A realistic ~1600-class ontology; classification takes tens of seconds.
    Class IRIs use the numeric SIO codes, e.g. ``SIO_NS + "SIO_000006"`` is
    "process":

        >>> import os, rustdl
        >>> os.path.exists(rustdl.examples.sio())
        True
    """
    return _materialize("sio")


__all__ = ["PIZZA_NS", "SULO_NS", "SIO_NS", "pizza", "sulo", "sio"]

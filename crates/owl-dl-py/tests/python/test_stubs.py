"""The hand-written __init__.pyi must stay in sync with the runtime __all__."""
import ast
import pathlib

import rustdl


def _stub_declared_names() -> set[str]:
    stub = pathlib.Path(rustdl.__file__).with_name("__init__.pyi")
    tree = ast.parse(stub.read_text())
    names: set[str] = set()
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            names.add(node.name)
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            names.add(node.target.id)
        elif isinstance(node, ast.ImportFrom) and (node.level or 0) > 0:
            # Only relative imports re-export public API (e.g.
            # `from . import examples`). Absolute stdlib/typing imports
            # (`from collections.abc import Mapping`, `from typing import
            # Optional`) are annotation helpers, not part of `rustdl`'s surface.
            for a in node.names:
                names.add(a.asname or a.name)
    return names


def test_pyi_exists_and_py_typed():
    pkg = pathlib.Path(rustdl.__file__).parent
    assert (pkg / "__init__.pyi").is_file()
    assert (pkg / "py.typed").is_file()


def test_every_public_name_is_stubbed():
    declared = _stub_declared_names()
    missing = set(rustdl.__all__) - declared
    assert not missing, f"__all__ names missing from __init__.pyi: {sorted(missing)}"


def test_stub_names_are_real():
    for name in _stub_declared_names() - {"examples"}:
        assert hasattr(rustdl, name), f"stub declares {name!r} but rustdl has no such attr"

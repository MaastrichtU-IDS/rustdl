"""The hand-written __init__.pyi must stay in sync with the runtime __all__."""
import ast
import pathlib
import re

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


def _stub_return_arity(func_name: str) -> int | None:
    """Declared tuple arity of `func_name`'s `list[tuple[...]]` return, if any.

    The name-level check above cannot see SIGNATURE drift, which is a real gap
    and not a theoretical one: issue #72 widened `data_property_values` from a
    4-tuple to a 5-tuple, the stub kept saying 4, and CI stayed green — stubs
    are not checked at runtime, and the query test indexed only `q[0]`/`q[1]`.
    """
    stub = pathlib.Path(rustdl.__file__).with_name("__init__.pyi")
    for node in ast.parse(stub.read_text()).body:
        if isinstance(node, ast.FunctionDef) and node.name == func_name:
            ann = ast.unparse(node.returns) if node.returns else ""
            m = re.fullmatch(r"list\[tuple\[(.*)\]\]", ann)
            if m:
                return len(m.group(1).split(","))
    return None


def test_tuple_returning_stubs_match_runtime_arity(tmp_path):
    """The declared tuple width must equal what the function actually returns."""
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(ObjectProperty(:r))\n"
        "  Declaration(DataProperty(:dp))\n"
        "  Declaration(NamedIndividual(:a))\n"
        "  Declaration(NamedIndividual(:b))\n"
        "  ObjectPropertyAssertion(:r :a :b)\n"
        '  DataPropertyAssertion(:dp :a "bonjour"@fr))\n'
    )
    for name in ("object_property_values", "data_property_values"):
        declared = _stub_return_arity(name)
        assert declared is not None, f"{name}: stub declares no list[tuple[...]]"
        rows = getattr(rustdl, name)(str(p))
        assert rows, f"{name}: fixture produced no rows, arity unchecked"
        for row in rows:
            assert len(row) == declared, (
                f"{name}: stub declares a {declared}-tuple, runtime returned "
                f"{len(row)}: {row}"
            )

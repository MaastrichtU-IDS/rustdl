def test_smoke():
    import rustdl
    assert rustdl.__version__


def test_exception_hierarchy():
    import rustdl
    assert issubclass(rustdl.ParseError, rustdl.RustdlError)
    assert issubclass(rustdl.UnsupportedAxiomError, rustdl.RustdlError)
    assert issubclass(rustdl.UnknownClassError, rustdl.RustdlError)
    assert issubclass(rustdl.RustdlError, Exception)

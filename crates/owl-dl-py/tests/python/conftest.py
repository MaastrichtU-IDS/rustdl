"""Shared pytest fixtures for the rustdl Python bindings tests."""
import pathlib
import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[4]
FIXTURE_DIR = REPO_ROOT / "crates" / "owl-dl-reasoner" / "tests" / "fixtures"

@pytest.fixture
def fixtures_dir():
    """Resolve to crates/owl-dl-reasoner/tests/fixtures/ — reuses the Rust-side test inputs."""
    return FIXTURE_DIR

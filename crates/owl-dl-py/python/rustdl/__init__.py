"""
rustdl — sound, performant OWL 2 DL (SROIQ) reasoner.

Python bindings for the rustdl Rust crate. Install via
`pip install rustdl`; import as `import rustdl`. See
https://github.com/MaastrichtU-IDS/rustdl for the full project.
"""

# Re-export the native extension's public surface.
from rustdl._native import (
    __version__ as __version__,
)

__all__ = ["__version__"]

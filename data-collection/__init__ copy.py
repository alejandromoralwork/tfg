"""Compatibility package to allow importing from the `data-collection` directory.

This package adds the actual `data-collection` folder to its `__path__` so
that imports like `import data_collection.client.web3_client` will resolve to
files inside the existing `data-collection` directory (which contains a
hyphen and can't be used directly as a package name).
"""
from pathlib import Path

# Path to the sibling `data-collection` directory
_real_dir = Path(__file__).parent.parent / "data-collection"

if _real_dir.exists():
    __path__.insert(0, str(_real_dir))

# Also include client/py_ws_client so subpackages like `types` are resolvable
_alt_dir = Path(__file__).parent.parent / "client" / "py_ws_client"
if _alt_dir.exists():
    __path__.insert(0, str(_alt_dir))

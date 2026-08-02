"""Compatibility package to expose `data-collection/utils` as `data-collection.utilities`.

This allows existing imports that reference `..utilities` to resolve to the
`utils` folder that already exists in the project.
"""
from pathlib import Path

_real_dir = Path(__file__).parent.parent / "utils"

if _real_dir.exists():
    __path__.insert(0, str(_real_dir))
    
# Also include utilities provided under client/py_ws_client/utilities (some
# modules live there in this repository layout).
_alt_dir = Path(__file__).parent.parent / "client" / "py_ws_client" / "utilities"
if _alt_dir.exists():
    __path__.insert(0, str(_alt_dir))

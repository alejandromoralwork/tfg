"""
Blockchain & Contract Interactions
=================================
Functions for interacting with Polygon blockchain and smart contracts.
"""

try:
    from .set_allowances import main as set_allowances
except ImportError:
    pass

__all__ = [
    'set_allowances'
]

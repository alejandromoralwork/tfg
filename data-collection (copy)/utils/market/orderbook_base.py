"""
Base OrderBook functionality - Core class and imports

This module provides the base OrderBookAnalyzer class with shared imports
and initialization logic for order book analysis functions.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, Any, List, Optional, TypedDict
from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON
from client.ClobClientWrapper import PolymarketAuth


# Type definitions
class OrderBookSummary(TypedDict):
    """Type definition for order book summary"""
    bids: List[Dict[str, Any]]
    asks: List[Dict[str, Any]]


class OrderBookAnalyzer:
    """
    Analyzer for Polymarket order book data
    
    Provides comprehensive order book analysis including:
    - Raw order book retrieval
    - Price analysis (bid/ask/spread)
    - Depth analysis
    - Market comparison tools
    """
    
    def __init__(self, use_auth: bool = False):
        """
        Initialize the OrderBook analyzer
        
        Args:
            use_auth: Whether to use authenticated client
        """
        self.use_auth = use_auth
        
        if use_auth:
            # Use authenticated wrapper
            self.client_wrapper = PolymarketAuth()
            self.client = self.client_wrapper.get_client()
        else:
            # Use public client
            self.client = ClobClient(
                host="https://clob.polymarket.com",
                key="",  # Empty key for public endpoints
                chain_id=POLYGON
            )
        
        print(f"📊 OrderBookAnalyzer initialized (auth: {use_auth})")
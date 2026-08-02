"""
Base Activity functionality - Core enums and imports

This module provides shared enums and imports for user activity functions.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

import requests
from typing import Dict, List, Any, Optional, Union
from enum import Enum


class ActivityType(Enum):
    """Available activity types for filtering"""
    TRADE = "TRADE"
    SPLIT = "SPLIT"
    MERGE = "MERGE"
    REDEEM = "REDEEM"
    REWARD = "REWARD"
    CONVERSION = "CONVERSION"


class TradeSide(Enum):
    """Trade side options"""
    BUY = "BUY"
    SELL = "SELL"


class SortBy(Enum):
    """Sort criteria options"""
    TIMESTAMP = "TIMESTAMP"
    TOKENS = "TOKENS"
    CASH = "CASH"


class SortDirection(Enum):
    """Sort direction options"""
    ASC = "ASC"
    DESC = "DESC"


# Constants
DATA_API_BASE_URL = "https://data-api.polymarket.com"
#!/usr/bin/env python3
"""
Polymarket Positions API - All Functions
========================================

This module provides easy access to all position-related functions.
Import this file to get access to all position functions.

Available Functions:
- get_current_positions: Main function with all parameters
- get_user_profitable_positions: Most profitable positions
- get_user_largest_positions: Largest positions by value
- get_user_redeemable_positions: Redeemable positions
- get_user_positions_by_events: Positions for specific events
- get_user_significant_positions: Positions above size threshold

Usage:
    from positions_all import *
    
    # Get all positions
    positions = get_current_positions("0xF937dBe9976Ac34157b30DD55BDbf248458F6b43")
    
    # Get profitable positions
    profitable = get_user_profitable_positions("0xF937dBe9976Ac34157b30DD55BDbf248458F6b43")
"""

# Import all position functions
from positions_base import get_current_positions, SortBy, SortDirection
from get_user_profitable_positions import get_user_profitable_positions
from get_user_largest_positions import get_user_largest_positions
from get_user_redeemable_positions import get_user_redeemable_positions
from get_user_positions_by_events import get_user_positions_by_events
from get_user_significant_positions import get_user_significant_positions

# Export all functions for easy import
__all__ = [
    'get_current_positions',
    'get_user_profitable_positions', 
    'get_user_largest_positions',
    'get_user_redeemable_positions',
    'get_user_positions_by_events',
    'get_user_significant_positions',
    'SortBy',
    'SortDirection'
]

# Example usage demonstration
if __name__ == "__main__":
    print("Polymarket Positions API - All Functions")
    print("=" * 50)
    
    # Example user address
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    
    print(f"Available functions for user: {test_user}")
    print()
    
    functions_info = [
        ("get_current_positions", "Main function with all filtering options"),
        ("get_user_profitable_positions", "Most profitable positions (by cash P&L)"),
        ("get_user_largest_positions", "Largest positions (by current value)"),
        ("get_user_redeemable_positions", "Positions ready for redemption"),
        ("get_user_positions_by_events", "Positions filtered by event IDs"),
        ("get_user_significant_positions", "Positions above size threshold")
    ]
    
    for func_name, description in functions_info:
        print(f"📊 {func_name}")
        print(f"   {description}")
        print()
    
    print("Usage Examples:")
    print("-" * 20)
    print("from positions_all import *")
    print()
    print("# Get all positions")
    print("positions = get_current_positions(user)")
    print()
    print("# Get top 10 profitable positions") 
    print("profitable = get_user_profitable_positions(user, limit=10)")
    print()
    print("# Get redeemable positions")
    print("redeemable = get_user_redeemable_positions(user)")
    print()
    print("# Get positions for specific events")
    print("event_positions = get_user_positions_by_events(user, [123, 456])")
    print()
    print("API Endpoint: https://data-api.polymarket.com/positions")
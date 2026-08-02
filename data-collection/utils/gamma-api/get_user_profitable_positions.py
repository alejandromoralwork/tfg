#!/usr/bin/env python3
"""
Get user's most profitable positions
"""

from typing import Dict, Any, List
from positions_base import get_current_positions, SortBy, SortDirection


def get_user_profitable_positions(user: str, limit: int = 50) -> List[Dict[str, Any]]:
    """
    Get user's most profitable positions sorted by cash P&L
    
    Args:
        user (str): User wallet address
        limit (int): Maximum number of positions to return (default: 50)
    
    Returns:
        List[Dict[str, Any]]: Positions sorted by cash P&L (highest first)
    
    Example:
        # Get top 20 most profitable positions
        profitable = get_user_profitable_positions("0xF937dBe9976Ac34157b30DD55BDbf248458F6b43", limit=20)
    """
    return get_current_positions(
        user=user,
        sort_by=SortBy.CASHPNL,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing get_user_profitable_positions function")
    print("=" * 50)
    
    # Example user address
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    
    try:
        print(f"Getting most profitable positions for: {test_user}")
        
        profitable = get_user_profitable_positions(test_user, limit=10)
        
        if isinstance(profitable, list):
            print(f"Found {len(profitable)} profitable positions")
            
            # Display sample data if available
            if profitable:
                print("\nSample position:")
                sample = profitable[0]
                for key, value in list(sample.items())[:5]:  # Show first 5 fields
                    print(f"  {key}: {value}")
        else:
            print(f"Unexpected response type: {type(profitable)}")
            
    except Exception as e:
        print(f"Error: {e}")
        print("Note: Make sure the user address has positions")
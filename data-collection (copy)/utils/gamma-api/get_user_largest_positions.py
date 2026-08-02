#!/usr/bin/env python3
"""
Get user's largest positions by current value
"""

from typing import Dict, Any, List
from positions_base import get_current_positions, SortBy, SortDirection


def get_user_largest_positions(user: str, limit: int = 50) -> List[Dict[str, Any]]:
    """
    Get user's largest positions sorted by current value
    
    Args:
        user (str): User wallet address
        limit (int): Maximum number of positions to return (default: 50)
    
    Returns:
        List[Dict[str, Any]]: Positions sorted by current value (highest first)
    
    Example:
        # Get top 20 largest positions by value
        largest = get_user_largest_positions("0xF937dBe9976Ac34157b30DD55BDbf248458F6b43", limit=20)
    """
    return get_current_positions(
        user=user,
        sort_by=SortBy.CURRENT,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing get_user_largest_positions function")
    print("=" * 50)
    
    # Example user address
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    
    try:
        print(f"Getting largest positions for: {test_user}")
        
        largest = get_user_largest_positions(test_user, limit=10)
        
        if isinstance(largest, list):
            print(f"Found {len(largest)} largest positions")
            
            # Display sample data if available
            if largest:
                print("\nSample position:")
                sample = largest[0]
                for key, value in list(sample.items())[:5]:  # Show first 5 fields
                    print(f"  {key}: {value}")
        else:
            print(f"Unexpected response type: {type(largest)}")
            
    except Exception as e:
        print(f"Error: {e}")
        print("Note: Make sure the user address has positions")
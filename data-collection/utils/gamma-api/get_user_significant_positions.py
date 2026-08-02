#!/usr/bin/env python3
"""
Get user's significant positions above a size threshold
"""

from typing import Dict, Any, List
from positions_base import get_current_positions, SortBy, SortDirection


def get_user_significant_positions(user: str, min_size: float = 10.0, limit: int = 100) -> List[Dict[str, Any]]:
    """
    Get user's significant positions above a minimum size threshold
    
    Args:
        user (str): User wallet address
        min_size (float): Minimum position size threshold (default: 10.0)
        limit (int): Maximum number of positions to return (default: 100)
    
    Returns:
        List[Dict[str, Any]]: Positions above the size threshold, sorted by current value
    
    Example:
        # Get positions larger than 50 tokens
        significant = get_user_significant_positions(
            "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43", 
            min_size=50.0, 
            limit=20
        )
    """
    return get_current_positions(
        user=user,
        size_threshold=min_size,
        sort_by=SortBy.CURRENT,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing get_user_significant_positions function")
    print("=" * 50)
    
    # Example user address
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    min_threshold = 5.0
    
    try:
        print(f"Getting significant positions for: {test_user}")
        print(f"Minimum size threshold: {min_threshold}")
        
        significant = get_user_significant_positions(test_user, min_size=min_threshold, limit=15)
        
        if isinstance(significant, list):
            print(f"Found {len(significant)} significant positions")
            
            # Display sample data if available
            if significant:
                print("\nSample significant position:")
                sample = significant[0]
                for key, value in list(sample.items())[:5]:  # Show first 5 fields
                    print(f"  {key}: {value}")
        else:
            print(f"Unexpected response type: {type(significant)}")
            
    except Exception as e:
        print(f"Error: {e}")
        print("Note: Make sure the user address has positions above the threshold")
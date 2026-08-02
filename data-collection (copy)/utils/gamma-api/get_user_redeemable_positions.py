#!/usr/bin/env python3
"""
Get user's redeemable positions
"""

from typing import Dict, Any, List
from positions_base import get_current_positions


def get_user_redeemable_positions(user: str, limit: int = 100) -> List[Dict[str, Any]]:
    """
    Get user's redeemable positions (positions that can be redeemed for winnings)
    
    Args:
        user (str): User wallet address
        limit (int): Maximum number of positions to return (default: 100)
    
    Returns:
        List[Dict[str, Any]]: Redeemable positions
    
    Example:
        # Get all redeemable positions
        redeemable = get_user_redeemable_positions("0xF937dBe9976Ac34157b30DD55BDbf248458F6b43")
    """
    return get_current_positions(
        user=user,
        redeemable=True,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing get_user_redeemable_positions function")
    print("=" * 50)
    
    # Example user address
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    
    try:
        print(f"Getting redeemable positions for: {test_user}")
        
        redeemable = get_user_redeemable_positions(test_user, limit=20)
        
        if isinstance(redeemable, list):
            print(f"Found {len(redeemable)} redeemable positions")
            
            # Display sample data if available
            if redeemable:
                print("\nSample redeemable position:")
                sample = redeemable[0]
                for key, value in list(sample.items())[:5]:  # Show first 5 fields
                    print(f"  {key}: {value}")
        else:
            print(f"Unexpected response type: {type(redeemable)}")
            
    except Exception as e:
        print(f"Error: {e}")
        print("Note: Make sure the user address has redeemable positions")
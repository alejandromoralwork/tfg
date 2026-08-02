#!/usr/bin/env python3
"""
Get user's positions for specific events
"""

from typing import Dict, Any, List
from positions_base import get_current_positions


def get_user_positions_by_events(user: str, event_ids: List[int], limit: int = 100) -> List[Dict[str, Any]]:
    """
    Get user's positions filtered by specific event IDs
    
    Args:
        user (str): User wallet address
        event_ids (List[int]): List of event IDs to filter by
        limit (int): Maximum number of positions to return (default: 100)
    
    Returns:
        List[Dict[str, Any]]: Positions for the specified events
    
    Example:
        # Get positions for specific events
        positions = get_user_positions_by_events(
            "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43", 
            [123, 456, 789], 
            limit=50
        )
    """
    return get_current_positions(
        user=user,
        event_id=event_ids,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing get_user_positions_by_events function")
    print("=" * 50)
    
    # Example user address and event IDs
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
    test_events = [123, 456, 789]  # Replace with real event IDs
    
    try:
        print(f"Getting positions for user: {test_user}")
        print(f"Event IDs: {test_events}")
        
        positions = get_user_positions_by_events(test_user, test_events, limit=20)
        
        if isinstance(positions, list):
            print(f"Found {len(positions)} positions for specified events")
            
            # Display sample data if available
            if positions:
                print("\nSample position:")
                sample = positions[0]
                for key, value in list(sample.items())[:5]:  # Show first 5 fields
                    print(f"  {key}: {value}")
        else:
            print(f"Unexpected response type: {type(positions)}")
            
    except Exception as e:
        print(f"Error: {e}")
        print("Note: Make sure the user address has positions for the specified events")
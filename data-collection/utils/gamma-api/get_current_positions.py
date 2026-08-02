#!/usr/bin/env python3
"""
Function to fetch current positions from Polymarket Data API /positions endpoint
"""

import requests
from typing import Dict, Any, List, Optional, Union
from enum import Enum


class SortBy(Enum):
    """Available sort options for positions"""
    CURRENT = "CURRENT"
    INITIAL = "INITIAL"
    TOKENS = "TOKENS"
    CASHPNL = "CASHPNL"
    PERCENTPNL = "PERCENTPNL"
    TITLE = "TITLE"
    RESOLVING = "RESOLVING"
    PRICE = "PRICE"
    AVGPRICE = "AVGPRICE"


class SortDirection(Enum):
    """Available sort directions"""
    ASC = "ASC"
    DESC = "DESC"


def get_current_positions(
    user: str,
    market: Optional[List[str]] = None,
    event_id: Optional[List[int]] = None,
    size_threshold: Optional[float] = 1.0,
    redeemable: Optional[bool] = False,
    mergeable: Optional[bool] = False,
    limit: Optional[int] = 100,
    offset: Optional[int] = 0,
    sort_by: Optional[Union[str, SortBy]] = SortBy.TOKENS,
    sort_direction: Optional[Union[str, SortDirection]] = SortDirection.DESC,
    title: Optional[str] = None,
    fields: Optional[List[str]] = None
) -> Union[List[Dict[str, Any]], Dict[str, Any]]:
    """
    Get current positions for a user from Polymarket Data API /positions endpoint
    URL: https://data-api.polymarket.com/positions
    
    Args:
        user (str): User address (required) - 0x-prefixed hex string
                   Example: "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"
        market (List[str], optional): Condition IDs to filter by (mutually exclusive with event_id)
                                    0x-prefixed 64-hex strings
        event_id (List[int], optional): Event IDs to filter by (mutually exclusive with market)
        size_threshold (float, optional): Minimum position size (default: 1.0, x >= 0)
        redeemable (bool, optional): Filter redeemable positions (default: False)
        mergeable (bool, optional): Filter mergeable positions (default: False)
        limit (int, optional): Max results to return (default: 100, range: 0-500)
        offset (int, optional): Skip results for pagination (default: 0, range: 0-10000)
        sort_by (str/SortBy, optional): Sort field (default: TOKENS)
                Available: CURRENT, INITIAL, TOKENS, CASHPNL, PERCENTPNL, TITLE, RESOLVING, PRICE, AVGPRICE
        sort_direction (str/SortDirection, optional): Sort direction (default: DESC)
                Available: ASC, DESC
        title (str, optional): Filter by title (max 100 chars)
        fields (List[str], optional): Specific fields to extract from response
    
    Returns:
        Union[List[Dict], Dict]: User positions data
    
    Examples:
        # Get all positions for a user
        positions = get_current_positions("0x56687bf447db6ffa42ffe2204a05edaa20f55839")
        
        # Get positions for specific markets
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            market=["0x1234567890abcdef...", "0xfedcba0987654321..."]
        )
        
        # Get positions for specific events
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            event_id=[123, 456, 789]
        )
        
        # Get large positions only, sorted by PnL
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            size_threshold=100.0,
            sort_by=SortBy.CASHPNL,
            sort_direction=SortDirection.DESC,
            limit=50
        )
        
        # Get redeemable positions
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            redeemable=True,
            limit=20
        )
        
        # Get positions with title filter
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            title="election",
            limit=10
        )
        
        # Get specific fields only
        positions = get_current_positions(
            user="0x56687bf447db6ffa42ffe2204a05edaa20f55839",
            fields=["market", "size", "value", "pnl"],
            limit=20
        )
    """
    # Validate required parameter
    if not user:
        raise ValueError("user parameter is required")
    
    # Validate user address format (basic check)
    if not user.startswith('0x') or len(user) != 42:
        raise ValueError("user must be a valid 0x-prefixed hex address (42 characters)")
    
    # Validate mutually exclusive parameters
    if market is not None and event_id is not None:
        raise ValueError("market and event_id parameters are mutually exclusive")
    
    # Validate ranges
    if size_threshold is not None and size_threshold < 0:
        raise ValueError("size_threshold must be >= 0")
    
    if limit is not None and not (0 <= limit <= 500):
        raise ValueError("limit must be between 0 and 500")
    
    if offset is not None and not (0 <= offset <= 10000):
        raise ValueError("offset must be between 0 and 10000")
    
    if title is not None and len(title) > 100:
        raise ValueError("title must be 100 characters or less")
    
    # Base URL for Data API (positions endpoint uses different API)
    base_url = "https://data-api.polymarket.com"
    
    # Construct the endpoint URL
    url = f"{base_url}/positions"
    
    # Build query parameters
    params = {}
    
    # Required parameter
    params['user'] = user
    
    # Market/Event filtering (mutually exclusive)
    if market is not None:
        # Convert list to comma-separated string
        params['market'] = ','.join(market)
    
    if event_id is not None:
        # Convert list to comma-separated string
        params['eventId'] = ','.join(map(str, event_id))
    
    # Filtering parameters
    if size_threshold is not None:
        params['sizeThreshold'] = str(size_threshold)
    
    if redeemable is not None:
        params['redeemable'] = 'true' if redeemable else 'false'
    
    if mergeable is not None:
        params['mergeable'] = 'true' if mergeable else 'false'
    
    # Pagination
    if limit is not None:
        params['limit'] = str(limit)
    
    if offset is not None:
        params['offset'] = str(offset)
    
    # Sorting
    if sort_by is not None:
        if isinstance(sort_by, SortBy):
            params['sortBy'] = sort_by.value
        else:
            params['sortBy'] = str(sort_by)
    
    if sort_direction is not None:
        if isinstance(sort_direction, SortDirection):
            params['sortDirection'] = sort_direction.value
        else:
            params['sortDirection'] = str(sort_direction)
    
    # Title filtering
    if title is not None:
        params['title'] = title
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        # Make the request
        response = requests.get(url, headers=headers, params=params, timeout=30)
        response.raise_for_status()  # Raise an exception for bad status codes
        
        # Parse JSON response
        data = response.json()
        
        # If no specific fields requested, return everything
        if fields is None:
            return data
        
        # Extract only requested fields from each position
        if isinstance(data, list):
            extracted_data = []
            for position in data:
                extracted_position = {}
                for field in fields:
                    if field in position:
                        extracted_position[field] = position[field]
                    else:
                        extracted_position[field] = None  # Field not found in response
                extracted_data.append(extracted_position)
            return extracted_data
        
        # If data is not a list, extract fields from the single object
        else:
            extracted_data = {}
            for field in fields:
                if field in data:
                    extracted_data[field] = data[field]
                else:
                    extracted_data[field] = None
            return extracted_data
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            raise ValueError(f"No positions found for user '{user}' with the specified criteria")
        elif e.response.status_code == 400:
            raise ValueError(f"Invalid request parameters: {e.response.text}")
        else:
            raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


# Convenience functions for common use cases
def get_user_profitable_positions(user: str, limit: int = 50) -> List[Dict[str, Any]]:
    """Get user's most profitable positions (convenience function)"""
    return get_current_positions(
        user=user,
        sort_by=SortBy.CASHPNL,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


def get_user_largest_positions(user: str, limit: int = 50) -> List[Dict[str, Any]]:
    """Get user's largest positions by value (convenience function)"""
    return get_current_positions(
        user=user,
        sort_by=SortBy.CURRENT,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


def get_user_redeemable_positions(user: str, limit: int = 100) -> List[Dict[str, Any]]:
    """Get user's redeemable positions (convenience function)"""
    return get_current_positions(
        user=user,
        redeemable=True,
        limit=limit
    )


def get_user_positions_by_events(user: str, event_ids: List[int], limit: int = 100) -> List[Dict[str, Any]]:
    """Get user's positions for specific events (convenience function)"""
    return get_current_positions(
        user=user,
        event_id=event_ids,
        limit=limit
    )


def get_user_significant_positions(user: str, min_size: float = 10.0, limit: int = 100) -> List[Dict[str, Any]]:
    """Get user's significant positions above threshold (convenience function)"""
    return get_current_positions(
        user=user,
        size_threshold=min_size,
        sort_by=SortBy.CURRENT,
        sort_direction=SortDirection.DESC,
        limit=limit
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing Polymarket Data API /positions endpoint")
    print("URL: https://data-api.polymarket.com/positions")
    print("=" * 60)
    
    # Example user address (using the one from your example)
    test_user = "0xF937dBe9976Ac34157b30DD55BDbf248458F6b43"  # Real address from example
    
    try:
        print(f"Testing with user: {test_user}")
        print("Note: Replace test_user with a real wallet address")
        print("-" * 50)
        
        # Test 1: Basic positions query
        print("1. Getting user's positions...")
        try:
            positions = get_current_positions(test_user, limit=5)
            if isinstance(positions, list):
                print(f"   Found {len(positions)} positions")
            else:
                print(f"   Response type: {type(positions)}")
        except ValueError as e:
            print(f"   Error: {e}")
            print("   Note: Update test_user with a real wallet address that has positions")
            exit()
        
        # Test 2: Get profitable positions
        print("\n2. Getting most profitable positions...")
        profitable = get_user_profitable_positions(test_user, limit=3)
        print(f"   Most profitable positions: {len(profitable)}")
        
        # Test 3: Get largest positions
        print("\n3. Getting largest positions...")
        largest = get_user_largest_positions(test_user, limit=3)
        print(f"   Largest positions: {len(largest)}")
        
        # Test 4: Get redeemable positions
        print("\n4. Getting redeemable positions...")
        redeemable = get_user_redeemable_positions(test_user, limit=5)
        print(f"   Redeemable positions: {len(redeemable)}")
        
        # Test 5: Get significant positions
        print("\n5. Getting significant positions (>= 10 tokens)...")
        significant = get_user_significant_positions(test_user, min_size=10.0, limit=5)
        print(f"   Significant positions: {len(significant)}")
        
        # Test 6: Get specific fields
        print("\n6. Getting positions with specific fields...")
        specific = get_current_positions(
            test_user, 
            fields=["market", "size", "value", "pnl"],
            limit=3
        )
        print(f"   Positions with specific fields: {len(specific)}")
        if specific and len(specific) > 0:
            print(f"   Sample fields: {list(specific[0].keys())}")
        
        # Test 7: Test sorting options
        print("\n7. Testing different sort options...")
        by_pnl = get_current_positions(
            test_user,
            sort_by=SortBy.PERCENTPNL,
            sort_direction=SortDirection.DESC,
            limit=2
        )
        print(f"   Sorted by % PnL: {len(by_pnl)} positions")
        
    except Exception as e:
        print(f"Error during test: {e}")
        print("\nNote: To test this function properly, you need to:")
        print("1. Find a real wallet address that has positions")
        print("2. Replace 'test_user' with that address")
        print("3. Run the test again")
        print(f"\nAPI Endpoint: https://data-api.polymarket.com/positions")
        print(f"Example URL: https://data-api.polymarket.com/positions?user=0xF937dBe9976Ac34157b30DD55BDbf248458F6b43")
#!/usr/bin/env python3
"""
Function to fetch trades from Polymarket Data API /trades endpoint
"""

import requests
from typing import Dict, Any, List, Optional, Union
from enum import Enum


class FilterType(Enum):
    """Available filter types for trades"""
    CASH = "CASH"
    TOKENS = "TOKENS"


class TradeSide(Enum):
    """Available trade sides"""
    BUY = "BUY"
    SELL = "SELL"


def get_trades(
    limit: Optional[int] = 100,
    offset: Optional[int] = None,
    taker_only: Optional[bool] = True,
    filter_type: Optional[Union[str, FilterType]] = None,
    filter_amount: Optional[float] = None,
    market: Optional[List[str]] = None,
    user: Optional[str] = None,
    side: Optional[Union[str, TradeSide]] = None,
    fields: Optional[List[str]] = None
) -> List[Dict[str, Any]]:
    """
    Fetch trades from Polymarket Data API /trades endpoint
    URL: https://data-api.polymarket.com/trades
    
    Fetches trades ordered by timestamp in descending order (most recent first)
    
    Args:
        limit (int, optional): Max number of trades to return (default: 100, max: 500)
        offset (int, optional): Starting index for pagination
        taker_only (bool, optional): Return only taker orders (default: True)
        filter_type (str/FilterType, optional): Filter trades by parameter (CASH or TOKENS)
        filter_amount (float, optional): Amount to filter by (related to filter_type)
        market (List[str], optional): Condition IDs to filter by (CSV separated values)
        user (str, optional): User wallet address to filter by
        side (str/TradeSide, optional): Trade side to filter by (BUY or SELL)
        fields (List[str], optional): Specific fields to extract from response
    
    Returns:
        List[Dict[str, Any]]: List of trade data ordered by timestamp (newest first)
    
    Response Fields:
        Trading: proxyWallet, side, asset, conditionId, size, price, timestamp
        Market: title, slug, icon, eventSlug, outcome, outcomeIndex
        User: name, pseudonym, bio, profileImage, profileImageOptimized
        Transaction: transactionHash
    
    Examples:
        # Get recent trades
        trades = get_trades(limit=50)
        
        # Get trades for specific user
        trades = get_trades(
            user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
            limit=100
        )
        
        # Get trades filtered by cash amount
        trades = get_trades(
            filter_type=FilterType.CASH,
            filter_amount=100.0,
            limit=50
        )
        
        # Get buy trades for specific market
        trades = get_trades(
            market=["0x1731c2d00c722fa4d53d1bddae549f14cf1870e2cf59dc040e7791046672cda5"],
            side=TradeSide.BUY,
            limit=25
        )
        
        # Get specific fields only
        trades = get_trades(
            user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
            fields=["side", "size", "price", "timestamp", "title"],
            limit=20
        )
        
        # Get maker and taker trades
        trades = get_trades(
            taker_only=False,
            limit=30
        )
    """
    # Validate parameters
    if limit is not None and not (1 <= limit <= 500):
        raise ValueError("limit must be between 1 and 500")
    
    if offset is not None and offset < 0:
        raise ValueError("offset must be >= 0")
    
    if user is not None and (not user.startswith('0x') or len(user) != 42):
        raise ValueError("user must be a valid 0x-prefixed hex address (42 characters)")
    
    if filter_type is not None and filter_amount is None:
        raise ValueError("filter_amount is required when filter_type is specified")
    
    if filter_amount is not None and filter_type is None:
        raise ValueError("filter_type is required when filter_amount is specified")
    
    # Base URL for Data API
    base_url = "https://data-api.polymarket.com"
    
    # Construct the endpoint URL
    url = f"{base_url}/trades"
    
    # Build query parameters
    params = {}
    
    if limit is not None:
        params['limit'] = str(limit)
    
    if offset is not None:
        params['offset'] = str(offset)
    
    if taker_only is not None:
        params['takerOnly'] = 'true' if taker_only else 'false'
    
    # Filter parameters
    if filter_type is not None:
        if isinstance(filter_type, FilterType):
            params['filterType'] = filter_type.value
        else:
            params['filterType'] = str(filter_type)
    
    if filter_amount is not None:
        params['filterAmount'] = str(filter_amount)
    
    # Market filtering
    if market is not None:
        # Convert list to comma-separated string
        params['market'] = ','.join(market)
    
    # User filtering
    if user is not None:
        params['user'] = user
    
    # Side filtering
    if side is not None:
        if isinstance(side, TradeSide):
            params['side'] = side.value
        else:
            params['side'] = str(side)
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json',
        'Content-Type': 'application/json'
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
        
        # Extract only requested fields from each trade
        if isinstance(data, list):
            extracted_data = []
            for trade in data:
                extracted_trade = {}
                for field in fields:
                    if field in trade:
                        extracted_trade[field] = trade[field]
                    else:
                        extracted_trade[field] = None  # Field not found in response
                extracted_data.append(extracted_trade)
            return extracted_data
        
        # If data is not a list, return as is (unexpected but handle gracefully)
        else:
            return data
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            raise ValueError(f"No trades found with the specified criteria")
        elif e.response.status_code == 400:
            raise ValueError(f"Invalid request parameters: {e.response.text}")
        else:
            raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


# Convenience functions for common use cases
def get_user_trades(user: str, limit: int = 100) -> List[Dict[str, Any]]:
    """Get trades for a specific user (convenience function)"""
    return get_trades(user=user, limit=limit)


def get_recent_trades(limit: int = 50) -> List[Dict[str, Any]]:
    """Get recent trades across all markets (convenience function)"""
    return get_trades(limit=limit)


def get_large_trades(min_cash_amount: float = 1000.0, limit: int = 100) -> List[Dict[str, Any]]:
    """Get large trades above cash threshold (convenience function)"""
    return get_trades(
        filter_type=FilterType.CASH,
        filter_amount=min_cash_amount,
        limit=limit
    )


def get_market_trades(condition_id: str, limit: int = 100) -> List[Dict[str, Any]]:
    """Get trades for a specific market (convenience function)"""
    return get_trades(market=[condition_id], limit=limit)


def get_buy_trades(user: Optional[str] = None, limit: int = 100) -> List[Dict[str, Any]]:
    """Get buy trades, optionally filtered by user (convenience function)"""
    return get_trades(user=user, side=TradeSide.BUY, limit=limit)


def get_sell_trades(user: Optional[str] = None, limit: int = 100) -> List[Dict[str, Any]]:
    """Get sell trades, optionally filtered by user (convenience function)"""
    return get_trades(user=user, side=TradeSide.SELL, limit=limit)


# Example usage and test function
if __name__ == "__main__":
    print("Testing Polymarket Data API /trades endpoint")
    print("URL: https://data-api.polymarket.com/trades")
    print("=" * 60)
    
    try:
        # Test 1: Get recent trades
        print("1. Getting recent trades...")
        recent = get_recent_trades(limit=5)
        print(f"   Found {len(recent)} recent trades")
        
        if recent:
            sample = recent[0]
            print(f"   Sample trade:")
            print(f"     Side: {sample.get('side', 'N/A')}")
            print(f"     Size: {sample.get('size', 'N/A')}")
            print(f"     Price: {sample.get('price', 'N/A')}")
            print(f"     Title: {sample.get('title', 'N/A')}")
        
        # Test 2: Get trades for specific user
        print("\n2. Getting trades for specific user...")
        test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
        user_trades = get_user_trades(test_user, limit=3)
        print(f"   Found {len(user_trades)} trades for user")
        
        # Test 3: Get large trades
        print("\n3. Getting large trades (>= $100)...")
        large = get_large_trades(min_cash_amount=100.0, limit=5)
        print(f"   Found {len(large)} large trades")
        
        # Test 4: Get specific fields
        print("\n4. Getting trades with specific fields...")
        specific = get_trades(
            limit=3,
            fields=["side", "size", "price", "title", "timestamp"]
        )
        print(f"   Retrieved {len(specific)} trades with specific fields")
        if specific:
            print(f"   Fields: {list(specific[0].keys())}")
        
        # Test 5: Get buy trades only
        print("\n5. Getting buy trades only...")
        buys = get_buy_trades(limit=5)
        print(f"   Found {len(buys)} buy trades")
        
        # Test 6: Filter by market and side
        print("\n6. Testing market and side filtering...")
        test_market = "0x1731c2d00c722fa4d53d1bddae549f14cf1870e2cf59dc040e7791046672cda5"
        market_buys = get_trades(
            market=[test_market],
            side=TradeSide.BUY,
            limit=2
        )
        print(f"   Found {len(market_buys)} buy trades for specific market")
        
    except Exception as e:
        print(f"Error during test: {e}")
        print(f"\nAPI Endpoint: https://data-api.polymarket.com/trades")
        print("Note: Some tests may fail if there's no recent trading activity")
#!/usr/bin/env python3
"""
Simple function to fetch data from Gamma API /markets endpoint with tag filtering
"""

import requests
from typing import Dict, Any, List, Optional, Union


def get_markets_with_tags(
    tag_id: Optional[Union[int, str]] = None,
    limit: Optional[int] = None,
    closed: Optional[bool] = False,
    offset: Optional[int] = None,
    order: Optional[str] = "id",
    ascending: Optional[bool] = False,
    fields: Optional[List[str]] = None
) -> Dict[str, Any]:
    """
    Fetch markets data from Gamma API /markets endpoint with filtering options
    
    Args:
        tag_id (int/str, optional): Tag ID to filter markets by
        limit (int, optional): Maximum number of results to return (controls response size)
        closed (bool, optional): Filter by market status (True=closed, False=active/default, None=all)
        offset (int, optional): Number of results to skip (for pagination)
        order (str, optional): Field to order by - default "id" (order by market ID)
        ascending (bool, optional): Sort order - default False (get newest markets first)
        fields (List[str], optional): List of fields to extract from response. 
                                    If None, returns everything.
    
    Returns:
        Dict[str, Any]: Parsed markets data with specified fields or all data
    
    Key Parameters (Polymarket recommended):
        - order="id" - Order by market ID
        - ascending=False - Get newest markets first
        - closed=False - Only active markets  
        - limit - Control response size
        - offset - For pagination
    
    Examples:
        # Get all active markets with tag 100381, limit 10
        data = get_markets_with_tags(tag_id=100381, limit=10, closed=False)
        
        # Get first 5 newest active markets (uses defaults: order="id", ascending=False, closed=False)
        data = get_markets_with_tags(limit=5)
        
        # Get specific fields only from active markets
        data = get_markets_with_tags(tag_id=100381, fields=["id", "question", "tokens"])
        
        # Pagination: Get next page of results
        data = get_markets_with_tags(limit=50, offset=50)
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Construct the endpoint URL
    url = f"{base_url}/markets"
    
    # Build query parameters
    params = {}
    
    if tag_id is not None:
        params['tag_id'] = str(tag_id)
    
    if limit is not None:
        params['limit'] = str(limit)
    
    if closed is not None:
        params['closed'] = 'true' if closed else 'false'
    
    if offset is not None:
        params['offset'] = str(offset)
    
    if order is not None:
        params['order'] = order
    
    if ascending is not None:
        params['ascending'] = 'true' if ascending else 'false'
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        # Make the request
        response = requests.get(url, headers=headers, params=params, timeout=15)
        response.raise_for_status()  # Raise an exception for bad status codes
        
        # Parse JSON response
        data = response.json()
        
        # If no specific fields requested, return everything
        if fields is None:
            return data
        
        # Extract only requested fields from each market (assuming data is a list)
        if isinstance(data, list):
            extracted_data = []
            for market in data:
                extracted_market = {}
                for field in fields:
                    if field in market:
                        extracted_market[field] = market[field]
                    else:
                        extracted_market[field] = None  # Field not found in response
                extracted_data.append(extracted_market)
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
            raise ValueError(f"No markets found with the specified criteria")
        else:
            raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


# Example usage and quick test function
if __name__ == "__main__":
    # Test the function
    try:
        print("Testing Gamma API /markets endpoint")
        print("=" * 50)
        
        # Test 1: Get active markets with specific tag (like the curl example)
        print("1. Fetching active markets with tag 100381, limit 1...")
        tag_data = get_markets_with_tags(tag_id=100381, limit=1, closed=False)
        print(f"   Status: Success")
        if isinstance(tag_data, list):
            print(f"   Markets found: {len(tag_data)}")
            if tag_data:
                print(f"   First market keys: {list(tag_data[0].keys())}")
        else:
            print(f"   Response keys: {list(tag_data.keys())}")
        
        # Test 2: Get first 3 active markets (any tag)
        print("\n2. Fetching first 3 active markets...")
        active_markets = get_markets_with_tags(limit=3, closed=False)
        print(f"   Status: Success")
        if isinstance(active_markets, list):
            print(f"   Markets found: {len(active_markets)}")
        
        # Test 3: Get specific fields only
        print("\n3. Fetching specific fields from active markets...")
        specific_fields = ["id", "question", "active", "closed", "tokens"]
        field_data = get_markets_with_tags(limit=2, closed=False, fields=specific_fields)
        print(f"   Requested fields: {specific_fields}")
        print(f"   Retrieved data:")
        if isinstance(field_data, list):
            for i, market in enumerate(field_data):
                print(f"   Market {i+1}:")
                for field, value in market.items():
                    if value is not None:
                        print(f"     {field}: {str(value)[:100]}{'...' if len(str(value)) > 100 else ''}")
                    else:
                        print(f"     {field}: None (field not found)")
        
    except Exception as e:
        print(f"Error during test: {e}")
        import traceback
        traceback.print_exc()
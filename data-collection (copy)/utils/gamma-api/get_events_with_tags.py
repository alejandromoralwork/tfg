#!/usr/bin/env python3
"""
Simple function to fetch data from Gamma API /events endpoint with tag filtering
"""

import requests
from typing import Dict, Any, List, Optional, Union


def get_events_with_tags(
    tag_id: Optional[Union[int, str]] = None,
    limit: Optional[int] = None,
    closed: Optional[bool] = False,
    offset: Optional[int] = None,
    order: Optional[str] = "id",
    ascending: Optional[bool] = False,
    fields: Optional[List[str]] = None
) -> Dict[str, Any]:
    """
    Fetch events data from Gamma API /events endpoint with filtering options
    
    Args:
        tag_id (int/str, optional): Tag ID to filter events by
        limit (int, optional): Maximum number of results to return (controls response size)
        closed (bool, optional): Filter by event status (True=closed, False=active/default, None=all)
        offset (int, optional): Number of results to skip (for pagination)
        order (str, optional): Field to order by - default "id" (order by event ID)
        ascending (bool, optional): Sort order - default False (get newest events first)
        fields (List[str], optional): List of fields to extract from response. 
                                    If None, returns everything.
    
    Returns:
        Dict[str, Any]: Parsed events data with specified fields or all data
    
    Key Parameters (Polymarket recommended):
        - order="id" - Order by event ID
        - ascending=False - Get newest events first  
        - closed=False - Only active markets
        - limit - Control response size
        - offset - For pagination
    
    Examples:
        # Get all active events with tag 100381, limit 1 (like curl example)
        data = get_events_with_tags(tag_id=100381, limit=1, closed=False)
        
        # Get first 10 newest active events (uses defaults: order="id", ascending=False, closed=False)
        data = get_events_with_tags(limit=10)
        
        # Get specific fields only from active events
        data = get_events_with_tags(tag_id=100381, fields=["id", "title", "markets"])
        
        # Pagination: Get next page of results
        data = get_events_with_tags(limit=20, offset=20)
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Construct the endpoint URL
    url = f"{base_url}/events"
    
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
        
        # Extract only requested fields from each event (assuming data is a list)
        if isinstance(data, list):
            extracted_data = []
            for event in data:
                extracted_event = {}
                for field in fields:
                    if field in event:
                        extracted_event[field] = event[field]
                    else:
                        extracted_event[field] = None  # Field not found in response
                extracted_data.append(extracted_event)
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
            raise ValueError(f"No events found with the specified criteria")
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
        print("Testing Gamma API /events endpoint")
        print("=" * 50)
        
        # Test 1: Get active events with specific tag (exactly like the curl example)
        print("1. Fetching active events with tag 100381, limit 1...")
        tag_data = get_events_with_tags(tag_id=100381, limit=1, closed=False)
        print(f"   Status: Success")
        if isinstance(tag_data, list):
            print(f"   Events found: {len(tag_data)}")
            if tag_data:
                print(f"   First event keys: {list(tag_data[0].keys())}")
        else:
            print(f"   Response keys: {list(tag_data.keys())}")
        
        # Test 2: Get first 3 active events (any tag)
        print("\n2. Fetching first 3 active events...")
        active_events = get_events_with_tags(limit=3, closed=False)
        print(f"   Status: Success")
        if isinstance(active_events, list):
            print(f"   Events found: {len(active_events)}")
        
        # Test 3: Get specific fields only
        print("\n3. Fetching specific fields from active events...")
        specific_fields = ["id", "title", "description", "markets", "active"]
        field_data = get_events_with_tags(limit=2, closed=False, fields=specific_fields)
        print(f"   Requested fields: {specific_fields}")
        print(f"   Retrieved data:")
        if isinstance(field_data, list):
            for i, event in enumerate(field_data):
                print(f"   Event {i+1}:")
                for field, value in event.items():
                    if value is not None:
                        if field == "markets" and isinstance(value, list):
                            print(f"     {field}: List with {len(value)} markets")
                        else:
                            print(f"     {field}: {str(value)[:100]}{'...' if len(str(value)) > 100 else ''}")
                    else:
                        print(f"     {field}: None (field not found)")
        
    except Exception as e:
        print(f"Error during test: {e}")
        import traceback
        traceback.print_exc()
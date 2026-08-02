#!/usr/bin/env python3
"""
Generic function to fetch data from Gamma API /events endpoint with all available parameters
"""

import requests
from typing import Dict, Any, List, Optional, Union
from datetime import datetime


def get_events(
    limit: Optional[int] = None,
    offset: Optional[int] = None,
    order: Optional[str] = None,
    ascending: Optional[bool] = None,
    id: Optional[List[int]] = None,
    slug: Optional[List[str]] = None,
    tag_id: Optional[int] = None,
    exclude_tag_id: Optional[List[int]] = None,
    related_tags: Optional[bool] = None,
    featured: Optional[bool] = None,
    cyom: Optional[bool] = None,
    include_chat: Optional[bool] = None,
    include_template: Optional[bool] = None,
    recurrence: Optional[str] = None,
    closed: Optional[bool] = None,
    start_date_min: Optional[Union[str, datetime]] = None,
    start_date_max: Optional[Union[str, datetime]] = None,
    end_date_min: Optional[Union[str, datetime]] = None,
    end_date_max: Optional[Union[str, datetime]] = None,
    fields: Optional[List[str]] = None
) -> Union[List[Dict[str, Any]], Dict[str, Any]]:
    """
    Generic function to fetch events from Gamma API /events endpoint with all available parameters
    
    Args:
        limit (int, optional): Maximum number of results to return (x >= 0)
        offset (int, optional): Number of results to skip for pagination (x >= 0)
        order (str, optional): Comma-separated list of fields to order by
        ascending (bool, optional): Sort order (True=ascending, False=descending)
        id (List[int], optional): Filter by specific event IDs
        slug (List[str], optional): Filter by specific event slugs
        tag_id (int, optional): Filter by specific tag ID
        exclude_tag_id (List[int], optional): Exclude events with these tag IDs
        related_tags (bool, optional): Include related tags information
        featured (bool, optional): Filter by featured events
        cyom (bool, optional): Filter by CYOM (Create Your Own Market) events
        include_chat (bool, optional): Include chat information
        include_template (bool, optional): Include template information
        recurrence (str, optional): Filter by recurrence pattern
        closed (bool, optional): Filter by event status (True=closed, False=active)
        start_date_min (str/datetime, optional): Minimum start date (ISO format)
        start_date_max (str/datetime, optional): Maximum start date (ISO format)
        end_date_min (str/datetime, optional): Minimum end date (ISO format)
        end_date_max (str/datetime, optional): Maximum end date (ISO format)
        fields (List[str], optional): Specific fields to extract from response
    
    Returns:
        Union[List[Dict], Dict]: Events data (list of events or single event)
    
    Examples:
        # Basic usage - get active events
        events = get_events(limit=10, closed=False)
        
        # Filter by tag and date range
        events = get_events(
            tag_id=100381, 
            limit=20,
            start_date_min="2025-01-01T00:00:00Z",
            end_date_max="2025-12-31T23:59:59Z"
        )
        
        # Get specific events by ID
        events = get_events(id=[123, 456, 789])
        
        # Get events by slugs
        events = get_events(slug=["event-1", "event-2"])
        
        # Featured events only with specific fields
        events = get_events(
            featured=True,
            limit=5,
            fields=["id", "title", "description", "start_date"]
        )
        
        # Complex filtering
        events = get_events(
            limit=50,
            offset=100,
            order="start_date,created_at",
            ascending=False,
            tag_id=100381,
            exclude_tag_id=[999, 888],
            featured=True,
            closed=False,
            include_chat=True
        )
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Construct the endpoint URL
    url = f"{base_url}/events"
    
    # Build query parameters
    params = {}
    
    # Basic pagination and ordering
    if limit is not None:
        if limit < 0:
            raise ValueError("limit must be >= 0")
        params['limit'] = str(limit)
    
    if offset is not None:
        if offset < 0:
            raise ValueError("offset must be >= 0")
        params['offset'] = str(offset)
    
    if order is not None:
        params['order'] = order
    
    if ascending is not None:
        params['ascending'] = 'true' if ascending else 'false'
    
    # ID and slug filtering
    if id is not None:
        # Convert list of IDs to comma-separated string
        params['id'] = ','.join(map(str, id))
    
    if slug is not None:
        # Convert list of slugs to comma-separated string
        params['slug'] = ','.join(slug)
    
    # Tag filtering
    if tag_id is not None:
        params['tag_id'] = str(tag_id)
    
    if exclude_tag_id is not None:
        # Convert list of tag IDs to comma-separated string
        params['exclude_tag_id'] = ','.join(map(str, exclude_tag_id))
    
    # Boolean flags
    if related_tags is not None:
        params['related_tags'] = 'true' if related_tags else 'false'
    
    if featured is not None:
        params['featured'] = 'true' if featured else 'false'
    
    if cyom is not None:
        params['cyom'] = 'true' if cyom else 'false'
    
    if include_chat is not None:
        params['include_chat'] = 'true' if include_chat else 'false'
    
    if include_template is not None:
        params['include_template'] = 'true' if include_template else 'false'
    
    if closed is not None:
        params['closed'] = 'true' if closed else 'false'
    
    # String parameters
    if recurrence is not None:
        params['recurrence'] = recurrence
    
    # Date parameters (convert datetime objects to ISO strings)
    def format_date(date_param):
        if isinstance(date_param, datetime):
            return date_param.isoformat()
        return str(date_param)
    
    if start_date_min is not None:
        params['start_date_min'] = format_date(start_date_min)
    
    if start_date_max is not None:
        params['start_date_max'] = format_date(start_date_max)
    
    if end_date_min is not None:
        params['end_date_min'] = format_date(end_date_min)
    
    if end_date_max is not None:
        params['end_date_max'] = format_date(end_date_max)
    
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
        
        # Extract only requested fields from each event
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


# Convenience functions for common use cases
def get_active_events(limit: int = 50, **kwargs) -> List[Dict[str, Any]]:
    """Get active events (convenience function)"""
    return get_events(limit=limit, closed=False, **kwargs)


def get_featured_events(limit: int = 20, **kwargs) -> List[Dict[str, Any]]:
    """Get featured events (convenience function)"""
    return get_events(limit=limit, featured=True, **kwargs)


def get_events_by_tag(tag_id: int, limit: int = 50, **kwargs) -> List[Dict[str, Any]]:
    """Get events by specific tag (convenience function)"""
    return get_events(tag_id=tag_id, limit=limit, **kwargs)


def get_events_in_date_range(
    start_min: Union[str, datetime], 
    start_max: Union[str, datetime], 
    limit: int = 100, 
    **kwargs
) -> List[Dict[str, Any]]:
    """Get events in a specific date range (convenience function)"""
    return get_events(
        start_date_min=start_min,
        start_date_max=start_max,
        limit=limit,
        **kwargs
    )


# Example usage and test function
if __name__ == "__main__":
    print("Testing Generic Gamma API /events endpoint")
    print("=" * 60)
    
    try:
        # Test 1: Basic active events
        print("1. Getting active events...")
        active = get_events(limit=3, closed=False)
        print(f"   Found {len(active)} active events")
        
        # Test 2: Featured events with specific fields
        print("\n2. Getting featured events with specific fields...")
        featured = get_events(
            featured=True, 
            limit=2, 
            fields=["id", "title", "featured"]
        )
        print(f"   Found {len(featured)} featured events")
        if featured:
            for event in featured:
                print(f"   - {event.get('title', 'N/A')} (ID: {event.get('id', 'N/A')})")
        
        # Test 3: Events with tag filtering
        print("\n3. Getting events with tag filtering...")
        tagged = get_events(tag_id=100381, limit=2)
        print(f"   Found {len(tagged)} events with tag 100381")
        
        # Test 4: Complex filtering example
        print("\n4. Complex filtering example...")
        complex_filter = get_events(
            limit=5,
            order="created_at",
            ascending=False,
            closed=False,
            include_chat=True,
            fields=["id", "title", "closed", "created_at"]
        )
        print(f"   Found {len(complex_filter)} events with complex filtering")
        
        # Test 5: Convenience functions
        print("\n5. Testing convenience functions...")
        active_conv = get_active_events(limit=2)
        print(f"   Active events: {len(active_conv)}")
        
        featured_conv = get_featured_events(limit=2)
        print(f"   Featured events: {len(featured_conv)}")
        
    except Exception as e:
        print(f"Error during test: {e}")
        import traceback
        traceback.print_exc()
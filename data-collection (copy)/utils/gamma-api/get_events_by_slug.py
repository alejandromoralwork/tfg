#!/usr/bin/env python3
"""
Simple function to fetch data from Gamma API /events/slug/ endpoint
"""

import requests
from typing import Dict, Any, List, Optional


def get_events_by_slug(slug: str, fields: Optional[List[str]] = None) -> Dict[str, Any]:
    """
    Fetch event data from Gamma API /events/slug/ endpoint
    
    Args:
        slug (str): The event slug to fetch
        fields (List[str], optional): List of fields to extract from response. 
                                    If None, returns everything.
    
    Returns:
        Dict[str, Any]: Parsed event data with specified fields or all data
    
    Example:
        # Get all data
        data = fetch_event_by_slug("lal-vil-bet-2025-10-18")
        
        # Get specific fields only
        data = fetch_event_by_slug("lal-vil-bet-2025-10-18", ["id", "title", "markets"])
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Clean the slug (convert to lowercase, replace spaces with hyphens)
    clean_slug = slug.lower().strip().replace(' ', '-')
    
    # Construct the endpoint URL
    url = f"{base_url}/events/slug/{clean_slug}"
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        # Make the request
        response = requests.get(url, headers=headers, timeout=15)
        response.raise_for_status()  # Raise an exception for bad status codes
        
        # Parse JSON response
        data = response.json()
        
        # If no specific fields requested, return everything
        if fields is None:
            return data
        
        # Extract only requested fields
        extracted_data = {}
        for field in fields:
            if field in data:
                extracted_data[field] = data[field]
            else:
                extracted_data[field] = None  # Field not found in response
        
        return extracted_data
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            raise ValueError(f"Event with slug '{slug}' not found")
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
        # Test with a known slug
        test_slug = "ssc-fcb-rmc-2026-01-11"
        
        print(f"Testing with slug: {test_slug}")
        print("=" * 50)
        
        # Get all data
        print("1. Fetching all data...")
        all_data = get_events_by_slug(test_slug)
        print(f"   Status: Success")
        print(f"   Keys available: {list(all_data.keys())}")
        print(f"   The data: {all_data}")
        
        # Get specific fields
        print("\n2. Fetching specific fields...")
        specific_fields = ["id", "title", "description", "markets"]
        field_data = get_events_by_slug(test_slug, specific_fields)
        print(f"   Requested fields: {specific_fields}")
        print(f"   Retrieved data:")
        for field, value in field_data.items():
            if value is not None:
                print(f"     {field}: {str(value)[:100]}{'...' if len(str(value)) > 100 else ''}")
            else:
                print(f"     {field}: None (field not found)")
        
    except Exception as e:
        print(f"Error during test: {e}")

#!/usr/bin/env python3
"""
Function to fetch event data by ID from Gamma API /events/{id} endpoint
"""

import requests
from typing import Dict, Any, List, Optional, Union


def get_event_by_id(
    event_id: Union[int, str],
    include_chat: Optional[bool] = None,
    include_template: Optional[bool] = None,
    fields: Optional[List[str]] = None
) -> Dict[str, Any]:
    """
    Fetch event data by ID from Gamma API /events/{id} endpoint
    
    Args:
        event_id (int/str): The event ID to fetch (required)
        include_chat (bool, optional): Include chat information
        include_template (bool, optional): Include template information
        fields (List[str], optional): Specific fields to extract from response.
                                    If None, returns everything.
    
    Returns:
        Dict[str, Any]: Complete event data with all available fields
    
    Available Response Fields:
        Basic Info: id, ticker, slug, title, subtitle, description, resolutionSource
        Dates: startDate, creationDate, endDate, createdAt, updatedAt, closedTime
        Media: image, icon, featuredImage, imageOptimized, iconOptimized, featuredImageOptimized
        Status: active, closed, archived, new, featured, restricted
        Financial: liquidity, volume, openInterest, volume24hr, volume1wk, volume1mo, volume1yr
        Classification: category, subcategory, sortBy
        Template: isTemplate, templateVariables
        Social: commentsEnabled, commentCount, disqusThread, tweetCount
        Trading: competitive, enableOrderBook, liquidityAmm, liquidityClob
        Risk: negRisk, negRiskMarketID, negRiskFeeBips, enableNegRisk
        Relationships: parentEvent, subEvents, markets, series, categories, collections, tags
        CYOM: cyom, eventCreators
        Display: showAllOutcomes, showMarketImages, featuredOrder
        Automation: automaticallyResolved, automaticallyActive, pendingDeployment, deploying
        Estimation: estimateValue, cantEstimate, estimatedValue
        Sports: eventDate, startTime, eventWeek, score, elapsed, period, live, ended
        Charts: gmpChartMode, spreadsMainLine, totalsMainLine, carouselMap
        Metadata: published_at, createdBy, updatedBy, seriesSlug, finishedTimestamp
        
    Examples:
        # Get complete event data
        event = get_event_by_id(12345)
        
        # Get event with chat and template info
        event = get_event_by_id(12345, include_chat=True, include_template=True)
        
        # Get specific fields only
        event = get_event_by_id(12345, fields=["id", "title", "markets", "volume"])
        
        # Get basic event info
        basic_info = get_event_by_id(
            12345, 
            fields=["id", "title", "description", "startDate", "endDate", "active"]
        )
        
        # Get financial data
        financial = get_event_by_id(
            12345,
            fields=["id", "title", "liquidity", "volume", "volume24hr", "openInterest"]
        )
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Construct the endpoint URL with event ID
    url = f"{base_url}/events/{event_id}"
    
    # Build query parameters
    params = {}
    
    if include_chat is not None:
        params['include_chat'] = 'true' if include_chat else 'false'
    
    if include_template is not None:
        params['include_template'] = 'true' if include_template else 'false'
    
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
            raise ValueError(f"Event with ID '{event_id}' not found")
        else:
            raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


# Convenience functions for common field groups
def get_event_basic_info(event_id: Union[int, str]) -> Dict[str, Any]:
    """Get basic event information (convenience function)"""
    basic_fields = [
        "id", "title", "subtitle", "description", "slug", 
        "startDate", "endDate", "active", "closed", "featured"
    ]
    return get_event_by_id(event_id, fields=basic_fields)


def get_event_financial_data(event_id: Union[int, str]) -> Dict[str, Any]:
    """Get event financial/trading data (convenience function)"""
    financial_fields = [
        "id", "title", "liquidity", "volume", "openInterest",
        "volume24hr", "volume1wk", "volume1mo", "volume1yr",
        "liquidityAmm", "liquidityClob", "competitive"
    ]
    return get_event_by_id(event_id, fields=financial_fields)


def get_event_with_markets(event_id: Union[int, str]) -> Dict[str, Any]:
    """Get event with markets data (convenience function)"""
    market_fields = [
        "id", "title", "description", "markets", "active", 
        "closed", "startDate", "endDate", "volume", "liquidity"
    ]
    return get_event_by_id(event_id, fields=market_fields)


def get_event_metadata(event_id: Union[int, str]) -> Dict[str, Any]:
    """Get event metadata and classification (convenience function)"""
    metadata_fields = [
        "id", "title", "category", "subcategory", "tags", "collections",
        "createdAt", "updatedAt", "createdBy", "updatedBy", "published_at"
    ]
    return get_event_by_id(event_id, fields=metadata_fields)


def get_event_status(event_id: Union[int, str]) -> Dict[str, Any]:
    """Get event status information (convenience function)"""
    status_fields = [
        "id", "title", "active", "closed", "archived", "new", 
        "featured", "restricted", "live", "ended", "finishedTimestamp"
    ]
    return get_event_by_id(event_id, fields=status_fields)


# Example usage and test function
if __name__ == "__main__":
    print("Testing Gamma API /events/{id} endpoint")
    print("=" * 50)
    
    try:
        # You'll need to replace this with a real event ID
        test_event_id = 12345  # Replace with actual event ID
        
        print(f"Testing with event ID: {test_event_id}")
        print("-" * 30)
        
        # Test 1: Get basic event info
        print("1. Getting basic event info...")
        try:
            basic = get_event_basic_info(test_event_id)
            print(f"   Event Title: {basic.get('title', 'N/A')}")
            print(f"   Event ID: {basic.get('id', 'N/A')}")
            print(f"   Active: {basic.get('active', 'N/A')}")
            print(f"   Start Date: {basic.get('startDate', 'N/A')}")
        except ValueError as e:
            print(f"   Event not found: {e}")
            print("   Note: Update test_event_id with a real event ID")
            print("   Skipping remaining tests...")
            exit()
        
        # Test 2: Get financial data
        print("\n2. Getting financial data...")
        financial = get_event_financial_data(test_event_id)
        print(f"   Volume: {financial.get('volume', 'N/A')}")
        print(f"   Liquidity: {financial.get('liquidity', 'N/A')}")
        print(f"   24h Volume: {financial.get('volume24hr', 'N/A')}")
        
        # Test 3: Get event with markets
        print("\n3. Getting event with markets...")
        with_markets = get_event_with_markets(test_event_id)
        markets = with_markets.get('markets', [])
        print(f"   Markets count: {len(markets) if markets else 0}")
        
        # Test 4: Get specific fields
        print("\n4. Getting specific fields...")
        specific = get_event_by_id(
            test_event_id, 
            fields=["id", "title", "volume", "active", "markets"]
        )
        print(f"   Retrieved fields: {list(specific.keys())}")
        
        # Test 5: Get with optional parameters
        print("\n5. Getting with chat and template info...")
        full_data = get_event_by_id(
            test_event_id,
            include_chat=True,
            include_template=True
        )
        print(f"   Total response fields: {len(full_data.keys())}")
        
        # Display some key fields if available
        if 'chats' in full_data:
            chats = full_data.get('chats', [])
            print(f"   Chats: {len(chats) if chats else 0}")
        
        if 'templates' in full_data:
            templates = full_data.get('templates', [])
            print(f"   Templates: {len(templates) if templates else 0}")
        
    except Exception as e:
        print(f"Error during test: {e}")
        print("\nNote: To test this function properly, you need to:")
        print("1. Find a real event ID from the API")
        print("2. Replace 'test_event_id = 12345' with the real ID")
        print("3. Run the test again")
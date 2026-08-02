#!/usr/bin/env python3
"""
Fetch sports metadata from Gamma API /sports endpoint

This provides:
- Sport identifiers (football, basketball, etc.)
- Tag IDs for filtering events
- Resolution sources
- Images and ordering preferences
"""

import requests
from typing import List, Dict, Any, Optional


def get_sports_metadata() -> List[Dict[str, Any]]:
    """
    Fetch sports metadata from Gamma API
    
    Returns:
        List[Dict]: List of sports metadata objects with:
            - sport: Sport identifier (e.g., "football", "basketball")
            - image: URL to sport logo
            - resolution: Official resolution source URL
            - ordering: Display ordering preference ("home"/"away")
            - tags: Comma-separated tag IDs
            - series: Series identifier
    
    Example response:
        [
            {
                "sport": "football",
                "image": "https://...",
                "resolution": "https://...",
                "ordering": "home",
                "tags": "100381,100382",
                "series": "..."
            },
            ...
        ]
    """
    url = "https://gamma-api.polymarket.com/sports"
    
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        response = requests.get(url, headers=headers, timeout=15)
        response.raise_for_status()
        
        data = response.json()
        return data if isinstance(data, list) else []
        
    except Exception as e:
        print(f"Error fetching sports metadata: {e}")
        return []


def get_sport_tags(sport_name: str) -> List[int]:
    """
    Get tag IDs for a specific sport
    
    Args:
        sport_name: Sport identifier (e.g., "football", "basketball")
    
    Returns:
        List[int]: List of tag IDs for the sport
    """
    sports = get_sports_metadata()
    
    for sport in sports:
        if sport.get('sport', '').lower() == sport_name.lower():
            tags_str = sport.get('tags', '')
            if tags_str:
                try:
                    return [int(tag.strip()) for tag in tags_str.split(',') if tag.strip()]
                except ValueError:
                    return []
    
    return []


def get_all_sports_tags() -> Dict[str, List[int]]:
    """
    Get all sports and their tag IDs
    
    Returns:
        Dict[str, List[int]]: Mapping of sport name to list of tag IDs
    """
    sports = get_sports_metadata()
    result = {}
    
    for sport in sports:
        sport_name = sport.get('sport', '')
        tags_str = sport.get('tags', '')
        
        if sport_name and tags_str:
            try:
                tag_ids = [int(tag.strip()) for tag in tags_str.split(',') if tag.strip()]
                result[sport_name] = tag_ids
            except ValueError:
                pass
    
    return result


def main():
    """Example usage and testing"""
    print("🏅 Sports Metadata from Gamma API")
    print("=" * 70)
    
    sports = get_sports_metadata()
    
    if not sports:
        print("❌ No sports metadata available")
        return
    
    print(f"✅ Found {len(sports)} sports\n")
    
    for sport in sports:
        sport_name = sport.get('sport', 'Unknown')
        tags = sport.get('tags', 'None')
        resolution = sport.get('resolution', 'N/A')
        
        print(f"🏆 {sport_name.upper()}")
        print(f"   Tags: {tags}")
        print(f"   Resolution: {resolution[:50]}...")
        print()
    
    # Show tag mapping
    print("\n" + "=" * 70)
    print("📋 Sport Tag ID Mapping")
    print("=" * 70)
    
    all_tags = get_all_sports_tags()
    for sport_name, tag_ids in sorted(all_tags.items()):
        print(f"{sport_name:15s} → Tag IDs: {tag_ids}")
    
    # Test specific sport
    print("\n" + "=" * 70)
    print("🔍 Testing Football Tags")
    print("=" * 70)
    
    football_tags = get_sport_tags("football")
    if football_tags:
        print(f"✅ Football tag IDs: {football_tags}")
        print(f"   First tag to use: {football_tags[0]}")
    else:
        print("❌ No football tags found")


if __name__ == "__main__":
    main()

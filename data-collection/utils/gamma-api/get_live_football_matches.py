#!/usr/bin/env python3
"""
Function to fetch all live football matches from Polymarket
"""

import requests
import re
import csv
from datetime import datetime
from typing import Dict, Any, List, Optional
from get_events_by_slug import get_events_by_slug


def get_live_football_matches(fields: Optional[List[str]] = None) -> Dict[str, Dict[str, Any]]:
    """
    Fetch all live football matches from Polymarket
    
    Strategy:
    1. Query /sports endpoint to get all football leagues and their series IDs
    2. Filter for football/soccer sports by checking tags for tag "100350" (soccer/football tag)
    3. For each football league, query /events with series ID filter
    4. Collect all active events with endDate (live/upcoming matches)
    5. Return dictionary with slug as key and event data as value
    
    Args:
        fields (List[str], optional): List of fields to extract from each event. 
                                     If None, returns everything from the events list.
                                     If specified, fetches detailed data per event.
    
    Returns:
        Dict[str, Dict[str, Any]]: Dictionary where keys are slugs and values are event data
                                   Returns empty dict if no matches found
    
    Example:
        # Get all data for all live matches
        matches = get_live_football_matches()
        
        # Get specific fields only
        matches = get_live_football_matches(["id", "title", "markets", "startDate"])
        
        # Access individual match data
        for slug, match_data in matches.items():
            print(f"Slug: {slug}")
            print(f"Title: {match_data.get('title')}")
    """
    
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    # Football/soccer tag ID (observed from /sports data: tag "100350" appears in all football leagues)
    FOOTBALL_TAG = "100350"
    
    try:
        # Step 1: Get all sports/leagues from /sports endpoint
        sports_url = f"{base_url}/sports"
        print(f"Fetching sports/leagues from {sports_url}...")
        
        response = requests.get(sports_url, headers=headers, timeout=15)
        response.raise_for_status()
        sports_data = response.json()
        
        if not isinstance(sports_data, list):
            raise Exception("Unexpected response format from /sports endpoint")
        
        print(f"✓ Retrieved {len(sports_data)} total sports/leagues")
        
        # Step 2: Filter for football/soccer leagues
        # Looking for tag "100350" which appears in all football leagues
        football_leagues = []
        for sport in sports_data:
            tags_str = sport.get('tags', '')
            # Tags are comma-separated string
            tags_list = tags_str.split(',') if tags_str else []
            
            if FOOTBALL_TAG in tags_list:
                football_leagues.append({
                    'sport': sport.get('sport'),
                    'series': sport.get('series'),
                    'id': sport.get('id'),
                    'name': sport.get('sport', 'unknown').upper()
                })
        
        print(f"✓ Found {len(football_leagues)} football leagues")
        
        if not football_leagues:
            print("⚠ No football leagues found")
            return {}
        
        # Show which leagues were found
        league_names = [league['name'] for league in football_leagues[:10]]
        print(f"  Leagues: {', '.join(league_names)}{' ...' if len(football_leagues) > 10 else ''}")
        
        # Step 3: Fetch events for each football league
        all_football_events = {}
        total_events_fetched = 0
        
        for league in football_leagues:
            series_id = league['series']
            league_name = league['name']
            
            if not series_id:
                continue
            
            print(f"\nFetching events for {league_name} (series: {series_id})...")
            
            # Query events for this series
            events_url = f"{base_url}/events"
            params = {
                'series_id': series_id,
                'active': 'true',
                'closed': 'false',
                'limit': 100
            }
            
            try:
                response = requests.get(events_url, headers=headers, params=params, timeout=15)
                response.raise_for_status()
                
                data = response.json()
                
                # Handle different response formats
                league_events = []
                if isinstance(data, list):
                    league_events = data
                elif isinstance(data, dict) and 'data' in data:
                    league_events = data['data'] if isinstance(data['data'], list) else [data['data']]
                elif isinstance(data, dict) and 'events' in data:
                    league_events = data['events'] if isinstance(data['events'], list) else [data['events']]
                
                # Filter events with endDate (live/upcoming matches)
                valid_events = 0
                for event in league_events:
                    slug = event.get('slug')
                    end_date = event.get('endDate')
                    
                    if slug and end_date and slug not in all_football_events:
                        all_football_events[slug] = event
                        valid_events += 1
                
                total_events_fetched += valid_events
                print(f"  ✓ {valid_events} active events")
                
            except Exception as e:
                print(f"  ⚠ Failed to fetch events for {league_name}: {str(e)}")
                continue
        
        print(f"\n{'='*50}")
        print(f"Total events collected: {len(all_football_events)}")
        print(f"{'='*50}")
        
        if not all_football_events:
            print("⚠ No football events found")
            return {}
        
        # Step 4: Process events - if fields are specified, fetch detailed data
        all_matches = {}
        successful = 0
        failed = 0
        
        for i, (slug, event) in enumerate(all_football_events.items(), 1):
            try:
                # If specific fields requested, fetch detailed data per event
                if fields is not None:
                    print(f"Fetching detailed data {i}/{len(all_football_events)}: {slug}")
                    event_data = get_events_by_slug(slug, fields)
                    all_matches[slug] = event_data
                else:
                    # Use the data we already have from the list
                    all_matches[slug] = event
                
                successful += 1
                
            except Exception as e:
                print(f"  ⚠ Failed to fetch details for {slug}: {str(e)}")
                # Still add the basic event data even if detailed fetch fails
                all_matches[slug] = event
                failed += 1
        
        print(f"\n{'='*50}")
        print(f"Summary: {successful} successful, {failed} failed")
        print(f"Total football matches: {len(all_matches)}")
        print(f"{'='*50}")
        
        return all_matches
        
    except requests.exceptions.HTTPError as e:
        raise Exception(f"HTTP error {e.response.status_code}: {e.response.text}")
    
    except requests.exceptions.RequestException as e:
        raise Exception(f"Network error: {str(e)}")
    
    except Exception as e:
        raise Exception(f"Unexpected error: {str(e)}")


def save_matches_to_csv(matches: Dict[str, Dict[str, Any]], filename: str = None) -> str:
    """
    Save football matches to CSV file with main data fields
    
    Args:
        matches: Dictionary of matches (slug -> event data)
        filename: Optional CSV filename. If None, auto-generates with timestamp
    
    Returns:
        str: Path to the created CSV file
    """
    if not matches:
        print("No matches to save")
        return None
    
    # Generate filename if not provided
    if filename is None:
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        filename = f"football_matches_{timestamp}.csv"
    
    # Define main fields to extract
    main_fields = [
        'slug', 'id', 'title', 'description', 'startDate', 'endDate',
        'active', 'closed', 'archived', 'category', 'icon', 'image'
    ]
    
    # Write to CSV
    with open(filename, 'w', newline='', encoding='utf-8') as csvfile:
        writer = csv.DictWriter(csvfile, fieldnames=main_fields, extrasaction='ignore')
        writer.writeheader()
        
        for slug, match_data in matches.items():
            # Add slug to the row data
            row = {'slug': slug}
            row.update(match_data)
            
            # Convert lists/dicts to string for CSV
            for key, value in row.items():
                if isinstance(value, (list, dict)):
                    row[key] = str(value)
            
            writer.writerow(row)
    
    print(f"\n✓ Saved {len(matches)} matches to {filename}")
    return filename


# Example usage and quick test function
if __name__ == "__main__":
    try:
        print("Testing get_live_football_matches()")
        print("=" * 50)
        
        # Test 1: Get all data
        print("\n1. Fetching all live matches with all data...")
        all_matches = get_live_football_matches()
        
        if all_matches:
            print(f"\n✓ Found {len(all_matches)} live matches")
            
            # Show summary of first match
            first_slug = list(all_matches.keys())[0]
            first_match = all_matches[first_slug]
            print(f"\nFirst match example:")
            print(f"  Slug: {first_slug}")
            print(f"  Available fields: {list(first_match.keys())}")
            if 'title' in first_match:
                print(f"  Title: {first_match['title']}")
            
            # Save to CSV
            csv_file = save_matches_to_csv(all_matches)
        else:
            print("✗ No live matches found")
        
        # Test 2: Get specific fields only
        print("\n2. Fetching specific fields only...")
        specific_fields = ["id", "title", "description", "markets", "startDate", "endDate"]
        matches_filtered = get_live_football_matches(specific_fields)
        
        if matches_filtered:
            print(f"\n✓ Retrieved {len(matches_filtered)} matches with filtered fields")
            
            # Show one example
            example_slug = list(matches_filtered.keys())[0]
            example_match = matches_filtered[example_slug]
            print(f"\nExample match data:")
            for field, value in example_match.items():
                if value is not None:
                    value_str = str(value)
                    if len(value_str) > 100:
                        value_str = value_str[:100] + "..."
                    print(f"  {field}: {value_str}")
                else:
                    print(f"  {field}: None")
        
    except Exception as e:
        print(f"\n✗ Error during test: {e}")
        import traceback
        traceback.print_exc()

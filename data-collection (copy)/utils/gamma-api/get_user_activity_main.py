"""
Get User Activity - Comprehensive user activity retrieval

Function to fetch Polymarket onchain activity for a user with full filtering options.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

import requests
from typing import Dict, List, Any, Optional, Union
from enum import Enum


class ActivityType(Enum):
    """Available activity types for filtering"""
    TRADE = "TRADE"
    SPLIT = "SPLIT"
    MERGE = "MERGE"
    REDEEM = "REDEEM"
    REWARD = "REWARD"
    CONVERSION = "CONVERSION"


class TradeSide(Enum):
    """Trade side options"""
    BUY = "BUY"
    SELL = "SELL"


class SortBy(Enum):
    """Sort criteria options"""
    TIMESTAMP = "TIMESTAMP"
    TOKENS = "TOKENS"
    CASH = "CASH"


class SortDirection(Enum):
    """Sort direction options"""
    ASC = "ASC"
    DESC = "DESC"


# Constants
DATA_API_BASE_URL = "https://data-api.polymarket.com"


def get_user_activity_data(
    user: str,
    limit: Optional[int] = None,
    offset: Optional[int] = None,
    market: Optional[Union[str, List[str]]] = None,
    activity_type: Optional[Union[ActivityType, List[ActivityType]]] = None,
    start: Optional[int] = None,
    end: Optional[int] = None,
    side: Optional[TradeSide] = None,
    sort_by: Optional[SortBy] = None,
    sort_direction: Optional[SortDirection] = None
) -> List[Dict[str, Any]]:
    """
    Fetch Polymarket onchain activity for a user
    
    Args:
        user: The address of the user (required)
        limit: Max number of activities to return (default 100, max 500)
        offset: Starting index for pagination (default 0)
        market: Market condition ID(s) - single string or list of strings
        activity_type: Activity type(s) to filter by - single or list of ActivityType
        start: Start timestamp in seconds
        end: End timestamp in seconds
        side: Trade side (BUY/SELL) - only applies to trades
        sort_by: Sort criteria (default TIMESTAMP)
        sort_direction: Sort direction (default DESC)
        
    Returns:
        List[Dict[str, Any]]: List of user activity records
        
    Example:
        >>> # Get recent activity for a user
        >>> activities = get_user_activity_data("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> print(f"Found {len(activities)} activities")
        
        >>> # Get only trades with pagination
        >>> trades = get_user_activity_data(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     activity_type=ActivityType.TRADE,
        ...     limit=50,
        ...     offset=0
        ... )
        
        >>> # Get activity for specific market
        >>> market_activity = get_user_activity_data(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     market="0x2c95c926e924f243aab41e96a90d22fcaf8cf273a678a07c49abb95fde489678"
        ... )
    """
    
    try:
        print(f"🔍 Getting user activity for: {user}")
        
        # Build query parameters
        params = {"user": user}
        
        # Add optional parameters
        if limit is not None:
            if limit > 500:
                print(f"⚠️ Limiting to max 500 (requested {limit})")
                limit = 500
            params["limit"] = limit
            
        if offset is not None:
            params["offset"] = offset
            
        # Handle market parameter (single or multiple)
        if market is not None:
            if isinstance(market, list):
                params["market"] = ",".join(market)
            else:
                params["market"] = market
                
        # Handle activity type parameter (single or multiple)
        if activity_type is not None:
            if isinstance(activity_type, list):
                params["type"] = ",".join([t.value for t in activity_type])
            else:
                params["type"] = activity_type.value
                
        if start is not None:
            params["start"] = start
            
        if end is not None:
            params["end"] = end
            
        if side is not None:
            params["side"] = side.value
            
        if sort_by is not None:
            params["sortBy"] = sort_by.value
            
        if sort_direction is not None:
            params["sortDirection"] = sort_direction.value
        
        # Make API request
        url = f"{DATA_API_BASE_URL}/activity"
        
        print(f"📡 Making request to: {url}")
        print(f"📋 Parameters: {params}")
        
        response = requests.get(url, params=params)
        response.raise_for_status()
        
        activities = response.json()
        
        print(f"✅ Retrieved {len(activities)} activity records")
        
        # Log summary statistics
        if activities:
            activity_types = {}
            for activity in activities:
                act_type = activity.get('type', 'UNKNOWN')
                activity_types[act_type] = activity_types.get(act_type, 0) + 1
            
            print(f"📊 Activity breakdown:")
            for act_type, count in activity_types.items():
                print(f"   {act_type}: {count}")
        
        return activities
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            print(f"❌ User not found or no activity: {user}")
            return []
        else:
            print(f"❌ HTTP error getting user activity: {e}")
            raise
    except requests.exceptions.RequestException as e:
        print(f"❌ Network error getting user activity: {e}")
        raise
    except Exception as e:
        print(f"❌ Unexpected error getting user activity: {e}")
        raise


# Convenience alias for backward compatibility
def get_user_activity(
    user: str,
    limit: Optional[int] = None,
    offset: Optional[int] = None,
    market: Optional[Union[str, List[str]]] = None,
    activity_type: Optional[Union[ActivityType, List[ActivityType]]] = None,
    start: Optional[int] = None,
    end: Optional[int] = None,
    side: Optional[TradeSide] = None,
    sort_by: Optional[SortBy] = None,
    sort_direction: Optional[SortDirection] = None
) -> List[Dict[str, Any]]:
    """
    Alias for get_user_activity_data for backward compatibility
    """
    return get_user_activity_data(
        user=user,
        limit=limit,
        offset=offset,
        market=market,
        activity_type=activity_type,
        start=start,
        end=end,
        side=side,
        sort_by=sort_by,
        sort_direction=sort_direction
    )


# Example usage and testing
if __name__ == "__main__":
    print("🔍 Get User Activity Data - Testing\n")
    
    # Example user address from the API documentation
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing user activity for: {test_user}")
        
        # Test with limited results
        print(f"\n1️⃣ Recent Activity (limit 5):")
        recent_activities = get_user_activity_data(test_user, limit=5)
        
        if recent_activities:
            print(f"✅ Found {len(recent_activities)} activities")
            
            # Show first activity details
            first_activity = recent_activities[0]
            print(f"   📊 Latest Activity:")
            print(f"      Type: {first_activity.get('type', 'N/A')}")
            print(f"      Timestamp: {first_activity.get('timestamp', 'N/A')}")
            print(f"      Market: {first_activity.get('title', 'N/A')}")
            if first_activity.get('usdcSize'):
                print(f"      Size: ${first_activity.get('usdcSize', 0)}")
        else:
            print("   ⚠️ No recent activities found")
        
        # Test filtering by activity type
        print(f"\n2️⃣ Trades Only (limit 3):")
        trades = get_user_activity_data(
            test_user, 
            activity_type=ActivityType.TRADE, 
            limit=3
        )
        
        if trades:
            print(f"✅ Found {len(trades)} trades")
            for i, trade in enumerate(trades, 1):
                side = trade.get('side', 'N/A')
                size = trade.get('usdcSize', 0)
                title = trade.get('title', 'N/A')[:50] + "..." if len(trade.get('title', '')) > 50 else trade.get('title', 'N/A')
                print(f"   {i}. {side} ${size} - {title}")
        else:
            print("   ⚠️ No trades found")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Note:")
        print(f"   • Make sure the user address is valid")
        print(f"   • User might not have any recent activity")
        print(f"   • Check network connection to Data API")
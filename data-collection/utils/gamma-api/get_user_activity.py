"""
Get User Activity - Polymarket onchain activity retrieval

Function to fetch Polymarket onchain activity for a user, including trades, 
splits, merges, redeems, rewards, and conversions.
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
        >>> activities = get_user_activity("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> print(f"Found {len(activities)} activities")
        
        >>> # Get only trades with pagination
        >>> trades = get_user_activity(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     activity_type=ActivityType.TRADE,
        ...     limit=50,
        ...     offset=0
        ... )
        
        >>> # Get activity for specific market
        >>> market_activity = get_user_activity(
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
        base_url = "https://data-api.polymarket.com"
        url = f"{base_url}/activity"
        
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


def get_user_trades_only(
    user: str,
    limit: Optional[int] = None,
    side: Optional[TradeSide] = None
) -> List[Dict[str, Any]]:
    """
    Convenience function to get only trades for a user
    
    Args:
        user: User address
        limit: Max number of trades to return
        side: Trade side (BUY/SELL)
        
    Returns:
        List of trade activities
    """
    return get_user_activity(
        user=user,
        activity_type=ActivityType.TRADE,
        limit=limit,
        side=side
    )


def get_recent_user_activity(
    user: str,
    hours: int = 24
) -> List[Dict[str, Any]]:
    """
    Get user activity from the last N hours
    
    Args:
        user: User address
        hours: Number of hours to look back (default 24)
        
    Returns:
        List of recent activities
    """
    import time
    
    end_time = int(time.time())
    start_time = end_time - (hours * 3600)  # Convert hours to seconds
    
    return get_user_activity(
        user=user,
        start=start_time,
        end=end_time
    )


def analyze_user_activity_summary(activities: List[Dict[str, Any]]) -> Dict[str, Any]:
    """
    Analyze user activity data and provide summary statistics
    
    Args:
        activities: List of activity records from get_user_activity
        
    Returns:
        Dict with summary statistics
    """
    if not activities:
        return {"total_activities": 0}
    
    # Count by type
    type_counts = {}
    total_volume = 0
    total_trades = 0
    buy_volume = 0
    sell_volume = 0
    
    for activity in activities:
        act_type = activity.get('type', 'UNKNOWN')
        type_counts[act_type] = type_counts.get(act_type, 0) + 1
        
        # Calculate volumes for trades
        if act_type == 'TRADE':
            total_trades += 1
            usdc_size = activity.get('usdcSize', 0)
            if usdc_size:
                total_volume += usdc_size
                
                side = activity.get('side', '')
                if side == 'BUY':
                    buy_volume += usdc_size
                elif side == 'SELL':
                    sell_volume += usdc_size
    
    # Calculate average trade size
    avg_trade_size = total_volume / total_trades if total_trades > 0 else 0
    
    # Get time range
    timestamps = [act.get('timestamp', 0) for act in activities if act.get('timestamp')]
    time_range = {
        'earliest': min(timestamps) if timestamps else 0,
        'latest': max(timestamps) if timestamps else 0
    }
    
    return {
        'total_activities': len(activities),
        'activity_breakdown': type_counts,
        'trade_summary': {
            'total_trades': total_trades,
            'total_volume_usdc': total_volume,
            'buy_volume_usdc': buy_volume,
            'sell_volume_usdc': sell_volume,
            'average_trade_size_usdc': round(avg_trade_size, 2)
        },
        'time_range': time_range
    }


# Example usage and testing
if __name__ == "__main__":
    print("🔍 Get User Activity - Testing\n")
    
    # Example user address from the API documentation
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing user activity for: {test_user}")
        
        # Test 1: Get recent activity (limited)
        print(f"\n1️⃣ Recent Activity (limit 5):")
        recent_activities = get_user_activity(test_user, limit=5)
        
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
        
        # Test 2: Get only trades
        print(f"\n2️⃣ Trades Only (limit 3):")
        trades = get_user_trades_only(test_user, limit=3)
        
        if trades:
            print(f"✅ Found {len(trades)} trades")
            for i, trade in enumerate(trades[:3], 1):
                print(f"   {i}. {trade.get('side', 'N/A')} ${trade.get('usdcSize', 0)} - {trade.get('title', 'N/A')}")
        else:
            print("   ⚠️ No trades found")
        
        # Test 3: Activity analysis
        if recent_activities:
            print(f"\n3️⃣ Activity Analysis:")
            analysis = analyze_user_activity_summary(recent_activities)
            
            print(f"   📈 Total Activities: {analysis['total_activities']}")
            print(f"   📊 Activity Types:")
            for act_type, count in analysis['activity_breakdown'].items():
                print(f"      {act_type}: {count}")
                
            trade_summary = analysis['trade_summary']
            if trade_summary['total_trades'] > 0:
                print(f"   💰 Trade Summary:")
                print(f"      Total Trades: {trade_summary['total_trades']}")
                print(f"      Total Volume: ${trade_summary['total_volume_usdc']}")
                print(f"      Average Size: ${trade_summary['average_trade_size_usdc']}")
        
        # Test 4: Recent activity (last 24 hours)
        print(f"\n4️⃣ Last 24 Hours:")
        recent_24h = get_recent_user_activity(test_user, hours=24)
        print(f"   📅 Activities in last 24h: {len(recent_24h)}")
        
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Note:")
        print(f"   • Make sure the user address is valid")
        print(f"   • User might not have any recent activity")
        print(f"   • Check network connection to Gamma API")
"""
Get Recent User Activity - Time-based activity retrieval

Function to get user activity from the last N hours.
"""

import os
import sys
import time

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, List, Any

# Import the main activity function
from get_user_activity_main import get_user_activity_data


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
        
    Example:
        >>> # Get activity from last 24 hours
        >>> recent = get_recent_user_activity("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> print(f"Found {len(recent)} activities in last 24h")
        
        >>> # Get activity from last 7 days (168 hours)
        >>> week_activity = get_recent_user_activity(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     hours=168
        ... )
    """
    print(f"📅 Getting activity from last {hours} hours for user: {user}")
    
    end_time = int(time.time())
    start_time = end_time - (hours * 3600)  # Convert hours to seconds
    
    print(f"   ⏰ Time range: {start_time} to {end_time} (timestamps)")
    
    return get_user_activity_data(
        user=user,
        start=start_time,
        end=end_time
    )


def get_user_activity_today(user: str) -> List[Dict[str, Any]]:
    """
    Get user activity from today only
    
    Args:
        user: User address
        
    Returns:
        List of today's activities
    """
    return get_recent_user_activity(user, hours=24)


def get_user_activity_this_week(user: str) -> List[Dict[str, Any]]:
    """
    Get user activity from the last 7 days
    
    Args:
        user: User address
        
    Returns:
        List of this week's activities
    """
    return get_recent_user_activity(user, hours=168)  # 7 * 24 hours


def get_user_activity_this_month(user: str) -> List[Dict[str, Any]]:
    """
    Get user activity from the last 30 days
    
    Args:
        user: User address
        
    Returns:
        List of this month's activities
    """
    return get_recent_user_activity(user, hours=720)  # 30 * 24 hours


# Example usage and testing
if __name__ == "__main__":
    print("📅 Get Recent User Activity - Testing\n")
    
    # Example user address
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing recent activity for: {test_user}")
        
        # Test 1: Last 24 hours
        print(f"\n1️⃣ Last 24 Hours:")
        recent_24h = get_recent_user_activity(test_user, hours=24)
        
        if recent_24h:
            print(f"✅ Found {len(recent_24h)} activities in last 24h")
            
            # Group by activity type
            activity_counts = {}
            for activity in recent_24h:
                act_type = activity.get('type', 'UNKNOWN')
                activity_counts[act_type] = activity_counts.get(act_type, 0) + 1
            
            print(f"   📊 Activity breakdown:")
            for act_type, count in activity_counts.items():
                print(f"      {act_type}: {count}")
        else:
            print("   ⚠️ No activity in last 24 hours")
        
        # Test 2: Today only (convenience function)
        print(f"\n2️⃣ Today's Activity:")
        today_activity = get_user_activity_today(test_user)
        print(f"   📅 Today's activities: {len(today_activity)}")
        
        # Test 3: This week (convenience function)
        print(f"\n3️⃣ This Week's Activity:")
        week_activity = get_user_activity_this_week(test_user)
        print(f"   📅 This week's activities: {len(week_activity)}")
        
        # Test 4: Custom time range (last 6 hours)
        print(f"\n4️⃣ Last 6 Hours:")
        recent_6h = get_recent_user_activity(test_user, hours=6)
        print(f"   ⏰ Activities in last 6h: {len(recent_6h)}")
        
        # Show most recent activity if available
        if recent_24h:
            latest = recent_24h[0]  # Should be sorted by timestamp desc
            print(f"\n📊 Most Recent Activity:")
            print(f"   Type: {latest.get('type', 'N/A')}")
            print(f"   Timestamp: {latest.get('timestamp', 'N/A')}")
            if latest.get('type') == 'TRADE':
                print(f"   Side: {latest.get('side', 'N/A')}")
                print(f"   Size: ${latest.get('usdcSize', 0)}")
            print(f"   Market: {latest.get('title', 'N/A')}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Note:")
        print(f"   • Make sure the user address is valid")
        print(f"   • User might not have recent activity")
        print(f"   • Check network connection to Data API")
"""
Get User Trades Only - Convenience function for trades

Function to get only trade activities for a user with optional filtering.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, List, Any, Optional
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


# Import the main activity function
from get_user_activity_main import get_user_activity_data


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
        
    Example:
        >>> # Get all recent trades
        >>> trades = get_user_trades_only("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> print(f"Found {len(trades)} trades")
        
        >>> # Get only buy trades
        >>> buy_trades = get_user_trades_only(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     side=TradeSide.BUY,
        ...     limit=10
        ... )
    """
    print(f"🔍 Getting trades only for user: {user}")
    
    return get_user_activity_data(
        user=user,
        activity_type=ActivityType.TRADE,
        limit=limit,
        side=side
    )


# Example usage and testing
if __name__ == "__main__":
    print("💰 Get User Trades Only - Testing\n")
    
    # Example user address
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing user trades for: {test_user}")
        
        # Test 1: Get recent trades (limited)
        print(f"\n1️⃣ Recent Trades (limit 5):")
        trades = get_user_trades_only(test_user, limit=5)
        
        if trades:
            print(f"✅ Found {len(trades)} trades")
            
            for i, trade in enumerate(trades, 1):
                side = trade.get('side', 'N/A')
                size = trade.get('usdcSize', 0)
                price = trade.get('price', 0)
                title = trade.get('title', 'N/A')
                
                # Truncate long titles
                if len(title) > 40:
                    title = title[:37] + "..."
                
                print(f"   {i}. {side} {size} tokens @ ${price} - {title}")
        else:
            print("   ⚠️ No trades found")
        
        # Test 2: Get only buy trades
        print(f"\n2️⃣ Buy Trades Only (limit 3):")
        buy_trades = get_user_trades_only(test_user, side=TradeSide.BUY, limit=3)
        
        if buy_trades:
            print(f"✅ Found {len(buy_trades)} buy trades")
            
            total_buy_volume = sum(trade.get('usdcSize', 0) for trade in buy_trades)
            print(f"   💰 Total buy volume: ${total_buy_volume}")
        else:
            print("   ⚠️ No buy trades found")
        
        # Test 3: Get only sell trades
        print(f"\n3️⃣ Sell Trades Only (limit 3):")
        sell_trades = get_user_trades_only(test_user, side=TradeSide.SELL, limit=3)
        
        if sell_trades:
            print(f"✅ Found {len(sell_trades)} sell trades")
            
            total_sell_volume = sum(trade.get('usdcSize', 0) for trade in sell_trades)
            print(f"   💰 Total sell volume: ${total_sell_volume}")
        else:
            print("   ⚠️ No sell trades found")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Note:")
        print(f"   • Make sure the user address is valid")
        print(f"   • User might not have any trade activity")
        print(f"   • Check network connection to Data API")
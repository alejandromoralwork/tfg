"""
Analyze User Activity Summary - Activity data analysis

Function to analyze user activity data and provide summary statistics.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, List, Any


def analyze_user_activity_summary(activities: List[Dict[str, Any]]) -> Dict[str, Any]:
    """
    Analyze user activity data and provide summary statistics
    
    Args:
        activities: List of activity records from get_user_activity
        
    Returns:
        Dict with summary statistics
        
    Example:
        >>> from get_user_activity_main import get_user_activity_data
        >>> activities = get_user_activity_data("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> summary = analyze_user_activity_summary(activities)
        >>> print(f"Total activities: {summary['total_activities']}")
        >>> print(f"Total trade volume: ${summary['trade_summary']['total_volume_usdc']}")
    """
    
    print(f"📊 Analyzing {len(activities)} activity records...")
    
    if not activities:
        return {"total_activities": 0}
    
    # Count by type
    type_counts = {}
    total_volume = 0
    total_trades = 0
    buy_volume = 0
    sell_volume = 0
    buy_trades = 0
    sell_trades = 0
    
    # Market activity tracking
    markets_traded = set()
    market_volumes = {}
    
    for activity in activities:
        act_type = activity.get('type', 'UNKNOWN')
        type_counts[act_type] = type_counts.get(act_type, 0) + 1
        
        # Calculate volumes for trades
        if act_type == 'TRADE':
            total_trades += 1
            usdc_size = activity.get('usdcSize', 0)
            market_id = activity.get('conditionId', 'unknown')
            
            # Track markets
            markets_traded.add(market_id)
            
            if usdc_size:
                total_volume += usdc_size
                
                # Track market volumes
                if market_id not in market_volumes:
                    market_volumes[market_id] = {
                        'volume': 0,
                        'trades': 0,
                        'title': activity.get('title', 'Unknown Market')
                    }
                market_volumes[market_id]['volume'] += usdc_size
                market_volumes[market_id]['trades'] += 1
                
                side = activity.get('side', '')
                if side == 'BUY':
                    buy_volume += usdc_size
                    buy_trades += 1
                elif side == 'SELL':
                    sell_volume += usdc_size
                    sell_trades += 1
    
    # Calculate averages
    avg_trade_size = total_volume / total_trades if total_trades > 0 else 0
    avg_buy_size = buy_volume / buy_trades if buy_trades > 0 else 0
    avg_sell_size = sell_volume / sell_trades if sell_trades > 0 else 0
    
    # Get time range
    timestamps = [act.get('timestamp', 0) for act in activities if act.get('timestamp')]
    time_range = {
        'earliest': min(timestamps) if timestamps else 0,
        'latest': max(timestamps) if timestamps else 0,
        'span_hours': (max(timestamps) - min(timestamps)) / 3600 if len(timestamps) > 1 else 0
    }
    
    # Top markets by volume
    top_markets = sorted(
        market_volumes.items(),
        key=lambda x: x[1]['volume'],
        reverse=True
    )[:5]  # Top 5 markets
    
    summary = {
        'total_activities': len(activities),
        'activity_breakdown': type_counts,
        'trade_summary': {
            'total_trades': total_trades,
            'total_volume_usdc': round(total_volume, 2),
            'buy_volume_usdc': round(buy_volume, 2),
            'sell_volume_usdc': round(sell_volume, 2),
            'buy_trades': buy_trades,
            'sell_trades': sell_trades,
            'average_trade_size_usdc': round(avg_trade_size, 2),
            'average_buy_size_usdc': round(avg_buy_size, 2),
            'average_sell_size_usdc': round(avg_sell_size, 2),
            'markets_traded': len(markets_traded)
        },
        'time_range': time_range,
        'top_markets': [
            {
                'condition_id': market_id,
                'title': data['title'],
                'volume': round(data['volume'], 2),
                'trades': data['trades']
            }
            for market_id, data in top_markets
        ]
    }
    
    print(f"✅ Analysis complete!")
    print(f"   📈 Total Activities: {summary['total_activities']}")
    print(f"   💰 Total Trade Volume: ${summary['trade_summary']['total_volume_usdc']}")
    print(f"   📊 Markets Traded: {summary['trade_summary']['markets_traded']}")
    
    return summary


def get_activity_insights(activities: List[Dict[str, Any]]) -> Dict[str, str]:
    """
    Generate insights and observations from activity data
    
    Args:
        activities: List of activity records
        
    Returns:
        Dict with insight messages
    """
    summary = analyze_user_activity_summary(activities)
    insights = {}
    
    if summary['total_activities'] == 0:
        insights['overall'] = "No activity found for this user."
        return insights
    
    trade_summary = summary['trade_summary']
    
    # Trading behavior insights
    if trade_summary['total_trades'] > 0:
        buy_ratio = trade_summary['buy_trades'] / trade_summary['total_trades']
        
        if buy_ratio > 0.7:
            insights['trading_behavior'] = "Predominantly a buyer - prefers to take long positions"
        elif buy_ratio < 0.3:
            insights['trading_behavior'] = "Predominantly a seller - prefers to take short positions or realize profits"
        else:
            insights['trading_behavior'] = "Balanced trader - buys and sells relatively equally"
        
        # Volume insights
        avg_size = trade_summary['average_trade_size_usdc']
        if avg_size > 1000:
            insights['trade_size'] = "High-volume trader - makes large bets"
        elif avg_size > 100:
            insights['trade_size'] = "Medium-volume trader - moderate position sizes"
        else:
            insights['trade_size'] = "Small-volume trader - cautious position sizing"
        
        # Market diversity
        markets_count = trade_summary['markets_traded']
        if markets_count > 10:
            insights['diversification'] = "Highly diversified - trades many different markets"
        elif markets_count > 3:
            insights['diversification'] = "Moderately diversified - trades several markets"
        else:
            insights['diversification'] = "Focused trader - concentrates on few markets"
    
    # Activity level
    total_activities = summary['total_activities']
    if total_activities > 100:
        insights['activity_level'] = "Very active user - high engagement"
    elif total_activities > 20:
        insights['activity_level'] = "Moderately active user"
    else:
        insights['activity_level'] = "Casual user - limited activity"
    
    return insights


# Example usage and testing
if __name__ == "__main__":
    print("📊 Analyze User Activity Summary - Testing\n")
    
    # We'll need to import the main function for testing
    try:
        from get_user_activity_main import get_user_activity_data
    except ImportError:
        print("⚠️ Cannot import main activity function for testing")
        print("   Run this test after all files are created")
        sys.exit(0)
    
    # Example user address
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing activity analysis for: {test_user}")
        
        # Get activity data first
        print(f"\n1️⃣ Fetching activity data...")
        activities = get_user_activity_data(test_user, limit=50)
        
        if not activities:
            print("   ⚠️ No activities found - cannot test analysis")
            sys.exit(0)
        
        # Test analysis
        print(f"\n2️⃣ Analyzing activity data...")
        analysis = analyze_user_activity_summary(activities)
        
        print(f"\n📈 Analysis Results:")
        print(f"   Total Activities: {analysis['total_activities']}")
        
        print(f"\n   📊 Activity Breakdown:")
        for act_type, count in analysis['activity_breakdown'].items():
            print(f"      {act_type}: {count}")
        
        if analysis['trade_summary']['total_trades'] > 0:
            trade_sum = analysis['trade_summary']
            print(f"\n   💰 Trade Summary:")
            print(f"      Total Trades: {trade_sum['total_trades']}")
            print(f"      Total Volume: ${trade_sum['total_volume_usdc']}")
            print(f"      Buy/Sell Ratio: {trade_sum['buy_trades']}/{trade_sum['sell_trades']}")
            print(f"      Average Trade Size: ${trade_sum['average_trade_size_usdc']}")
            print(f"      Markets Traded: {trade_sum['markets_traded']}")
        
        # Test insights
        print(f"\n3️⃣ Generating insights...")
        insights = get_activity_insights(activities)
        
        print(f"\n🔍 User Insights:")
        for category, insight in insights.items():
            print(f"   {category.title()}: {insight}")
        
        # Show top markets if available
        if analysis['top_markets']:
            print(f"\n🏆 Top Markets by Volume:")
            for i, market in enumerate(analysis['top_markets'][:3], 1):
                title = market['title']
                if len(title) > 50:
                    title = title[:47] + "..."
                print(f"   {i}. ${market['volume']} ({market['trades']} trades) - {title}")
        
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Note:")
        print(f"   • Make sure get_user_activity_main.py is available")
        print(f"   • User might not have any activity to analyze")
"""
User Activity All - Complete user activity analysis suite

This module imports all user activity functions for convenient access.
Import this file to get access to all user activity functionality.

Available Functions:
- get_user_activity_data(): Comprehensive activity retrieval with filtering
- get_user_trades_only(): Get only trade activities  
- get_recent_user_activity(): Get activity from last N hours
- analyze_user_activity_summary(): Analyze activity data and generate statistics
- get_activity_insights(): Generate insights from activity patterns

Backward Compatibility:
- get_user_activity(): Alias for get_user_activity_data()
"""

# Import all activity functions
try:
    from get_user_activity_main import get_user_activity_data, get_user_activity, ActivityType, TradeSide, SortBy, SortDirection
    from get_user_trades_only import get_user_trades_only
    from get_recent_user_activity import get_recent_user_activity, get_user_activity_today, get_user_activity_this_week, get_user_activity_this_month
    from analyze_user_activity import analyze_user_activity_summary, get_activity_insights
    
    print("✅ All user activity functions imported successfully")
    
except ImportError as e:
    print(f"❌ Import error in activity_all.py: {e}")

# Version info
__version__ = "1.0.0"
__author__ = "Polymarket Trading System"

# All available functions
__all__ = [
    # Main functions
    'get_user_activity_data',
    'get_user_trades_only',
    'get_recent_user_activity', 
    'analyze_user_activity_summary',
    'get_activity_insights',
    
    # Time-based convenience functions
    'get_user_activity_today',
    'get_user_activity_this_week', 
    'get_user_activity_this_month',
    
    # Backward compatibility
    'get_user_activity',
    
    # Enums
    'ActivityType',
    'TradeSide',
    'SortBy',
    'SortDirection'
]


def print_available_functions():
    """Print all available user activity functions"""
    print("🔍 Available User Activity Functions:")
    print("=" * 50)
    
    print("\n🔧 Core Functions:")
    print("  • get_user_activity_data(user, ...) - Comprehensive activity retrieval")
    print("  • get_user_trades_only(user, side, limit) - Trade activities only")
    print("  • get_recent_user_activity(user, hours) - Recent activity by time")
    print("  • analyze_user_activity_summary(activities) - Statistical analysis")
    print("  • get_activity_insights(activities) - Generate insights")
    
    print("\n⏰ Time-Based Functions:")
    print("  • get_user_activity_today(user) - Today's activity")
    print("  • get_user_activity_this_week(user) - Last 7 days")
    print("  • get_user_activity_this_month(user) - Last 30 days")
    
    print("\n📊 Filtering Enums:")
    print("  • ActivityType: TRADE, SPLIT, MERGE, REDEEM, REWARD, CONVERSION")
    print("  • TradeSide: BUY, SELL")
    print("  • SortBy: TIMESTAMP, TOKENS, CASH") 
    print("  • SortDirection: ASC, DESC")
    
    print(f"\n📦 Version: {__version__}")


def demo_user_analysis(user_address: str):
    """
    Demonstrate comprehensive user analysis
    
    Args:
        user_address: Ethereum address to analyze
    """
    print(f"🔍 Comprehensive User Analysis: {user_address}")
    print("=" * 60)
    
    try:
        # Get recent activity
        print("1️⃣ Fetching recent activity...")
        activities = get_user_activity_data(user_address, limit=100)
        
        if not activities:
            print("   ⚠️ No activity found for this user")
            return
        
        # Analyze the data
        print("2️⃣ Analyzing activity patterns...")
        analysis = analyze_user_activity_summary(activities)
        
        # Generate insights
        print("3️⃣ Generating insights...")
        insights = get_activity_insights(activities)
        
        # Display results
        print(f"\n📊 Summary:")
        print(f"   Total Activities: {analysis['total_activities']}")
        print(f"   Total Trade Volume: ${analysis['trade_summary']['total_volume_usdc']}")
        print(f"   Markets Traded: {analysis['trade_summary']['markets_traded']}")
        
        print(f"\n🔍 Key Insights:")
        for category, insight in insights.items():
            print(f"   • {insight}")
        
        # Show recent trades
        recent_trades = get_user_trades_only(user_address, limit=5)
        if recent_trades:
            print(f"\n💰 Recent Trades:")
            for i, trade in enumerate(recent_trades[:3], 1):
                side = trade.get('side', 'N/A')
                size = trade.get('usdcSize', 0)
                title = trade.get('title', 'N/A')[:40] + "..." if len(trade.get('title', '')) > 40 else trade.get('title', 'N/A')
                print(f"   {i}. {side} ${size} - {title}")
        
    except Exception as e:
        print(f"❌ Analysis failed: {e}")


# Example usage
if __name__ == "__main__":
    print("🔍 User Activity Analysis Suite")
    print("=" * 50)
    
    # Show available functions
    print_available_functions()
    
    print("\n" + "="*60)
    print("📋 Example Usage:")
    print("="*60)
    
    print("""
# Import everything
from utils.gamma-api.activity_all import *

# Get comprehensive activity
user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
activities = get_user_activity_data(user, limit=100)

# Get only trades
trades = get_user_trades_only(user, side=TradeSide.BUY)

# Get recent activity
recent = get_recent_user_activity(user, hours=24)
today = get_user_activity_today(user)

# Analyze the data
analysis = analyze_user_activity_summary(activities)
insights = get_activity_insights(activities)

# Demo analysis
demo_user_analysis(user)
""")
    
    print("\n" + "="*60)
    print("🧪 Live Demo:")
    print("="*60)
    
    # Run a live demo if possible
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        demo_user_analysis(test_user)
    except Exception as e:
        print(f"❌ Live demo failed: {e}")
        print("💡 All functions are available for import and use!")
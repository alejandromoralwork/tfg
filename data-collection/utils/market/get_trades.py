"""
Get trade history from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import List, Dict, Any
from datetime import datetime
from dotenv import load_dotenv

from py_clob_client.client import ClobClient

# Load from config folder
load_dotenv("config/.env")


def get_trades(token_id: str) -> List[Dict[str, Any]]:
    """
    Get trade history for a specific token
    
    Args:
        token_id: The token ID to get trades for
        
    Returns:
        List of trade dictionaries
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"📈 Fetching trades for token: {token_id[:20]}...")
        trades = client.get_trades(token_id)
        
        print(f"✅ Retrieved {len(trades)} trades")
        return trades
        
    except Exception as e:
        print(f"❌ Error getting trades: {e}")
        raise


def get_market_trades_events(condition_id: str) -> List[Dict[str, Any]]:
    """
    Get market trade events for a condition
    
    Args:
        condition_id: The condition ID to get trade events for
        
    Returns:
        List of trade event dictionaries
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"📈 Fetching market trade events for condition: {condition_id}")
        events = client.get_market_trades_events(condition_id)
        
        print(f"✅ Retrieved {len(events)} trade events")
        return events
        
    except Exception as e:
        print(f"❌ Error getting market trade events: {e}")
        raise


def analyze_trade_history(token_id: str) -> Dict[str, Any]:
    """
    Analyze trade history with statistics
    
    Args:
        token_id: The token ID to analyze
        
    Returns:
        Dict with trade analysis
    """
    try:
        trades = get_trades(token_id)
        
        if not trades:
            return {
                'token_id': token_id,
                'total_trades': 0,
                'error': 'No trades found'
            }
        
        # Calculate statistics
        total_trades = len(trades)
        total_volume = 0
        total_value = 0
        prices = []
        sizes = []
        timestamps = []
        
        for trade in trades:
            price = float(trade.get('price', 0))
            size = float(trade.get('size', 0))
            timestamp = trade.get('timestamp', 0)
            
            prices.append(price)
            sizes.append(size)
            timestamps.append(timestamp)
            
            total_volume += size
            total_value += price * size
        
        # Calculate price statistics
        if prices:
            min_price = min(prices)
            max_price = max(prices)
            avg_price = sum(prices) / len(prices)
            latest_price = prices[-1] if prices else 0
            
            # Price change
            if len(prices) > 1:
                first_price = prices[0]
                price_change = latest_price - first_price
                price_change_pct = (price_change / first_price * 100) if first_price > 0 else 0
            else:
                price_change = 0
                price_change_pct = 0
        else:
            min_price = max_price = avg_price = latest_price = 0
            price_change = price_change_pct = 0
        
        # Calculate size statistics
        if sizes:
            avg_size = sum(sizes) / len(sizes)
            max_size = max(sizes)
            min_size = min(sizes)
        else:
            avg_size = max_size = min_size = 0
        
        # Calculate time range
        if timestamps:
            timestamps = [t for t in timestamps if t > 0]  # Filter valid timestamps
            if timestamps:
                min_timestamp = min(timestamps)
                max_timestamp = max(timestamps)
                time_range_hours = (max_timestamp - min_timestamp) / 3600 if len(timestamps) > 1 else 0
            else:
                min_timestamp = max_timestamp = time_range_hours = 0
        else:
            min_timestamp = max_timestamp = time_range_hours = 0
        
        analysis = {
            'token_id': token_id,
            'total_trades': total_trades,
            'total_volume': round(total_volume, 2),
            'total_value': round(total_value, 2),
            'price_stats': {
                'latest_price': round(latest_price, 4),
                'min_price': round(min_price, 4),
                'max_price': round(max_price, 4),
                'avg_price': round(avg_price, 4),
                'price_change': round(price_change, 4),
                'price_change_pct': round(price_change_pct, 2)
            },
            'volume_stats': {
                'avg_trade_size': round(avg_size, 2),
                'max_trade_size': round(max_size, 2),
                'min_trade_size': round(min_size, 2)
            },
            'time_stats': {
                'time_range_hours': round(time_range_hours, 2),
                'trades_per_hour': round(total_trades / time_range_hours, 2) if time_range_hours > 0 else 0
            },
            'recent_trades': trades[-5:] if len(trades) >= 5 else trades  # Last 5 trades
        }
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing trade history: {e}")
        raise


def get_recent_trades(token_id: str, limit: int = 10) -> List[Dict[str, Any]]:
    """
    Get recent trades for a token
    
    Args:
        token_id: The token ID
        limit: Number of recent trades to return
        
    Returns:
        List of recent trades
    """
    try:
        trades = get_trades(token_id)
        
        # Sort by timestamp (most recent first) and take limit
        sorted_trades = sorted(trades, key=lambda x: x.get('timestamp', 0), reverse=True)
        recent_trades = sorted_trades[:limit]
        
        # Format timestamps for readability
        for trade in recent_trades:
            timestamp = trade.get('timestamp', 0)
            if timestamp > 0:
                if timestamp > 10**12:  # Milliseconds
                    timestamp = timestamp / 1000
                trade['formatted_time'] = datetime.fromtimestamp(timestamp).strftime('%Y-%m-%d %H:%M:%S')
            else:
                trade['formatted_time'] = 'Unknown'
        
        print(f"📈 Retrieved {len(recent_trades)} recent trades")
        return recent_trades
        
    except Exception as e:
        print(f"❌ Error getting recent trades: {e}")
        raise


def main():
    """Example usage"""
    print("📈 Get Trades Utility")
    print("=" * 40)
    
    # Example token ID
    token_id = "71321045679252212594626385532706912750332728571942532289631379312455583992563"
    
    try:
        # Trade analysis
        print("\n1️⃣ Trade history analysis:")
        analysis = analyze_trade_history(token_id)
        
        if analysis.get('error'):
            print(f"   {analysis['error']}")
        else:
            print(f"   Total Trades: {analysis['total_trades']}")
            print(f"   Total Volume: {analysis['total_volume']} tokens")
            print(f"   Total Value: ${analysis['total_value']}")
            
            price_stats = analysis['price_stats']
            print(f"   Latest Price: ${price_stats['latest_price']}")
            print(f"   Price Range: ${price_stats['min_price']} - ${price_stats['max_price']}")
            print(f"   Price Change: {price_stats['price_change_pct']}%")
            
            volume_stats = analysis['volume_stats']
            print(f"   Avg Trade Size: {volume_stats['avg_trade_size']} tokens")
            
            time_stats = analysis['time_stats']
            print(f"   Time Range: {time_stats['time_range_hours']} hours")
            print(f"   Trades/Hour: {time_stats['trades_per_hour']}")
        
        # Recent trades
        print("\n2️⃣ Recent trades:")
        recent_trades = get_recent_trades(token_id, limit=5)
        
        if recent_trades:
            for i, trade in enumerate(recent_trades, 1):
                price = trade.get('price', 0)
                size = trade.get('size', 0)
                side = trade.get('side', 'Unknown')
                time_str = trade.get('formatted_time', 'Unknown')
                print(f"   {i}. {side} ${price} x {size} at {time_str}")
        else:
            print("   No recent trades found")
        
        # Raw trades sample
        print("\n3️⃣ Raw trades sample:")
        all_trades = get_trades(token_id)
        if all_trades:
            sample_trade = all_trades[0]
            print(f"   Sample trade structure: {list(sample_trade.keys())}")
            print(f"   Sample trade: {sample_trade}")
        else:
            print("   No trades available")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
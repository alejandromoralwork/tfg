"""
Analyze Order Book - Comprehensive order book analysis

Function to perform detailed analysis of order book data including pricing,
spreads, depth analysis, and top orders.
"""

import os
import sys

# Add the project root to the Python path  
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, Any, Optional
from utils.market.orderbook_base import OrderBookAnalyzer


def analyze_order_book_data(token_id: str, use_auth: bool = False) -> Dict[str, Any]:
    """
    Perform comprehensive analysis of order book data
    
    Args:
        token_id: The token ID to analyze
        use_auth: Whether to use authenticated client
        
    Returns:
        Dict[str, Any]: Complete analysis including prices, spreads, depth, and top orders
        
    Example:
        >>> analysis = analyze_order_book_data("87073854310124528759506128171096701607709284910112533007376905018319069357459")
        >>> print(f"Best bid: {analysis['best_bid']}, Best ask: {analysis['best_ask']}")
        >>> print(f"Spread: {analysis['spread']} ({analysis['spread_percentage']}%)")
    """
    try:
        print(f"📊 Analyzing order book for token: {token_id}")
        
        analyzer = OrderBookAnalyzer(use_auth=use_auth)
        
        # Get the order book
        response = analyzer.client.get_order_book(token_id)
        
        if not response:
            raise ValueError(f"No order book data returned for token {token_id}")
        
        # Handle different response types (dict vs object)
        if hasattr(response, 'get'):
            # Response is dict-like
            orderbook_hash = response.get('hash', 'N/A')
            bids = response.get('bids', [])
            asks = response.get('asks', [])
            market = response.get('market', 'N/A')
            asset_id = response.get('asset_id', 'N/A')
        else:
            # Response is object-like, use attribute access
            orderbook_hash = getattr(response, 'hash', 'N/A')
            bids = getattr(response, 'bids', [])
            asks = getattr(response, 'asks', [])
            market = getattr(response, 'market', 'N/A')
            asset_id = getattr(response, 'asset_id', 'N/A')
        
        # Create orderbook dict for reference
        orderbook_dict = {
            'bids': bids,
            'asks': asks,
            'hash': orderbook_hash,
            'market': market,
            'asset_id': asset_id
        }
        
        # Process bids and asks into more detailed format
        all_bids = []
        all_asks = []
        
        # Process bids (buy orders)
        try:
            for bid in bids:
                # Handle both dict and object bid entries
                if hasattr(bid, 'get'):
                    price = float(bid['price'])
                    size = float(bid['size'])
                else:
                    price = float(getattr(bid, 'price', 0))
                    size = float(getattr(bid, 'size', 0))
                
                all_bids.append({
                    'price': price,
                    'size': size,
                    'total': price * size,
                    'side': 'bid'
                })
        except Exception as e:
            print(f"⚠️ Error processing bids: {e}")
        
        # Process asks (sell orders)  
        try:
            for ask in asks:
                # Handle both dict and object ask entries
                if hasattr(ask, 'get'):
                    price = float(ask['price'])
                    size = float(ask['size'])
                else:
                    price = float(getattr(ask, 'price', 0))
                    size = float(getattr(ask, 'size', 0))
                
                all_asks.append({
                    'price': price,
                    'size': size,
                    'total': price * size,
                    'side': 'ask'
                })
        except Exception as e:
            print(f"⚠️ Error processing asks: {e}")
        
        # Sort orders properly
        # Bids: highest price first (best bid is highest price)
        # Asks: lowest price first (best ask is lowest price)
        top_5_bids = sorted(all_bids, key=lambda x: x['price'], reverse=True)[:5]
        top_5_asks = sorted(all_asks, key=lambda x: x['price'], reverse=False)[:5]
        
        # Calculate best bid/ask from sorted data
        best_bid = top_5_bids[0]['price'] if top_5_bids else 0
        best_ask = top_5_asks[0]['price'] if top_5_asks else 0
        
        # Calculate spread and midpoint with correct prices
        spread = best_ask - best_bid if best_bid and best_ask else 0
        spread_pct = (spread / best_ask * 100) if best_ask else 0
        midpoint = (best_bid + best_ask) / 2 if best_bid and best_ask else 0
        
        # Calculate depth using top 5 sorted orders
        bid_depth = sum(bid['size'] for bid in top_5_bids)
        ask_depth = sum(ask['size'] for ask in top_5_asks)
        total_depth = bid_depth + ask_depth
        
        analysis = {
            # Basic Info
            'token_id': token_id,
            'market': market,
            'asset_id': asset_id,
            'orderbook_hash': orderbook_hash,
            
            # Price Information
            'best_bid': best_bid,
            'best_ask': best_ask,
            'midpoint_price': round(midpoint, 6),
            'spread': round(spread, 6),
            'spread_percentage': round(spread_pct, 4),
            
            # Depth Information
            'bid_depth_top5': round(bid_depth, 2),
            'ask_depth_top5': round(ask_depth, 2),
            'total_depth_top5': round(total_depth, 2),
            'total_orders': len(bids) + len(asks),
            'bid_orders': len(bids),
            'ask_orders': len(asks),
            
            # Processed Data
            'top_5_bids': top_5_bids,
            'top_5_asks': top_5_asks,
            
            # Full order book (for advanced analysis)
            'full_orderbook': orderbook_dict
        }
        
        print(f"✅ Order book analysis complete")
        print(f"   💰 Best Bid: {best_bid} | Best Ask: {best_ask}")
        print(f"   📏 Spread: {spread} ({spread_pct:.2f}%)")
        print(f"   📊 Total Orders: {len(bids) + len(asks)}")
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing order book for {token_id}: {e}")
        raise


# Convenience function for backward compatibility
def analyze_market(token_id: str) -> Dict[str, Any]:
    """
    Get comprehensive market analysis
    
    Args:
        token_id: Market/token identifier
        
    Returns:
        Dict with complete market analysis
    """
    return analyze_order_book_data(token_id, use_auth=False)


# Example usage and testing
if __name__ == "__main__":
    import json
    
    print("📊 Analyze Order Book Data - Testing\n")
    
    # Example token ID
    test_token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    
    try:
        print(f"🔍 Analyzing market: {test_token_id}")
        
        # Test the function
        analysis = analyze_order_book_data(test_token_id)
        
        print(f"✅ Analysis complete!")
        print(f"   💰 Best Bid: {analysis['best_bid']}")
        print(f"   💰 Best Ask: {analysis['best_ask']}")
        print(f"   💰 Midpoint: {analysis['midpoint_price']}")
        print(f"   📏 Spread: {analysis['spread']} ({analysis['spread_percentage']}%)")
        print(f"   📊 Total Depth: {analysis['total_depth_top5']} USDC")
        print(f"   📋 Total Orders: {analysis['total_orders']} ({analysis['bid_orders']} bids, {analysis['ask_orders']} asks)")
        
        # Show top 3 bids and asks
        print(f"\n   🟢 Top 3 Bids:")
        for i, bid in enumerate(analysis['top_5_bids'][:3], 1):
            print(f"      {i}. ${bid['price']} × {bid['size']} = ${bid['total']:.2f}")
        
        print(f"\n   🔴 Top 3 Asks:")
        for i, ask in enumerate(analysis['top_5_asks'][:3], 1):
            print(f"      {i}. ${ask['price']} × {ask['size']} = ${ask['total']:.2f}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print("\n💡 Make sure to:")
        print("   1. Have valid token ID")
        print("   2. Check network connection")
        print("   3. Verify API endpoint is accessible")
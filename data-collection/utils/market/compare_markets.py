"""
Compare Multiple Markets - Multi-market price comparison

Function to compare order books and pricing across multiple markets simultaneously.
"""

import os
import sys

# Add the project root to the Python path  
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, Any, List
from utils.market.get_latest_prices import get_latest_prices_data


def compare_multiple_markets_data(token_ids: List[str], use_auth: bool = False) -> List[Dict[str, Any]]:
    """
    Compare order books across multiple markets
    
    Args:
        token_ids: List of token/market IDs to compare
        use_auth: Whether to use authenticated client
        
    Returns:
        List of price comparisons for each market
        
    Example:
        >>> markets = ["token1", "token2", "token3"]
        >>> comparisons = compare_multiple_markets_data(markets)
        >>> for comp in comparisons:
        ...     print(f"{comp['token_id']}: ${comp['midpoint_price']}")
    """
    comparisons = []
    
    print(f"🔍 Comparing {len(token_ids)} markets...")
    
    for i, token_id in enumerate(token_ids, 1):
        try:
            print(f"   📊 ({i}/{len(token_ids)}) Processing {token_id}")
            
            prices = get_latest_prices_data(token_id, use_auth)
            comparisons.append(prices)
            
            print(f"      ✅ Success - Mid: ${prices['midpoint_price']}, Spread: {prices['spread_percentage']}%")
            
        except Exception as e:
            print(f"      ❌ Failed: {e}")
            # Add failed entry
            comparisons.append({
                'token_id': token_id,
                'error': str(e),
                'best_bid': None,
                'best_ask': None,
                'midpoint_price': None,
                'spread': None,
                'spread_percentage': None
            })
    
    print(f"✅ Market comparison complete: {len(comparisons)} results")
    return comparisons


def get_best_prices_across_markets(token_ids: List[str], use_auth: bool = False) -> Dict[str, Any]:
    """
    Find the best bid and ask across multiple markets
    
    Args:
        token_ids: List of token/market IDs to compare
        use_auth: Whether to use authenticated client
        
    Returns:
        Dict with best bid/ask across all markets
        
    Example:
        >>> markets = ["token1", "token2", "token3"]
        >>> best_prices = get_best_prices_across_markets(markets)
        >>> print(f"Best bid across all markets: ${best_prices['best_bid']}")
    """
    comparisons = compare_multiple_markets_data(token_ids, use_auth)
    
    # Filter out failed comparisons
    successful_comparisons = [comp for comp in comparisons if 'error' not in comp]
    
    if not successful_comparisons:
        raise ValueError("No successful market comparisons found")
    
    # Find best bid (highest) and best ask (lowest)
    best_bid = max(comp['best_bid'] for comp in successful_comparisons)
    best_ask = min(comp['best_ask'] for comp in successful_comparisons)
    best_midpoint = (best_bid + best_ask) / 2
    
    # Find which markets have the best prices
    best_bid_market = next(comp for comp in successful_comparisons if comp['best_bid'] == best_bid)
    best_ask_market = next(comp for comp in successful_comparisons if comp['best_ask'] == best_ask)
    
    return {
        'best_bid': best_bid,
        'best_ask': best_ask,
        'best_midpoint': round(best_midpoint, 6),
        'best_bid_market': best_bid_market['token_id'],
        'best_ask_market': best_ask_market['token_id'],
        'total_markets_compared': len(successful_comparisons),
        'failed_markets': len(comparisons) - len(successful_comparisons),
        'all_comparisons': comparisons
    }


# Example usage and testing
if __name__ == "__main__":
    print("🔍 Compare Multiple Markets - Testing\n")
    
    # Example token IDs (use multiple if available)
    test_token_ids = [
        "87073854310124528759506128171096701607709284910112533007376905018319069357459",
        # Add more token IDs here for testing multiple markets
    ]
    
    try:
        print(f"🔍 Comparing {len(test_token_ids)} markets")
        
        # Test multi-market comparison
        comparisons = compare_multiple_markets_data(test_token_ids)
        
        print(f"\n📊 Comparison Results:")
        for i, comp in enumerate(comparisons, 1):
            if 'error' not in comp:
                print(f"   {i}. {comp['token_id']}")
                print(f"      💰 Bid: ${comp['best_bid']} | Ask: ${comp['best_ask']}")
                print(f"      💰 Midpoint: ${comp['midpoint_price']}")
                print(f"      📏 Spread: {comp['spread_percentage']}%")
            else:
                print(f"   {i}. {comp['token_id']} - Error: {comp['error']}")
        
        # Test best prices if we have multiple markets
        if len(test_token_ids) > 1:
            print(f"\n🏆 Finding best prices across all markets...")
            best_prices = get_best_prices_across_markets(test_token_ids)
            
            print(f"   🥇 Best Bid: ${best_prices['best_bid']} (Market: {best_prices['best_bid_market']})")
            print(f"   🥇 Best Ask: ${best_prices['best_ask']} (Market: {best_prices['best_ask_market']})")
            print(f"   💰 Cross-Market Midpoint: ${best_prices['best_midpoint']}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print("\n💡 Make sure to:")
        print("   1. Have valid token IDs")
        print("   2. Check network connection")
        print("   3. Add multiple token IDs for comparison testing")
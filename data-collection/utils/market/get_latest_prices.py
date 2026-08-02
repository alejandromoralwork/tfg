"""
Get Latest Prices - Simple price information retrieval

Function to get just the essential pricing information from order book data.
"""

import os
import sys

# Add the project root to the Python path  
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict
from utils.market.analyze_order_book import analyze_order_book_data


def get_latest_prices_data(token_id: str, use_auth: bool = False) -> Dict[str, float]:
    """
    Get just the latest price information (simplified)
    
    Args:
        token_id: The token/market ID
        use_auth: Whether to use authenticated client
        
    Returns:
        Dict with current price information
        
    Example:
        >>> prices = get_latest_prices_data("87073854310124528759506128171096701607709284910112533007376905018319069357459")
        >>> print(f"Midpoint price: ${prices['midpoint_price']}")
        >>> print(f"Spread: {prices['spread_percentage']}%")
    """
    try:
        print(f"💰 Getting latest prices for token: {token_id}")
        
        # Use the comprehensive analysis function but return simplified data
        analysis = analyze_order_book_data(token_id, use_auth)
        
        prices = {
            'token_id': token_id,
            'best_bid': analysis['best_bid'],
            'best_ask': analysis['best_ask'],
            'midpoint_price': analysis['midpoint_price'],
            'spread': analysis['spread'],
            'spread_percentage': analysis['spread_percentage']
        }
        
        print(f"✅ Latest prices retrieved")
        print(f"   💰 Bid: ${prices['best_bid']} | Ask: ${prices['best_ask']}")
        print(f"   💰 Midpoint: ${prices['midpoint_price']}")
        
        return prices
        
    except Exception as e:
        print(f"❌ Error getting latest prices for {token_id}: {e}")
        raise


# Convenience function for backward compatibility
def get_latest_prices(token_id: str) -> Dict[str, float]:
    """
    Simple function to get latest prices for a token ID
    
    Args:
        token_id: Market/token identifier
        
    Returns:
        Dict with current pricing information
    """
    return get_latest_prices_data(token_id, use_auth=False)


# Example usage and testing
if __name__ == "__main__":
    print("💰 Get Latest Prices Data - Testing\n")
    
    # Example token ID
    test_token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    
    try:
        print(f"🔍 Getting latest prices for: {test_token_id}")
        
        # Test the function
        prices = get_latest_prices_data(test_token_id)
        
        print(f"✅ Success!")
        print(f"   💰 Best Bid: ${prices['best_bid']}")
        print(f"   💰 Best Ask: ${prices['best_ask']}")
        print(f"   💰 Midpoint: ${prices['midpoint_price']}")
        print(f"   📏 Spread: ${prices['spread']} ({prices['spread_percentage']}%)")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print("\n💡 Make sure to:")
        print("   1. Have valid token ID")
        print("   2. Check network connection")
        print("   3. Verify API endpoint is accessible")
"""
Get Order Book - Raw order book data retrieval

Simple function to retrieve raw order book data for a specific token/market.
"""

import os
import sys

# Add the project root to the Python path  
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Dict, Any, List
from utils.market.orderbook_base import OrderBookAnalyzer, OrderBookSummary


def get_order_book_data(token_id: str, use_auth: bool = False) -> OrderBookSummary:
    """
    Get raw order book data for a specific token/market
    
    Args:
        token_id: The token ID to get order book for
        use_auth: Whether to use authenticated client
        
    Returns:
        OrderBookSummary: Raw order book with bids and asks
        
    Example:
        >>> orderbook = get_order_book_data("87073854310124528759506128171096701607709284910112533007376905018319069357459")
        >>> print(f"Bids: {len(orderbook['bids'])}, Asks: {len(orderbook['asks'])}")
    """
    try:
        print(f"📖 Getting order book for token: {token_id}")
        
        analyzer = OrderBookAnalyzer(use_auth=use_auth)
        
        # Get the order book
        response = analyzer.client.get_order_book(token_id)
        
        if not response:
            raise ValueError(f"No order book data returned for token {token_id}")
        
        # Extract bids and asks (handle both dict and object responses)
        if hasattr(response, 'get'):
            bids = response.get('bids', [])
            asks = response.get('asks', [])
        else:
            bids = getattr(response, 'bids', [])
            asks = getattr(response, 'asks', [])
        
        print(f"✅ Order book retrieved: {len(bids)} bids, {len(asks)} asks")
        
        return {
            'bids': bids,
            'asks': asks
        }
        
    except Exception as e:
        print(f"❌ Error getting order book for {token_id}: {e}")
        raise


# Convenience function for backward compatibility
def get_order_book(token_id: str, use_auth: bool = False) -> OrderBookSummary:
    """
    Simple function to get order book for a token ID
    
    Args:
        token_id: Market/token identifier
        use_auth: Whether to use authenticated client
        
    Returns:
        OrderBookSummary: Complete order book data
    """
    return get_order_book_data(token_id, use_auth)


# Example usage and testing
if __name__ == "__main__":
    print("📖 Get Order Book Data - Testing\n")
    
    # Example token ID
    test_token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    
    try:
        print(f"🔍 Getting order book for: {test_token_id}")
        
        # Test the function
        orderbook = get_order_book_data(test_token_id)
        
        print(f"✅ Success!")
        print(f"   📊 Bids: {len(orderbook['bids'])}")
        print(f"   📊 Asks: {len(orderbook['asks'])}")
        
        # Show sample data
        if orderbook['bids']:
            print(f"   💰 Best Bid: {orderbook['bids'][0]}")
        if orderbook['asks']:
            print(f"   💰 Best Ask: {orderbook['asks'][0]}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print("\n💡 Make sure to:")
        print("   1. Have valid token ID")
        print("   2. Check network connection")
        print("   3. Verify API endpoint is accessible")
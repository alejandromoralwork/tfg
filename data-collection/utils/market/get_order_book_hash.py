"""
Get Order Book Hash - Order book hash retrieval

Simple function to get the order book hash for a specific token/market.
"""

import os
import sys

# Add the project root to the Python path  
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

from typing import Optional
from utils.market.orderbook_base import OrderBookAnalyzer


def get_order_book_hash_data(token_id: str, use_auth: bool = False) -> Optional[str]:
    """
    Get the order book hash for a specific token/market
    
    Args:
        token_id: The token ID to get hash for
        use_auth: Whether to use authenticated client
        
    Returns:
        Optional[str]: The order book hash if available
        
    Example:
        >>> hash_val = get_order_book_hash_data("87073854310124528759506128171096701607709284910112533007376905018319069357459")
        >>> print(f"Order book hash: {hash_val}")
    """
    try:
        print(f"🔢 Getting order book hash for token: {token_id}")
        
        analyzer = OrderBookAnalyzer(use_auth=use_auth)
        
        # Get the order book hash
        response = analyzer.client.get_order_book(token_id)
        
        if not response:
            print(f"⚠️ No order book data returned for token {token_id}")
            return None
        
        # Extract hash from response (handle both dict and object responses)
        if hasattr(response, 'get'):
            orderbook_hash = response.get('hash')
        else:
            orderbook_hash = getattr(response, 'hash', None)
        
        if orderbook_hash:
            print(f"✅ Order book hash retrieved: {orderbook_hash}")
        else:
            print(f"⚠️ No hash found in order book response")
            
        return orderbook_hash
        
    except Exception as e:
        print(f"❌ Error getting order book hash for {token_id}: {e}")
        raise


# Example usage and testing
if __name__ == "__main__":
    print("🔢 Get Order Book Hash - Testing\n")
    
    # Example token ID
    test_token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    
    try:
        print(f"🔍 Getting order book hash for: {test_token_id}")
        
        # Test the function
        hash_val = get_order_book_hash_data(test_token_id)
        
        if hash_val:
            print(f"✅ Success!")
            print(f"   🔢 Hash: {hash_val}")
        else:
            print(f"⚠️ No hash returned")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print("\n💡 Make sure to:")
        print("   1. Have valid token ID")
        print("   2. Check network connection")
        print("   3. Verify API endpoint is accessible")
"""
Simple Order Book Retrieval - Single function with authentication

Simple function to get order book data for a specific token with all parameters.
Requires authentication and returns complete order book summary.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)


from py_clob_client.clob_types import ApiCreds, OrderArgs
from typing import Dict, List, Any, Optional
from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON
from client.ClobClientWrapper import PolymarketAuth
from dotenv import load_dotenv


load_dotenv("config/.env")

def _setup_client() -> ClobClient:
    """Setup and return authenticated CLOB client"""
    host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
    key = os.getenv("PRIVATE_KEY") or os.getenv("PK")

    creds = ApiCreds(
        api_key=os.getenv("CLOB_API_KEY"),
        api_secret=os.getenv("CLOB_SECRET"),
        api_passphrase=os.getenv("CLOB_PASS_PHRASE"),
    )

    # Always use POLYGON mainnet
    chain_id = POLYGON

    return ClobClient(host, key=key, chain_id=chain_id, creds=creds)


def get_order_book_simple(token_id: str, use_auth: bool = True) -> Dict[str, Any]:
    """
    Get complete order book summary for a specific token
    
    Args:
        token_id: The unique identifier for the token (required)
        use_auth: Whether to use authenticated client (default True, required for this endpoint)
        
    Returns:
        Dict[str, Any]: Complete order book data with all fields:
        - market: Market identifier  
        - asset_id: Asset identifier
        - timestamp: Timestamp of the order book snapshot
        - hash: Hash of the order book state
        - bids: Array of bid levels with price/size
        - asks: Array of ask levels with price/size  
        - min_order_size: Minimum order size for this market
        - tick_size: Minimum price increment
        - neg_risk: Whether negative risk is enabled
        
    Example:
        >>> # Get order book with authentication (required)
        >>> orderbook = get_order_book_simple("87073854310124528759506128171096701607709284910112533007376905018319069357459")
        >>> print(f"Market: {orderbook['market']}")
        >>> print(f"Bids: {len(orderbook['bids'])}, Asks: {len(orderbook['asks'])}")
        >>> print(f"Best bid: {orderbook['bids'][0]['price'] if orderbook['bids'] else 'N/A'}")
        >>> print(f"Best ask: {orderbook['asks'][0]['price'] if orderbook['asks'] else 'N/A'}")
    """
    
    try:
        print(f"📖 Getting order book for token: {token_id}")
        
        if not use_auth:
            print("⚠️ Warning: Authentication is required for this endpoint. Enabling auth...")
            use_auth = True
        
        # Initialize authenticated client
        if use_auth:
            print("🔐 Using authenticated client")
            try:
                auth = PolymarketAuth()
                client = auth.get_client()
            except Exception as e:
                print("❌ Authentication failed. Please ensure the following environment variables are set:")
                print("   - CLOB_API_KEY")
                print("   - CLOB_SECRET")
                print("   - CLOB_PASS_PHRASE")
                print("   - PRIVATE_KEY or PK")
                print(f"   Error details: {e}")
                raise
        else:
            # Fallback to public client (may not work for all endpoints)
            print("🌐 Using public client")
            client = ClobClient(
                host="https://clob.polymarket.com",
                key="",
                chain_id=POLYGON
            )
        
        # Get the order book
        print(f"📡 Fetching order book data...")
        response = client.get_order_book(token_id)
        
        if not response:
            raise ValueError(f"No order book data returned for token {token_id}")
        
        # Handle different response types (dict vs object)
        if hasattr(response, 'get'):
            # Response is dict-like
            orderbook_data = {
                'market': response.get('market', 'N/A'),
                'asset_id': response.get('asset_id', 'N/A'),
                'timestamp': response.get('timestamp', 'N/A'),
                'hash': response.get('hash', 'N/A'),
                'bids': response.get('bids', []),
                'asks': response.get('asks', []),
                'min_order_size': response.get('min_order_size', 'N/A'),
                'tick_size': response.get('tick_size', 'N/A'),
                'neg_risk': response.get('neg_risk', False)
            }
        else:
            # Response is object-like, use attribute access
            orderbook_data = {
                'market': getattr(response, 'market', 'N/A'),
                'asset_id': getattr(response, 'asset_id', 'N/A'),
                'timestamp': getattr(response, 'timestamp', 'N/A'),
                'hash': getattr(response, 'hash', 'N/A'),
                'bids': getattr(response, 'bids', []),
                'asks': getattr(response, 'asks', []),
                'min_order_size': getattr(response, 'min_order_size', 'N/A'),
                'tick_size': getattr(response, 'tick_size', 'N/A'),
                'neg_risk': getattr(response, 'neg_risk', False)
            }
        
        # Log summary
        bids_count = len(orderbook_data['bids'])
        asks_count = len(orderbook_data['asks'])
        
        print(f"✅ Order book retrieved successfully")
        print(f"   📊 Market: {orderbook_data['market']}")
        print(f"   🆔 Asset ID: {orderbook_data['asset_id']}")
        print(f"   📋 Bids: {bids_count}, Asks: {asks_count}")
        print(f"   🔢 Hash: {orderbook_data['hash']}")
        print(f"   ⚙️  Min Order Size: {orderbook_data['min_order_size']}")
        print(f"   📏 Tick Size: {orderbook_data['tick_size']}")
        print(f"   ⚠️ Neg Risk: {orderbook_data['neg_risk']}")
        
        # Show best prices if available
        if orderbook_data['bids']:
            best_bid = orderbook_data['bids'][0]
            print(f"   💚 Best Bid: ${best_bid.get('price', 'N/A')} (Size: {best_bid.get('size', 'N/A')})")
        
        if orderbook_data['asks']:
            best_ask = orderbook_data['asks'][0]
            print(f"   💔 Best Ask: ${best_ask.get('price', 'N/A')} (Size: {best_ask.get('size', 'N/A')})")
        
        return orderbook_data
        
    except Exception as e:
        print(f"❌ Error getting order book for {token_id}: {e}")
        raise


def get_order_book_summary(token_id: str) -> Dict[str, Any]:
    """
    Convenience function - alias for get_order_book_simple with required auth
    
    Args:
        token_id: The unique identifier for the token
        
    Returns:
        Complete order book summary
    """
    return get_order_book_simple(token_id, use_auth=True)


def get_best_prices(token_id: str) -> Dict[str, float]:
    """
    Get just the best bid and ask prices from order book
    
    Args:
        token_id: The unique identifier for the token
        
    Returns:
        Dict with best_bid and best_ask prices
    """
    try:
        orderbook = get_order_book_simple(token_id)
        
        best_bid = None
        best_ask = None
        
        if orderbook['bids']:
            best_bid = float(orderbook['bids'][0].get('price', 0))
        
        if orderbook['asks']:
            best_ask = float(orderbook['asks'][0].get('price', 0))
        
        return {
            'token_id': token_id,
            'best_bid': best_bid,
            'best_ask': best_ask,
            'spread': (best_ask - best_bid) if (best_bid and best_ask) else None,
            'midpoint': ((best_bid + best_ask) / 2) if (best_bid and best_ask) else None
        }
        
    except Exception as e:
        print(f"❌ Error getting best prices for {token_id}: {e}")
        raise


# Example usage and testing
if __name__ == "__main__":
    print("📖 Simple Order Book Retrieval - Testing\n")
    
    # Example token ID
    test_token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    
    try:
        print(f"🔍 Testing order book retrieval for: {test_token_id}")
        
        # Test 1: Get complete order book
        print(f"\n1️⃣ Complete Order Book:")
        orderbook = get_order_book_simple(test_token_id)
        
        print(f"✅ Order Book Data:")
        print(f"   📊 Market: {orderbook['market']}")
        print(f"   🆔 Asset ID: {orderbook['asset_id']}")
        print(f"   ⏰ Timestamp: {orderbook['timestamp']}")
        print(f"   🔢 Hash: {orderbook['hash'][:20]}..." if len(str(orderbook['hash'])) > 20 else orderbook['hash'])
        print(f"   📋 Orders: {len(orderbook['bids'])} bids, {len(orderbook['asks'])} asks")
        print(f"   ⚙️  Min Order Size: {orderbook['min_order_size']}")
        print(f"   📏 Tick Size: {orderbook['tick_size']}")
        print(f"   ⚠️ Neg Risk: {orderbook['neg_risk']}")
        
        # Test 2: Show top orders
        print(f"\n2️⃣ Top Orders:")
        
        if orderbook['bids']:
            print(f"   🟢 Top 3 Bids:")
            for i, bid in enumerate(orderbook['bids'][:3], 1):
                price = bid.get('price', 'N/A')
                size = bid.get('size', 'N/A')
                print(f"      {i}. ${price} × {size}")
        
        if orderbook['asks']:
            print(f"   🔴 Top 3 Asks:")
            for i, ask in enumerate(orderbook['asks'][:3], 1):
                price = ask.get('price', 'N/A')
                size = ask.get('size', 'N/A')
                print(f"      {i}. ${price} × {size}")
        
        # Test 3: Best prices summary
        print(f"\n3️⃣ Best Prices Summary:")
        prices = get_best_prices(test_token_id)
        
        print(f"   💚 Best Bid: ${prices['best_bid']}")
        print(f"   💔 Best Ask: ${prices['best_ask']}")
        if prices['spread']:
            print(f"   📏 Spread: ${prices['spread']:.6f}")
        if prices['midpoint']:
            print(f"   💰 Midpoint: ${prices['midpoint']:.6f}")
        
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Make sure to:")
        print(f"   1. Have valid authentication credentials")
        print(f"   2. Use a valid token ID")
        print(f"   3. Check network connection to CLOB API")
        print(f"   4. Ensure PolymarketAuth is properly configured")

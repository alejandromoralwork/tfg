#!/usr/bin/env python3
"""
Integration test for WebSocket and data collection components
Tests each layer independently to identify issues
"""

import sys
from pathlib import Path

# Add paths
DATA_COLLECTION_ROOT = Path(__file__).parent
sys.path.insert(0, str(DATA_COLLECTION_ROOT))
sys.path.insert(0, str(DATA_COLLECTION_ROOT / "python-order-utils"))

def test_imports():
    """Test 1: Core module imports"""
    print("\n" + "="*60)
    print("TEST 1: Core Module Imports")
    print("="*60)
    
    tests = [
        ("lomond", lambda: __import__('lomond')),
        ("pydantic", lambda: __import__('pydantic')),
        ("requests", lambda: __import__('requests')),
        ("python-dotenv", lambda: __import__('dotenv')),
    ]
    
    for name, loader in tests:
        try:
            loader()
            print(f"  ✓ {name}")
        except ImportError as e:
            print(f"  ✗ {name}: {e}")
            return False
    
    return True

def test_websocket_client():
    """Test 2: WebSocket client import and initialization"""
    print("\n" + "="*60)
    print("TEST 2: WebSocket Client")
    print("="*60)
    
    try:
        from client.py_ws_client.websockets_client import PolymarketWebsocketsClient
        print("  ✓ PolymarketWebsocketsClient imported")
        
        client = PolymarketWebsocketsClient()
        print("  ✓ PolymarketWebsocketsClient instantiated")
        print(f"    - Market URL: {client.url_market}")
        print(f"    - User URL: {client.url_user}")
        print(f"    - Live data URL: {client.url_live_data}")
        return True
    except Exception as e:
        print(f"  ✗ WebSocket client error: {e}")
        import traceback
        traceback.print_exc()
        return False

def test_websocket_core():
    """Test 3: WebSocketCore manager"""
    print("\n" + "="*60)
    print("TEST 3: WebSocketCore Manager")
    print("="*60)
    
    try:
        from client.websocket_proccess import WebSocketCore
        print("  ✓ WebSocketCore imported")
        
        core = WebSocketCore()
        print("  ✓ WebSocketCore instantiated")
        print(f"    - WS Available: {core.ws_available}")
        print(f"    - Client: {core.client}")
        
        status = core.connection_status
        print(f"    - Connection Status: {status}")
        return True
    except Exception as e:
        print(f"  ✗ WebSocketCore error: {e}")
        import traceback
        traceback.print_exc()
        return False

def test_orderbook_logic():
    """Test 4: OrderBook data structure and updates"""
    print("\n" + "="*60)
    print("TEST 4: OrderBook Logic")
    print("="*60)
    
    try:
        from client.websocket_proccess import OrderBook
        print("  ✓ OrderBook imported")
        
        ob = OrderBook(market_slug="test-market")
        print("  ✓ OrderBook instantiated")
        
        # Simulate a price change event
        test_event = {
            "timestamp": "2026-08-02T12:00:00Z",
            "token_id": "test-token-123",
            "price_changes": [
                {
                    "side": "BUY",
                    "best_bid": "0.52",
                    "best_ask": "0.53",
                    "price": "0.52",
                    "size": "100"
                }
            ]
        }
        
        ob.update_from_price_change(test_event)
        print("  ✓ Price change update successful")
        print(f"    - Best Bid: {ob.best_bid}")
        print(f"    - Best Ask: {ob.best_ask}")
        
        data = ob.get_orderbook()
        print(f"    - Orderbook fields: {list(data.keys())}")
        return True
    except Exception as e:
        print(f"  ✗ OrderBook error: {e}")
        import traceback
        traceback.print_exc()
        return False

def test_market_search():
    """Test 5: Market search utilities"""
    print("\n" + "="*60)
    print("TEST 5: Market Search")
    print("="*60)
    
    try:
        from utils.market.market_search import get_market_by_slug, search_markets
        print("  ✓ Market search functions imported")
        
        # Note: This will fetch from API, may take a moment
        print("  ℹ Fetching sample markets from Gamma API (this may take 10-30s)...")
        
        from utils.market.market_search import get_all_active_markets
        markets = get_all_active_markets()
        print(f"  ✓ Loaded {len(markets)} markets from API")
        
        if markets:
            sample = markets[0]
            print(f"    - Sample market:")
            print(f"      Question: {sample.get('question', 'N/A')[:60]}...")
            print(f"      Slug: {sample.get('slug', 'N/A')}")
            print(f"      Tokens: {sample.get('clobTokenIds', [])}")
        
        return True
    except Exception as e:
        print(f"  ✗ Market search error: {e}")
        import traceback
        traceback.print_exc()
        return False

def test_env_config():
    """Test 6: Environment configuration"""
    print("\n" + "="*60)
    print("TEST 6: Environment Configuration")
    print("="*60)
    
    import os
    from pathlib import Path
    
    env_path = Path(__file__).parent / "config" / ".env"
    print(f"  - Config path: {env_path}")
    print(f"  - Exists: {env_path.exists()}")
    
    if env_path.exists():
        env_vars = ["CLOB_API_KEY", "CLOB_SECRET", "CLOB_PASS_PHRASE", "PRIVATE_KEY", "FUNDER"]
        for var in env_vars:
            value = os.getenv(var)
            if value:
                masked = value[:10] + "***" + value[-4:]
                print(f"  ✓ {var}: {masked}")
            else:
                print(f"  ✗ {var}: Not set")
    
    return True

def main():
    """Run all tests"""
    print("\n")
    print("╔" + "="*58 + "╗")
    print("║" + "  DATA COLLECTION INTEGRATION TEST".center(58) + "║")
    print("╚" + "="*58 + "╝")
    
    results = []
    
    # Run tests in sequence
    results.append(("Imports", test_imports()))
    results.append(("WebSocket Client", test_websocket_client()))
    results.append(("WebSocketCore", test_websocket_core()))
    results.append(("OrderBook", test_orderbook_logic()))
    results.append(("Market Search", test_market_search()))
    results.append(("Config", test_env_config()))
    
    # Summary
    print("\n" + "="*60)
    print("TEST SUMMARY")
    print("="*60)
    
    passed = sum(1 for _, result in results if result)
    total = len(results)
    
    for name, result in results:
        status = "✓ PASS" if result else "✗ FAIL"
        print(f"  {status}: {name}")
    
    print(f"\nTotal: {passed}/{total} tests passed")
    
    if passed == total:
        print("\n✓ All systems operational!")
        return 0
    else:
        print(f"\n✗ {total - passed} test(s) failed. See details above.")
        return 1

if __name__ == "__main__":
    sys.exit(main())

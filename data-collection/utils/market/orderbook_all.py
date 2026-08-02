"""
OrderBook All - Complete order book analysis suite

This module imports all order book functions for convenient access.
Import this file to get access to all order book functionality.

Available Functions:
- get_order_book_data(): Get raw order book data
- get_order_book_hash_data(): Get order book hash
- analyze_order_book_data(): Comprehensive analysis 
- get_latest_prices_data(): Simple price information
- compare_multiple_markets_data(): Multi-market comparison

Backward Compatibility Functions:
- get_order_book(): Alias for get_order_book_data()
- get_latest_prices(): Alias for get_latest_prices_data() 
- analyze_market(): Alias for analyze_order_book_data()
"""

# Import all order book functions
try:
    from utils.market.get_order_book_data import get_order_book_data, get_order_book
    from utils.market.get_order_book_hash import get_order_book_hash_data
    from utils.market.analyze_order_book import analyze_order_book_data, analyze_market
    from utils.market.get_latest_prices import get_latest_prices_data, get_latest_prices
    from utils.market.compare_markets import compare_multiple_markets_data, get_best_prices_across_markets

    # Import base class for advanced usage
    from utils.market.orderbook_base import OrderBookAnalyzer, OrderBookSummary
    
    print("✅ All order book functions imported successfully")
    
except ImportError as e:
    print(f"❌ Import error in orderbook_all.py: {e}")
    # Define what we can import
    try:
        from utils.market.orderbook_base import OrderBookAnalyzer, OrderBookSummary
        print("✅ Base classes imported successfully")
    except ImportError:
        print("❌ Could not import base classes")

# Version info
__version__ = "1.0.0"
__author__ = "Polymarket Trading System"

# All available functions
__all__ = [
    # Main functions
    'get_order_book_data',
    'get_order_book_hash_data', 
    'analyze_order_book_data',
    'get_latest_prices_data',
    'compare_multiple_markets_data',
    'get_best_prices_across_markets',
    
    # Backward compatibility
    'get_order_book',
    'get_latest_prices', 
    'analyze_market',
    
    # Base class and types
    'OrderBookAnalyzer',
    'OrderBookSummary'
]


def print_available_functions():
    """Print all available order book functions"""
    print("📊 Available Order Book Functions:")
    print("=" * 50)
    
    print("\n🔧 Core Functions:")
    print("  • get_order_book_data(token_id) - Get raw order book")
    print("  • get_order_book_hash_data(token_id) - Get order book hash") 
    print("  • analyze_order_book_data(token_id) - Full analysis")
    print("  • get_latest_prices_data(token_id) - Simple prices")
    print("  • compare_multiple_markets_data(token_ids) - Compare markets")
    
    print("\n🔄 Convenience Functions:")
    print("  • get_order_book(token_id) - Alias for get_order_book_data()")
    print("  • get_latest_prices(token_id) - Alias for get_latest_prices_data()")
    print("  • analyze_market(token_id) - Alias for analyze_order_book_data()")
    
    print("\n🏗️ Advanced:")
    print("  • OrderBookAnalyzer - Base class for custom analysis")
    print("  • get_best_prices_across_markets(token_ids) - Cross-market best prices")
    
    print(f"\n📦 Version: {__version__}")


# Example usage
if __name__ == "__main__":
    print("📊 Order Book Analysis Suite\n")
    
    # Show available functions
    print_available_functions()
    
    # Example usage
    print("\n" + "="*50)
    print("📋 Example Usage:")
    print("="*50)
    
    print("""
# Import everything
from utils.market.orderbook_all import *

# Get simple prices
token_id = "your_token_id_here"
prices = get_latest_prices(token_id)
print(f"Midpoint: ${prices['midpoint_price']}")

# Get comprehensive analysis  
analysis = analyze_market(token_id)
print(f"Spread: {analysis['spread_percentage']}%")
print(f"Total orders: {analysis['total_orders']}")

# Compare multiple markets
markets = [token_id1, token_id2, token_id3]  
comparisons = compare_multiple_markets_data(markets)
for comp in comparisons:
    print(f"{comp['token_id']}: ${comp['midpoint_price']}")

# Get raw order book data
orderbook = get_order_book(token_id)
print(f"Bids: {len(orderbook['bids'])}")
print(f"Asks: {len(orderbook['asks'])}")
""")
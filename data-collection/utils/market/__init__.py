"""
Market Data & Analysis Utilities (Consolidated)
=============================================
Functions for retrieving market data, prices, and order books.
Consolidated market search with token ID support for trading.
"""

try:
    # Basic market data functions
    from .get_prices import get_prices, get_token_prices
    from .get_mid_market_price import get_mid_market_price  
    from .get_orderbook import get_order_book
    from .get_spreads import get_spread
    from .get_trades import get_trade_history
    from .get_last_trade_price import get_last_trade_price
    
    # Consolidated market search (replaces get_markets, market_search, advanced_market_search)
    from .market_search import (
        get_markets,
        get_market_by_event_slug,
        get_market_by_market_slug,
        search_markets_by_question,
        extract_token_ids_only,
        get_market_with_prices,
        TokenInfo,
        MarketTokens
    )
    
except ImportError as e:
    print(f"[WARNING] Some market utilities not available: {e}")

__all__ = [
    # Basic market data functions
    'get_prices',
    'get_token_prices', 
    'get_mid_market_price',
    'get_order_book',
    'get_spread',
    'get_trade_history',
    'get_last_trade_price',
    
    # Consolidated market search functions
    'get_markets',
    'get_market_by_event_slug',
    'get_market_by_market_slug',
    'search_markets_by_question',
    'extract_token_ids_only',
    'get_market_with_prices',
    
    # Data structures
    'TokenInfo',
    'MarketTokens'
]

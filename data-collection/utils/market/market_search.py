"""
Simple Market Search Utilities
=============================
Find Polymarket markets and get their trading token IDs.

Purpose: Make it easy to search for markets and get the token IDs needed for trading.
Uses Gamma API for comprehensive event and market data.
"""

import argparse
import os
import re
from pathlib import Path
from typing import Any, Dict, List, NamedTuple, Optional

import requests

try:
    from dotenv import load_dotenv
except ModuleNotFoundError:
    def load_dotenv(path: str | os.PathLike[str] | None = None) -> bool:
        """Minimal .env loader fallback when python-dotenv is unavailable."""
        if path is None:
            return False

        env_path = Path(path)
        if not env_path.exists():
            return False

        for raw_line in env_path.read_text(encoding="utf-8").splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue

            key, value = line.split("=", 1)
            key = key.strip()
            value = value.strip().strip('"').strip("'")
            os.environ.setdefault(key, value)
        return True

try:
    from py_clob_client.client import ClobClient
    from py_clob_client.constants import POLYGON
except ModuleNotFoundError:
    ClobClient = None
    POLYGON = 137

ENV_PATH = Path(__file__).resolve().parents[2] / "config" / ".env"
load_dotenv(ENV_PATH)


class TokenInfo(NamedTuple):
    """Holds token info needed for trading"""
    clob_token_id: str  # This is what you need for placing orders
    outcome: str        # "Yes" or "No" usually
    winner: bool        # Is this the winning token?


class MarketTokens(NamedTuple):
    """Combines market info with its trading tokens"""
    market: Dict[str, Any]    # All market data from API
    tokens: List[TokenInfo]   # Trading tokens for this market


# Global client - reused to avoid creating new connections
_client = None

# API endpoints
GAMMA_API_BASE = "https://gamma-api.polymarket.com"
CLOB_API_BASE = "https://clob.polymarket.com"


def _get_client() -> ClobClient:
    """Get or create the Polymarket client (internal function)"""
    global _client
    if ClobClient is None:
        raise ModuleNotFoundError("py_clob_client is not installed")

    if _client is None:
        host = os.getenv("CLOB_API_URL", CLOB_API_BASE)
        _client = ClobClient(
            host=host,
            chain_id=POLYGON,
            key=os.getenv("PRIVATE_KEY"),
            creds=None,
            signature_type=0,
            funder=os.getenv("FUNDER")
        )
    return _client

# Simple data structures for organizing market + token data
class TokenInfo(NamedTuple):
    """Holds token info needed for trading"""
    clob_token_id: str  # This is what you need for placing orders
    outcome: str        # "Yes" or "No" usually
    winner: bool        # Is this the winning token?

class MarketTokens(NamedTuple):
    """Combines market info with its trading tokens"""
    market: Dict[str, Any]    # All market data from API
    tokens: List[TokenInfo]   # Trading tokens for this market

# Global client - reused to avoid creating new connections
_client = None

# API endpoints
GAMMA_API_BASE = "https://gamma-api.polymarket.com"
CLOB_API_BASE = "https://clob.polymarket.com"

def _get_client() -> ClobClient:
    """Get or create the Polymarket client (internal function)"""
    global _client
    if _client is None:
        host = os.getenv("CLOB_API_URL", CLOB_API_BASE)
        _client = ClobClient(
            host=host,
            chain_id=POLYGON,
            key=os.getenv("PRIVATE_KEY"),
            creds=None,
            signature_type=0,
            funder=os.getenv("FUNDER")
        )
    return _client

def _normalize_clob_token_ids(token_ids):
    """Helper to normalize token IDs from API"""
    if isinstance(token_ids, str):
        # Try to parse as JSON array string
        try:
            import json
            return json.loads(token_ids)
        except:
            return [token_ids]
    elif isinstance(token_ids, list):
        return token_ids
    else:
        return []

def get_all_active_markets() -> List[Dict[str, Any]]:
    """
    Get ALL active markets using Gamma API (recommended approach)
    
    Purpose: Efficiently fetch all active markets from both events and standalone.
    This is the most comprehensive way to get market data with proper pagination.
    
    Returns: List of all active market dictionaries with full data
    """
    all_markets = []
    
    try:
        # PRIMARY: Get markets from events (recommended by Polymarket)
        print("[INFO] Fetching markets from events...")
        
        events_url = f"{GAMMA_API_BASE}/events"
        limit = 100
        offset = 0
        
        while True:
            try:
                params = {
                    "order": "id",
                    "ascending": "false",
                    "closed": "false",      # Only active markets
                    "limit": str(limit),
                    "offset": str(offset)
                }
                
                response = requests.get(events_url, params=params, timeout=30)
                response.raise_for_status()
                batch_events = response.json()
                
                if not batch_events:
                    break
                
                # Extract markets from events
                for event in batch_events:
                    if not event.get("active", True) or event.get("closed", False):
                        continue
                    
                    for market in event.get("markets", []):
                        if not market.get("active", True) or market.get("closed", False):
                            continue
                        
                        # Parse outcome prices
                        outcome_prices = market.get("outcomePrices")
                        if outcome_prices:
                            outcome_prices_str = str(outcome_prices)
                            match = re.search(r'\[\"([0-9]+\.[0-9]+)\", \"([0-9]+\.[0-9]+)\"\]', outcome_prices_str)
                            
                            if match:
                                parsed_prices = [float(match.group(1)), float(match.group(2))]
                                
                                # Safe parsing of numeric fields
                                try:
                                    volume = float(market.get("volume", 0)) if market.get("volume") else 0
                                except (ValueError, TypeError):
                                    volume = 0
                                
                                try:
                                    liquidity = float(market.get("liquidity", 0)) if market.get("liquidity") else 0
                                except (ValueError, TypeError):
                                    liquidity = 0
                                
                                parsed_market = {
                                    "id": market.get("id"),
                                    "slug": market.get("slug"),
                                    "question": market.get("question", ""),
                                    "description": market.get("description", ""),
                                    "outcomePrices": parsed_prices,
                                    "endDate": market.get("endDate"),
                                    "volume": volume,
                                    "liquidity": liquidity,
                                    "clobTokenIds": _normalize_clob_token_ids(market.get("clobTokenIds", [])),
                                    "source": "event",
                                    "active": market.get("active", True),
                                    "closed": market.get("closed", False)
                                }
                                
                                all_markets.append(parsed_market)
                
                if len(batch_events) < limit:
                    break
                    
                offset += len(batch_events)
                
                if offset > 10000:  # Safety limit
                    break
                    
            except Exception as e:
                print(f"[ERROR] Events batch error at offset {offset}: {e}")
                break
        
        print(f"[INFO] Found {len(all_markets)} markets from events")
        
    except Exception as e:
        print(f"[ERROR] Failed to fetch event markets: {e}")
    
    try:
        # SUPPLEMENT: Get standalone markets
        print("[INFO] Fetching standalone markets...")
        
        markets_url = f"{GAMMA_API_BASE}/markets"
        initial_count = len(all_markets)
        limit = 100
        offset = 0
        
        while True:
            try:
                params = {
                    "order": "id",
                    "ascending": "false", 
                    "closed": "false",
                    "limit": str(limit),
                    "offset": str(offset)
                }
                
                response = requests.get(markets_url, params=params, timeout=30)
                response.raise_for_status()
                batch_markets = response.json()
                
                if not batch_markets:
                    break
                
                for market in batch_markets:
                    if not market.get("active", True) or market.get("closed", False):
                        continue
                    
                    # Parse outcome prices
                    outcome_prices = market.get("outcomePrices")
                    if outcome_prices:
                        outcome_prices_str = str(outcome_prices)
                        match = re.search(r'\[\"([0-9]+\.[0-9]+)\", \"([0-9]+\.[0-9]+)\"\]', outcome_prices_str)
                        
                        if match:
                            parsed_prices = [float(match.group(1)), float(match.group(2))]
                            
                            try:
                                volume = float(market.get("volume", 0)) if market.get("volume") else 0
                            except (ValueError, TypeError):
                                volume = 0
                            
                            try:
                                liquidity = float(market.get("liquidity", 0)) if market.get("liquidity") else 0
                            except (ValueError, TypeError):
                                liquidity = 0
                            
                            parsed_market = {
                                "id": market.get("id"),
                                "slug": market.get("slug"),
                                "question": market.get("question", ""),
                                "description": market.get("description", ""),
                                "outcomePrices": parsed_prices,
                                "endDate": market.get("endDate"),
                                "volume": volume,
                                "liquidity": liquidity,
                                "clobTokenIds": _normalize_clob_token_ids(market.get("clobTokenIds", [])),
                                "source": "standalone",
                                "active": market.get("active", True),
                                "closed": market.get("closed", False)
                            }
                            
                            all_markets.append(parsed_market)
                
                if len(batch_markets) < limit:
                    break
                    
                offset += len(batch_markets)
                
                if offset > 10000:  # Safety limit
                    break
                    
            except Exception as e:
                print(f"[ERROR] Standalone batch error at offset {offset}: {e}")
                break
        
        standalone_count = len(all_markets) - initial_count
        print(f"[INFO] Found {standalone_count} additional standalone markets")
        
    except Exception as e:
        print(f"[ERROR] Failed to fetch standalone markets: {e}")
    
    # Remove duplicates by ID (events take priority)
    seen_ids = set()
    unique_markets = []
    
    # First pass: Add event markets
    for market in all_markets:
        if market.get('source') == 'event':
            market_id = market.get("id")
            if market_id and market_id not in seen_ids:
                seen_ids.add(market_id)
                unique_markets.append(market)
    
    # Second pass: Add standalone markets not already seen
    for market in all_markets:
        if market.get('source') == 'standalone':
            market_id = market.get("id")
            if market_id and market_id not in seen_ids:
                seen_ids.add(market_id)
                unique_markets.append(market)
    
    duplicates_removed = len(all_markets) - len(unique_markets)
    if duplicates_removed > 0:
        print(f"[INFO] Removed {duplicates_removed} duplicate markets")
    
    print(f"[INFO] Total unique active markets: {len(unique_markets)}")
    return unique_markets

def search_markets_by_event_slug(event_slug: str) -> List[MarketTokens]:
    """
    Search markets by event slug using comprehensive market data
    
    Purpose: Find all markets within a specific event. This now uses the comprehensive
    market data which includes both event-sourced and standalone markets.
    
    Args:
        event_slug: Event slug to search for (e.g., "presidential-election-winner-2024")
        
    Returns: List of MarketTokens objects for all markets matching the event
    
    Example:
        markets = search_markets_by_event_slug("presidential-election")
        for market_data in markets:
            print(f"Market: {market_data.market['question']}")
            print(f"Tokens: {[t.clob_token_id for t in market_data.tokens]}")
    """
    # Get all markets using the comprehensive function
    all_markets = get_all_active_markets()
    if not all_markets:
        print("[ERROR] No markets available")
        return []
    
    # Search for matching markets
    matching_markets = []
    event_slug_lower = event_slug.lower()
    
    for market in all_markets:
        # Check slug match
        market_slug = market.get('slug', '').lower()
        question = market.get('question', '').lower()
        
        # Search in slug and question
        if (event_slug_lower in market_slug or 
            event_slug_lower.replace('-', ' ') in question):
            matching_markets.append(market)
    
    if not matching_markets:
        print(f"[ERROR] No markets found for event slug: {event_slug}")
        return []
    
    print(f"[INFO] Found {len(matching_markets)} markets for event: {event_slug}")
    
    # Convert to MarketTokens format
    markets_with_tokens = []
    for market in matching_markets:
        # Extract token information
        tokens = []
        clob_token_ids = market.get('clobTokenIds', [])
        
        # Create tokens based on available clob token IDs
        if len(clob_token_ids) >= 2:
            tokens.append(TokenInfo(
                clob_token_id=clob_token_ids[0],
                outcome="Yes",
                winner=False
            ))
            tokens.append(TokenInfo(
                clob_token_id=clob_token_ids[1],
                outcome="No", 
                winner=False
            ))
        
        markets_with_tokens.append(MarketTokens(market=market, tokens=tokens))
    
    return markets_with_tokens

def search_markets(query: str, limit: int = 10) -> List[Dict[str, Any]]:
    """
    Search markets by text (looks in question and slug)
    
    Purpose: Find markets by searching for keywords like "trump", "election", etc.
    This is the main function you'll use to find markets to trade.
    
    Args:
        query: What to search for (e.g. "trump", "election")
        limit: Max number of results
        
    Returns: List of matching markets
    
    Example:
        markets = search_markets("trump")
        for market in markets:
            print(market['question'])
    """
    markets = get_all_active_markets()
    if not markets:
        return []
    
    query_lower = query.lower()
    results = []
    
    for market in markets:
        question = market.get('question', '').lower()
        slug = market.get('slug', '').lower()
        
        # Search in question text and slug
        if query_lower in question or query_lower in slug:
            results.append(market)
            if len(results) >= limit:
                break
    
    print(f"[INFO] Found {len(results)} markets for '{query}'")
    return results

def get_market_by_slug(slug: str) -> Optional[Dict[str, Any]]:
    """
    Get a specific market by its slug (exact match)
    
    Purpose: When you know the exact slug of a market, use this to get its details.
    Slugs are the URL-friendly names like "will-trump-win-2024".
    
    Args:
        slug: Exact market slug
        
    Returns: Market data or None if not found
    
    Example:
        market = get_market_by_slug("will-trump-win-2024")
        if market:
            print(f"Found: {market['question']}")
    """
    markets = get_all_active_markets()
    
    for market in markets:
        if market.get('slug') == slug:
            print(f"[INFO] Found market: {market.get('question', 'Unknown')}")
            return market
    
    print(f"[ERROR] No market found with slug: {slug}")
    return None

def get_market_with_tokens(slug: str) -> Optional[MarketTokens]:
    """
    Get market AND its trading tokens (this is what you want for trading!)
    
    Purpose: This gets you everything you need to place trades - the market info
    plus the token IDs you need to buy/sell. This is the most useful function.
    
    Args:
        slug: Market slug to find
        
    Returns: MarketTokens object with market data + token IDs
    
    Example:
        result = get_market_with_tokens("will-trump-win-2024")
        if result:
            print(f"Market: {result.market['question']}")
            for token in result.tokens:
                print(f"  {token.outcome}: {token.clob_token_id}")
    """
    market = get_market_by_slug(slug)
    if not market:
        return None
    
    # Extract token information for trading
    tokens = []
    clob_token_ids = market.get('clobTokenIds', [])
    
    # Create tokens based on available clob token IDs
    if len(clob_token_ids) >= 2:
        tokens.append(TokenInfo(
            clob_token_id=clob_token_ids[0],
            outcome="Yes",
            winner=False
        ))
        tokens.append(TokenInfo(
            clob_token_id=clob_token_ids[1],
            outcome="No",
            winner=False
        ))
    elif len(clob_token_ids) == 1:
        tokens.append(TokenInfo(
            clob_token_id=clob_token_ids[0],
            outcome="Unknown",
            winner=False
        ))
    
    return MarketTokens(market=market, tokens=tokens)

def search_markets_with_tokens(query: str, limit: int = 5) -> List[MarketTokens]:
    """
    Search for markets AND get their trading tokens
    
    Purpose: Combines search + token extraction. Use this when you want to
    search for markets and immediately get their trading token IDs.
    
    Args:
        query: Search term
        limit: Max results
        
    Returns: List of MarketTokens objects
    
    Example:
        results = search_markets_with_tokens("election")
        for result in results:
            print(f"Market: {result.market['question']}")
            token_ids = [t.clob_token_id for t in result.tokens]
            print(f"Token IDs: {token_ids}")
    """
    markets = search_markets(query, limit)
    results = []
    
    for market in markets:
        # Extract tokens for each market
        tokens = []
        for token_data in market.get('tokens', []):
            if token_data.get('token_id'):
                tokens.append(TokenInfo(
                    clob_token_id=token_data['token_id'],
                    outcome=token_data.get('outcome', 'Unknown'),
                    winner=token_data.get('winner', False)
                ))
        
        results.append(MarketTokens(market=market, tokens=tokens))
    
    return results

def get_token_ids_only(market_slug: str) -> List[str]:
    """
    Quick function to just get the token IDs for trading
    
    Purpose: When you just want the token IDs and nothing else.
    This is the shortest path from market slug to tradeable token IDs.
    
    Args:
        market_slug: Market to get tokens for
        
    Returns: List of token IDs ready for trading
    
    Example:
        token_ids = get_token_ids_only("will-trump-win-2024")
        # Use these IDs to place orders: client.create_order(token_id=token_ids[0], ...)
    """
    result = get_market_with_tokens(market_slug)
    if result:
        return [token.clob_token_id for token in result.tokens]
    return []

# Test function to demonstrate usage
def main():
    """Test the simplified market search functions"""
    parser = argparse.ArgumentParser(description="Test Polymarket market search utilities")
    parser.add_argument(
        "--slug",
        type=str,
        default="hype-updown-5m-1785762000",
        help="Exact market slug to test with get_market_by_slug()",
    )
    args = parser.parse_args()

    print("Simple Market Search Test")
    print("=" * 40)
    
    # 1. Test comprehensive market loading
    print("\n1. Testing comprehensive market loading...")
    
    # Get a sample of markets to test with
    all_markets = get_all_active_markets()
    if all_markets:
        print(f"   Successfully loaded {len(all_markets)} total active markets")
        
        # Show some examples
        print(f"   Sample markets:")
        for i, market in enumerate(all_markets[:3], 1):
            question = market.get('question', 'Unknown question')
            slug = market.get('slug', 'no-slug')
            source = market.get('source', 'unknown')
            tokens = len(market.get('clobTokenIds', []))
            print(f"   {i}. {question[:60]}...")
            print(f"      Slug: {slug}")
            print(f"      Source: {source}, Tokens: {tokens}")
        
 
    # 4. Test exact slug lookup

    print(f"\n4. Testing exact slug lookup: {args.slug}")
    market = get_market_by_slug(args.slug)
    if market:
        print(f"   Found question: {market.get('question', 'Unknown question')}")
        print(f"   Slug: {market.get('slug', 'no-slug')}")
        print(f"   Token IDs: {market.get('clobTokenIds', [])}")
        market_with_tokens = get_market_with_tokens(args.slug)
        if market_with_tokens:
            print(f"   Token count: {len(market_with_tokens.tokens)}")
            for token in market_with_tokens.tokens:
                print(f"     - {token.outcome}: {token.clob_token_id}")


if __name__ == "__main__":
    main()
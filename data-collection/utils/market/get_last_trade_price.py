"""
Get last trade price from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any, List
from dotenv import load_dotenv

from py_clob_client.client import ClobClient

# Load from config folder
load_dotenv("config/.env")


def get_last_trade_price(token_id: str) -> Dict[str, Any]:
    """
    Get last trade price for a specific token
    
    Args:
        token_id: The token ID to get last trade price for
        
    Returns:
        Dict with last trade price information
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Fetching last trade price for token: {token_id[:20]}...")
        last_price = client.get_last_trade_price(token_id)
        
        print("✅ Last trade price retrieved successfully")
        return last_price
        
    except Exception as e:
        print(f"❌ Error getting last trade price: {e}")
        raise


def get_last_trades_prices(token_ids: List[str]) -> Dict[str, Any]:
    """
    Get last trade prices for multiple tokens
    
    Args:
        token_ids: List of token IDs
        
    Returns:
        Dict with last trade prices for all tokens
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Fetching last trade prices for {len(token_ids)} tokens...")
        last_prices = client.get_last_trades_prices(token_ids)
        
        print("✅ Last trade prices retrieved successfully")
        return last_prices
        
    except Exception as e:
        print(f"❌ Error getting last trade prices: {e}")
        raise


def get_price_with_analysis(token_id: str) -> Dict[str, Any]:
    """
    Get last trade price with additional analysis
    
    Args:
        token_id: The token ID to analyze
        
    Returns:
        Dict with price and analysis
    """
    try:
        last_price_data = get_last_trade_price(token_id)
        
        # Extract price information
        price = float(last_price_data.get('price', 0))
        timestamp = last_price_data.get('timestamp', 0)
        
        # Calculate implied probability (price = probability for binary markets)
        implied_probability = price * 100  # Convert to percentage
        
        # Calculate odds
        if price > 0 and price < 1:
            decimal_odds = 1 / price
            american_odds = (decimal_odds - 1) * 100 if decimal_odds >= 2 else -100 / (decimal_odds - 1)
        else:
            decimal_odds = 0
            american_odds = 0
        
        analysis = {
            'token_id': token_id,
            'last_price': round(price, 4),
            'implied_probability_pct': round(implied_probability, 2),
            'decimal_odds': round(decimal_odds, 2),
            'american_odds': round(american_odds, 0),
            'timestamp': timestamp,
            'raw_data': last_price_data
        }
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing price: {e}")
        raise


def compare_token_prices(token_ids: List[str]) -> List[Dict[str, Any]]:
    """
    Compare last trade prices for multiple tokens
    
    Args:
        token_ids: List of token IDs to compare
        
    Returns:
        List of price comparisons sorted by price
    """
    try:
        comparisons = []
        
        for token_id in token_ids:
            try:
                analysis = get_price_with_analysis(token_id)
                comparisons.append(analysis)
            except Exception as e:
                print(f"⚠️ Failed to get price for {token_id[:20]}...: {e}")
                continue
        
        # Sort by price (highest first)
        comparisons.sort(key=lambda x: x['last_price'], reverse=True)
        
        print(f"📊 Compared prices for {len(comparisons)} tokens")
        return comparisons
        
    except Exception as e:
        print(f"❌ Error comparing prices: {e}")
        raise


def main():
    """Example usage"""
    print("💰 Get Last Trade Price Utility")
    print("=" * 40)
    
    # Example token IDs
    token_ids = [
        "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "52114319501245915516055106046884209969926127482827954674443846427813813222426"
    ]
    
    try:
        # Single token price
        print("\n1️⃣ Single token price:")
        analysis = get_price_with_analysis(token_ids[0])
        print(f"   Token: {analysis['token_id'][:20]}...")
        print(f"   Last Price: ${analysis['last_price']}")
        print(f"   Probability: {analysis['implied_probability_pct']}%")
        print(f"   Decimal Odds: {analysis['decimal_odds']}")
        print(f"   American Odds: {analysis['american_odds']}")
        
        # Multiple token prices
        print("\n2️⃣ Multiple token prices:")
        prices = get_last_trades_prices(token_ids)
        print(f"   Retrieved prices for {len(prices)} tokens")
        for token_id, price_data in prices.items():
            price = price_data.get('price', 0)
            print(f"   {token_id[:20]}...: ${price}")
        
        # Price comparison
        print("\n3️⃣ Price comparison:")
        comparisons = compare_token_prices(token_ids)
        
        for i, comp in enumerate(comparisons, 1):
            print(f"   {i}. {comp['token_id'][:20]}... - ${comp['last_price']} ({comp['implied_probability_pct']}%)")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
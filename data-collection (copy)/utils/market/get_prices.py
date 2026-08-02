"""
Get prices for tokens from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import List, Dict, Any, Optional
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import BookParams

# Load from config folder
load_dotenv("config/.env")


def get_price(token_id: str, side: str) -> str:
    """
    Get market price for a specific token and side
    
    Args:
        token_id: The unique identifier for the token
        side: The side of the market (BUY or SELL)
        
    Returns:
        str: The market price (as string to maintain precision)
        
    Raises:
        ValueError: If side is not BUY or SELL
        Exception: If API call fails
    """
    if side not in ["BUY", "SELL"]:
        raise ValueError(f"Invalid side: {side}. Must be 'BUY' or 'SELL'")
    
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Getting {side} price for token {token_id[:20]}...")
        
        # Use the /price endpoint
        response = client.get_price(token_id=token_id, side=side)
        
        # Extract price from response
        price = response.get('price')
        
        if price is None:
            raise Exception("Price not found in response")
        
        print(f"✅ Price: {price}")
        return str(price)
        
    except Exception as e:
        print(f"❌ Error getting price: {e}")
        raise


def get_prices(token_params: List[BookParams]) -> Dict[str, Any]:
    """
    Get prices for multiple tokens and sides
    
    Args:
        token_params: List of BookParams with token_id and side
        
    Returns:
        Dict with price information for each token/side combination
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Getting prices for {len(token_params)} token/side combinations...")
        
        response = client.get_prices(params=token_params)
        
        print("✅ Successfully retrieved prices")
        return response
        
    except Exception as e:
        print(f"❌ Error getting prices: {e}")
        raise


def get_token_prices(token_id: str) -> Dict[str, Any]:
    """
    Get both BUY and SELL prices for a single token
    
    Args:
        token_id: The token ID to get prices for
        
    Returns:
        Dict with BUY and SELL price information
    """
    params = [
        BookParams(token_id=token_id, side="BUY"),
        BookParams(token_id=token_id, side="SELL"),
    ]
    
    return get_prices(params)


def get_multiple_token_prices(token_ids: List[str]) -> Dict[str, Any]:
    """
    Get BUY and SELL prices for multiple tokens
    
    Args:
        token_ids: List of token IDs
        
    Returns:
        Dict with price information for all tokens
    """
    params = []
    for token_id in token_ids:
        params.extend([
            BookParams(token_id=token_id, side="BUY"),
            BookParams(token_id=token_id, side="SELL"),
        ])
    
    return get_prices(params)


def main():
    """Example usage"""
    import argparse
    
    parser = argparse.ArgumentParser(description="Get Polymarket token price")
    parser.add_argument("--token-id", type=str, help="Token ID to get price for")
    parser.add_argument("--side", type=str, choices=["BUY", "SELL"], help="Side (BUY or SELL)")
    parser.add_argument("--demo", action="store_true", help="Run demo with example tokens")
    
    args = parser.parse_args()
    
    print("💰 Get Prices Utility")
    print("=" * 40)
    
    # If specific token and side provided, get single price
    if args.token_id and args.side:
        try:
            price = get_price(args.token_id, args.side)
            print(f"\n✅ {args.side} Price: {price}")
        except Exception as e:
            print(f"❌ Error: {e}")
        return
    
    # Otherwise run demo
    if args.demo or (not args.token_id and not args.side):
        # Example token IDs
        token_ids = [
            "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            "52114319501245915516055106046884209969926127482827954674443846427813813222426"
        ]
        
        try:
            # Method 0: Single price endpoint
            print("\n0️⃣ Single price (new endpoint):")
            buy_price = get_price(token_ids[0], "BUY")
            sell_price = get_price(token_ids[0], "SELL")
            print(f"BUY Price: {buy_price}")
            print(f"SELL Price: {sell_price}")
            
            # Method 1: Single token prices
            print("\n1️⃣ Single token prices (batch):")
            single_prices = get_token_prices(token_ids[0])
            print(f"Prices for first token: {single_prices}")
            
            # Method 2: Multiple token prices
            print(f"\n2️⃣ Multiple token prices:")
            multi_prices = get_multiple_token_prices(token_ids)
            print(f"Prices for {len(token_ids)} tokens: {multi_prices}")
            
            # Method 3: Custom parameters
            print(f"\n3️⃣ Custom parameters:")
            custom_params = [
                BookParams(token_id=token_ids[0], side="BUY"),
                BookParams(token_id=token_ids[1], side="SELL"),
            ]
            custom_prices = get_prices(custom_params)
            print(f"Custom prices: {custom_prices}")
            
        except Exception as e:
            print(f"❌ Error: {e}")
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
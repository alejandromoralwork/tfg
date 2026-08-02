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
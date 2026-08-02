"""
Place market buy orders on Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, MarketOrderArgs, OrderType
from py_clob_client.constants import POLYGON
from py_clob_client.order_builder.constants import BUY

# Load from config folder
load_dotenv("config/.env")


def place_market_buy_order(token_id: str, amount: float) -> Dict[str, Any]:
    """
    Place a market buy order
    
    Args:
        token_id: Token to buy
        amount: Amount in USDC to spend
        
    Returns:
        Order response
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
        
        creds = ApiCreds(
            api_key=os.getenv("CLOB_API_KEY"),
            api_secret=os.getenv("CLOB_SECRET"),
            api_passphrase=os.getenv("CLOB_PASS_PHRASE"),
        )
        
        client = ClobClient(host, key=key, chain_id=POLYGON, creds=creds)
        
        print(f"💰 Placing market buy order...")
        print(f"   Token ID: {token_id[:20]}...")
        print(f"   Amount: ${amount}")
        
        # Create market order
        market_order_args = MarketOrderArgs(
            token_id=token_id,
            amount=amount,
            side=BUY,
        )
        
        # Create and sign the order
        signed_order = client.create_market_order(market_order_args)
        
        # Place the order
        response = client.post_order(signed_order, orderType=OrderType.FOK)
        
        print("✅ Market buy order placed successfully!")
        return response
        
    except Exception as e:
        print(f"❌ Error placing market buy order: {e}")
        raise


def place_small_market_buy(token_id: str, usd_amount: float = 1.0) -> Dict[str, Any]:
    """
    Place a small market buy order for testing
    
    Args:
        token_id: Token to buy
        usd_amount: Small amount in USD (default: $1)
        
    Returns:
        Order response
    """
    try:
        print(f"🧪 Placing small test market buy order for ${usd_amount}")
        return place_market_buy_order(token_id, usd_amount)
        
    except Exception as e:
        print(f"❌ Error placing small market buy order: {e}")
        raise


def estimate_market_buy_outcome(token_id: str, amount: float) -> Dict[str, Any]:
    """
    Estimate outcome of market buy order without placing it
    
    Args:
        token_id: Token to estimate
        amount: Amount in USD
        
    Returns:
        Dict with estimated outcome
    """
    try:
        from utils.market.get_orderbook import get_orderbook
        
        print(f"📊 Estimating market buy outcome for ${amount}...")
        
        orderbook = get_orderbook(token_id)
        asks = orderbook.get('asks', [])
        
        if not asks:
            raise ValueError("No asks available in orderbook")
        
        remaining_amount = amount
        total_tokens = 0
        avg_price = 0
        levels_used = 0
        
        for ask in asks:
            ask_price = float(ask['price'])
            ask_size = float(ask['size'])
            ask_value = ask_price * ask_size
            
            if remaining_amount <= 0:
                break
                
            if remaining_amount >= ask_value:
                # Take entire level
                total_tokens += ask_size
                remaining_amount -= ask_value
                levels_used += 1
            else:
                # Partial level
                partial_tokens = remaining_amount / ask_price
                total_tokens += partial_tokens
                remaining_amount = 0
                levels_used += 1
        
        if total_tokens > 0:
            avg_price = (amount - remaining_amount) / total_tokens
        
        estimate = {
            'input_amount_usd': amount,
            'estimated_tokens': round(total_tokens, 4),
            'average_price': round(avg_price, 4),
            'levels_used': levels_used,
            'amount_filled': amount - remaining_amount,
            'amount_remaining': remaining_amount,
            'fully_fillable': remaining_amount == 0,
            'slippage': round(((avg_price / float(asks[0]['price'])) - 1) * 100, 2) if asks else 0
        }
        
        return estimate
        
    except Exception as e:
        print(f"❌ Error estimating market buy: {e}")
        raise


def main():
    """Example usage"""
    print("💰 Market Buy Order Utility")
    print("=" * 40)
    
    # Example token ID
    token_id = "71321045679252212594626385532706912750332728571942532289631379312455583992563"
    
    try:
        # Estimate market buy first
        print("\n1️⃣ Estimating market buy outcome:")
        test_amount = 10.0  # $10
        estimate = estimate_market_buy_outcome(token_id, test_amount)
        
        print(f"   Input: ${estimate['input_amount_usd']}")
        print(f"   Estimated tokens: {estimate['estimated_tokens']}")
        print(f"   Average price: ${estimate['average_price']}")
        print(f"   Levels used: {estimate['levels_used']}")
        print(f"   Slippage: {estimate['slippage']}%")
        print(f"   Fully fillable: {estimate['fully_fillable']}")
        
        print(f"\n⚠️ WARNING: This will place a real market buy order!")
        response = input("Do you want to place a real order? (yes/no): ").lower()
        
        if response == "yes":
            amount = input(f"Enter amount in USD (or press Enter for $1): ").strip()
            if not amount:
                amount = "1.0"
            
            try:
                amount_float = float(amount)
                print(f"\n2️⃣ Placing market buy order for ${amount_float}...")
                
                if amount_float <= 5.0:  # Small order
                    result = place_small_market_buy(token_id, amount_float)
                else:  # Larger order
                    result = place_market_buy_order(token_id, amount_float)
                
                print(f"✅ Order result: {result}")
                
            except ValueError:
                print("❌ Invalid amount entered")
                
        else:
            print("❌ Market buy order cancelled")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
"""
Place market sell orders on Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, MarketOrderArgs, OrderType
from py_clob_client.constants import POLYGON
from py_clob_client.order_builder.constants import SELL

# Load from config folder
load_dotenv("config/.env")


def place_market_sell_order(token_id: str, amount: float) -> Dict[str, Any]:
    """
    Place a market sell order
    
    Args:
        token_id: Token to sell
        amount: Amount of tokens to sell
        
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
        
        print(f"💰 Placing market sell order...")
        print(f"   Token ID: {token_id[:20]}...")
        print(f"   Amount: {amount} tokens")
        
        # Create market order
        market_order_args = MarketOrderArgs(
            token_id=token_id,
            amount=amount,
            side=SELL,
        )
        
        # Create and sign the order
        signed_order = client.create_market_order(market_order_args)
        
        # Place the order
        response = client.post_order(signed_order, orderType=OrderType.FOK)
        
        print("✅ Market sell order placed successfully!")
        return response
        
    except Exception as e:
        print(f"❌ Error placing market sell order: {e}")
        raise


def place_small_market_sell(token_id: str, token_amount: float = 10.0) -> Dict[str, Any]:
    """
    Place a small market sell order for testing
    
    Args:
        token_id: Token to sell
        token_amount: Small amount of tokens (default: 10)
        
    Returns:
        Order response
    """
    try:
        print(f"🧪 Placing small test market sell order for {token_amount} tokens")
        return place_market_sell_order(token_id, token_amount)
        
    except Exception as e:
        print(f"❌ Error placing small market sell order: {e}")
        raise


def estimate_market_sell_outcome(token_id: str, amount: float) -> Dict[str, Any]:
    """
    Estimate outcome of market sell order without placing it
    
    Args:
        token_id: Token to estimate
        amount: Amount of tokens to sell
        
    Returns:
        Dict with estimated outcome
    """
    try:
        from utils.market.get_orderbook import get_orderbook
        
        print(f"📊 Estimating market sell outcome for {amount} tokens...")
        
        orderbook = get_orderbook(token_id)
        bids = orderbook.get('bids', [])
        
        if not bids:
            raise ValueError("No bids available in orderbook")
        
        remaining_tokens = amount
        total_usd = 0
        avg_price = 0
        levels_used = 0
        
        for bid in bids:
            bid_price = float(bid['price'])
            bid_size = float(bid['size'])
            
            if remaining_tokens <= 0:
                break
                
            if remaining_tokens >= bid_size:
                # Take entire level
                total_usd += bid_price * bid_size
                remaining_tokens -= bid_size
                levels_used += 1
            else:
                # Partial level
                total_usd += bid_price * remaining_tokens
                remaining_tokens = 0
                levels_used += 1
        
        tokens_sold = amount - remaining_tokens
        if tokens_sold > 0:
            avg_price = total_usd / tokens_sold
        
        estimate = {
            'input_tokens': amount,
            'estimated_usd': round(total_usd, 2),
            'average_price': round(avg_price, 4),
            'levels_used': levels_used,
            'tokens_sold': round(tokens_sold, 4),
            'tokens_remaining': round(remaining_tokens, 4),
            'fully_fillable': remaining_tokens == 0,
            'slippage': round(((float(bids[0]['price']) / avg_price) - 1) * 100, 2) if bids and avg_price > 0 else 0
        }
        
        return estimate
        
    except Exception as e:
        print(f"❌ Error estimating market sell: {e}")
        raise


def get_sellable_positions() -> Dict[str, Any]:
    """
    Get positions that can be sold
    
    Returns:
        Dict with sellable positions
    """
    try:
        from utils.get_balance import get_balance
        
        print("📊 Checking sellable positions...")
        
        balance = get_balance()
        positions = {}
        
        # Extract token balances from balance data
        # This depends on the exact structure of the balance response
        if isinstance(balance, dict):
            for key, value in balance.items():
                if isinstance(value, (int, float)) and value > 0:
                    # Skip USDC and other base currencies
                    if key.lower() not in ['usdc', 'usd', 'total', 'available', 'locked']:
                        positions[key] = value
        
        print(f"✅ Found {len(positions)} sellable positions")
        return positions
        
    except Exception as e:
        print(f"❌ Error getting sellable positions: {e}")
        raise


def main():
    """Example usage"""
    print("💰 Market Sell Order Utility")
    print("=" * 40)
    
    # Example token ID
    token_id = "71321045679252212594626385532706912750332728571942532289631379312455583992563"
    
    try:
        # Check sellable positions first
        print("\n1️⃣ Checking sellable positions:")
        try:
            positions = get_sellable_positions()
            if positions:
                print("   Available positions:")
                for token, amount in positions.items():
                    print(f"      {token}: {amount}")
            else:
                print("   No sellable positions found")
        except Exception as e:
            print(f"   Error checking positions: {e}")
        
        # Estimate market sell
        print("\n2️⃣ Estimating market sell outcome:")
        test_amount = 50.0  # 50 tokens
        estimate = estimate_market_sell_outcome(token_id, test_amount)
        
        print(f"   Input tokens: {estimate['input_tokens']}")
        print(f"   Estimated USD: ${estimate['estimated_usd']}")
        print(f"   Average price: ${estimate['average_price']}")
        print(f"   Levels used: {estimate['levels_used']}")
        print(f"   Slippage: {estimate['slippage']}%")
        print(f"   Fully fillable: {estimate['fully_fillable']}")
        
        print(f"\n⚠️ WARNING: This will place a real market sell order!")
        response = input("Do you want to place a real sell order? (yes/no): ").lower()
        
        if response == "yes":
            amount = input(f"Enter token amount to sell (or press Enter for 10): ").strip()
            if not amount:
                amount = "10.0"
            
            try:
                amount_float = float(amount)
                print(f"\n3️⃣ Placing market sell order for {amount_float} tokens...")
                
                if amount_float <= 50.0:  # Small order
                    result = place_small_market_sell(token_id, amount_float)
                else:  # Larger order
                    result = place_market_sell_order(token_id, amount_float)
                
                print(f"✅ Order result: {result}")
                
            except ValueError:
                print("❌ Invalid amount entered")
                
        else:
            print("❌ Market sell order cancelled")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
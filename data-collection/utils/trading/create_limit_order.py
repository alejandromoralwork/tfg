"""
Create limit orders on Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, OrderArgs
from py_clob_client.constants import POLYGON
from py_clob_client.order_builder.constants import BUY, SELL

# Load from config folder
load_dotenv("config/.env")


def create_limit_order(token_id: str, price: float, size: int, side: str) -> Dict[str, Any]:
    """
    Create a limit order
    
    Args:
        token_id: Token to trade
        price: Price per token
        size: Number of tokens
        side: "BUY" or "SELL"
        
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
        
        print(f"📊 Creating {side} limit order...")
        print(f"   Token ID: {token_id[:20]}...")
        print(f"   Price: ${price}")
        print(f"   Size: {size} tokens")
        
        # Create limit order
        order_args = OrderArgs(
            price=price,
            size=size,
            side=side,
            token_id=token_id,
        )
        
        # Create and sign the order
        signed_order = client.create_order(order_args)
        
        # Place the order
        response = client.post_order(signed_order)
        
        print("✅ Limit order created successfully!")
        return response
        
    except Exception as e:
        print(f"❌ Error creating limit order: {e}")
        raise


def create_buy_limit_order(token_id: str, price: float, size: int) -> Dict[str, Any]:
    """
    Create a buy limit order
    
    Args:
        token_id: Token to buy
        price: Price per token
        size: Number of tokens
        
    Returns:
        Order response
    """
    return create_limit_order(token_id, price, size, BUY)


def create_sell_limit_order(token_id: str, price: float, size: int) -> Dict[str, Any]:
    """
    Create a sell limit order
    
    Args:
        token_id: Token to sell
        price: Price per token
        size: Number of tokens
        
    Returns:
        Order response
    """
    return create_limit_order(token_id, price, size, SELL)


def create_limit_order_with_validation(token_id: str, price: float, size: int, side: str) -> Dict[str, Any]:
    """
    Create limit order with price validation against current market
    
    Args:
        token_id: Token to trade
        price: Price per token
        size: Number of tokens
        side: "BUY" or "SELL"
        
    Returns:
        Order response with validation info
    """
    try:
        from utils.market.get_orderbook import get_orderbook
        
        # Get current market prices
        orderbook = get_orderbook(token_id)
        bids = orderbook.get('bids', [])
        asks = orderbook.get('asks', [])
        
        best_bid = float(bids[0]['price']) if bids else 0
        best_ask = float(asks[0]['price']) if asks else 0
        
        # Validate price
        validation = {
            'price_valid': True,
            'warning': None,
            'suggestion': None
        }
        
        if side.upper() == "BUY":
            if price > best_ask:
                validation['warning'] = f"Buy price ${price} higher than best ask ${best_ask} - will execute immediately"
            elif price < best_bid * 0.8:  # More than 20% below best bid
                validation['warning'] = f"Buy price ${price} significantly below market (best bid: ${best_bid})"
                validation['suggestion'] = f"Consider price closer to ${best_bid}"
        
        elif side.upper() == "SELL":
            if price < best_bid:
                validation['warning'] = f"Sell price ${price} lower than best bid ${best_bid} - will execute immediately"
            elif price > best_ask * 1.2:  # More than 20% above best ask
                validation['warning'] = f"Sell price ${price} significantly above market (best ask: ${best_ask})"
                validation['suggestion'] = f"Consider price closer to ${best_ask}"
        
        # Show validation results
        if validation['warning']:
            print(f"⚠️ Warning: {validation['warning']}")
            if validation['suggestion']:
                print(f"💡 Suggestion: {validation['suggestion']}")
        
        # Create the order
        response = create_limit_order(token_id, price, size, side)
        response['validation'] = validation
        
        return response
        
    except Exception as e:
        print(f"❌ Error creating validated limit order: {e}")
        raise


def create_bracket_orders(token_id: str, center_price: float, spread_pct: float, size: int) -> Dict[str, Any]:
    """
    Create bracket orders (buy below and sell above a center price)
    
    Args:
        token_id: Token to trade
        center_price: Center price for bracket
        spread_pct: Spread percentage (e.g., 2.0 for 2%)
        size: Size for each order
        
    Returns:
        Dict with both order responses
    """
    try:
        spread_multiplier = spread_pct / 100
        
        buy_price = center_price * (1 - spread_multiplier)
        sell_price = center_price * (1 + spread_multiplier)
        
        print(f"📊 Creating bracket orders around ${center_price}")
        print(f"   Buy at: ${buy_price:.4f}")
        print(f"   Sell at: ${sell_price:.4f}")
        print(f"   Spread: {spread_pct}%")
        
        # Create buy order
        buy_response = create_buy_limit_order(token_id, buy_price, size)
        
        # Create sell order
        sell_response = create_sell_limit_order(token_id, sell_price, size)
        
        bracket_result = {
            'center_price': center_price,
            'spread_percent': spread_pct,
            'buy_order': {
                'price': buy_price,
                'response': buy_response
            },
            'sell_order': {
                'price': sell_price,
                'response': sell_response
            }
        }
        
        print("✅ Bracket orders created successfully!")
        return bracket_result
        
    except Exception as e:
        print(f"❌ Error creating bracket orders: {e}")
        raise


def main():
    """Example usage"""
    print("📊 Create Limit Order Utility")
    print("=" * 40)
    
    # Example token ID
    token_id = "71321045679252212594626385532706912750332728571942532289631379312455583992563"
    
    try:
        # Get current market info first
        from utils.market.get_orderbook import analyze_orderbook
        
        print("\n1️⃣ Current market info:")
        analysis = analyze_orderbook(token_id)
        print(f"   Best Bid: ${analysis['best_bid']}")
        print(f"   Best Ask: ${analysis['best_ask']}")
        print(f"   Spread: {analysis['spread_percent']}%")
        
        print(f"\n⚠️ WARNING: This will place real limit orders!")
        response = input("Do you want to place limit orders? (yes/no): ").lower()
        
        if response == "yes":
            order_type = input("Order type: (1) Single buy, (2) Single sell, (3) Bracket orders: ").strip()
            
            if order_type == "1":
                # Single buy order
                price = input(f"Enter buy price (current best bid: ${analysis['best_bid']}): $").strip()
                size = input("Enter size (number of tokens): ").strip()
                
                try:
                    price_float = float(price)
                    size_int = int(size)
                    
                    result = create_limit_order_with_validation(token_id, price_float, size_int, "BUY")
                    print(f"✅ Buy order result: {result}")
                    
                except ValueError:
                    print("❌ Invalid price or size")
                    
            elif order_type == "2":
                # Single sell order
                price = input(f"Enter sell price (current best ask: ${analysis['best_ask']}): $").strip()
                size = input("Enter size (number of tokens): ").strip()
                
                try:
                    price_float = float(price)
                    size_int = int(size)
                    
                    result = create_limit_order_with_validation(token_id, price_float, size_int, "SELL")
                    print(f"✅ Sell order result: {result}")
                    
                except ValueError:
                    print("❌ Invalid price or size")
                    
            elif order_type == "3":
                # Bracket orders
                center_price = input(f"Enter center price (current midpoint: ${(analysis['best_bid'] + analysis['best_ask'])/2:.4f}): $").strip()
                spread_pct = input("Enter spread percentage (e.g., 2.0 for 2%): ").strip()
                size = input("Enter size for each order: ").strip()
                
                try:
                    center_float = float(center_price)
                    spread_float = float(spread_pct)
                    size_int = int(size)
                    
                    result = create_bracket_orders(token_id, center_float, spread_float, size_int)
                    print(f"✅ Bracket orders result: {result}")
                    
                except ValueError:
                    print("❌ Invalid values entered")
            else:
                print("❌ Invalid order type")
        else:
            print("❌ Limit order creation cancelled")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
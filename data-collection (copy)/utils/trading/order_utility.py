"""
Order Management Class for Polymarket Trading
Designed for use in trading strategies with state management
"""

import os
import sys
from dotenv import load_dotenv

load_dotenv("config/.env")

# Add split directory to path
current_dir = os.path.dirname(os.path.abspath(__file__))
split_dir = os.path.join(current_dir, "split")
sys.path.insert(0, split_dir)

from create_order import create_order as split_create_order
from buy import buy as split_buy
from sell import sell as split_sell
from cancel_order import cancel_order as split_cancel_order


def create_order(token_id: str, price: float, size: int, side: str):
    """Create and place an order using split utils"""
    return split_create_order(token_id, price, size, side)

def buy_tokens(token_id: str, price: float, size: int):
    """Buy tokens using split utils"""
    return split_buy(token_id, price, size)

def sell_tokens(token_id: str, price: float, size: int):
    """Sell tokens using split utils"""
    return split_sell(token_id, price, size)

def cancel_order(order_id: str):
    """Cancel order using split utils"""
    return split_cancel_order(order_id)



def main():
    """Example usage of split trading utilities"""
    print("🚀 Split Trading Utilities Example")
    print("=" * 50)
    # Example token ID
    token_id = "87073854310124528759506128171096701607709284910112533007376905018319069357459"
    # Buy example
    print("\n💰 Placing example buy order...")
    buy_result = buy_tokens(token_id, 0.01, 5)
    print(f"Buy result: {buy_result}")
    # Get balance
    print("\n📊 USDC Balance:")


if __name__ == "__main__":
    main()
"""
Sell tokens on Polymarket CLOB
"""
import os
import sys
current_dir = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, current_dir)

from create_order import create_order
from py_clob_client.order_builder.constants import SELL

def sell(token_id: str, price: float, size: int):
    """
    Sell tokens
    """
    return create_order(token_id, price, size, SELL)

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Sell Polymarket Tokens")
    parser.add_argument("token_id", type=str)
    parser.add_argument("price", type=float)
    parser.add_argument("size", type=int)
    args = parser.parse_args()
    result = sell(args.token_id, args.price, args.size)
    print(result)

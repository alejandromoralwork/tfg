"""
Create and place an order on Polymarket CLOB
"""
import os
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, OrderArgs
from py_clob_client.constants import POLYGON
from py_clob_client.order_builder.constants import BUY, SELL
from dotenv import load_dotenv
load_dotenv("config/.env")

def create_order(token_id: str, price: float, size: int, side: str):
    """
    Create and place an order
    Args:
        token_id: The token/market ID to trade
        price: Price per token
        size: Number of tokens to buy/sell
        side: BUY or SELL
    Returns:
        dict: Order response
    """
    host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
    key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
    creds = ApiCreds(
        api_key=os.getenv("CLOB_API_KEY"),
        api_secret=os.getenv("CLOB_SECRET"),
        api_passphrase=os.getenv("CLOB_PASS_PHRASE"),
    )
    client = ClobClient(host, key=key, chain_id=POLYGON, creds=creds)
    order_args = OrderArgs(
        price=price,
        size=size,
        side=side,
        token_id=token_id,
    )
    signed_order = client.create_order(order_args)
    response = client.post_order(signed_order)
    return response

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Create Polymarket Order")
    parser.add_argument("token_id", type=str)
    parser.add_argument("price", type=float)
    parser.add_argument("size", type=int)
    parser.add_argument("side", type=str, choices=[BUY, SELL])
    args = parser.parse_args()
    result = create_order(args.token_id, args.price, args.size, args.side)
    print(result)

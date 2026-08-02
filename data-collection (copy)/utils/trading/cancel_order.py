"""
Cancel an order on Polymarket CLOB
"""
import os
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON
from dotenv import load_dotenv
load_dotenv("config/.env")

def cancel_order(order_id: str):
    """
    Cancel an order
    """
    host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
    key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
    creds = ApiCreds(
        api_key=os.getenv("CLOB_API_KEY"),
        api_secret=os.getenv("CLOB_SECRET"),
        api_passphrase=os.getenv("CLOB_PASS_PHRASE"),
    )
    client = ClobClient(host, key=key, chain_id=POLYGON, creds=creds)
    response = client.cancel_order(order_id)
    return response

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Cancel Polymarket Order")
    parser.add_argument("order_id", type=str)
    args = parser.parse_args()
    result = cancel_order(args.order_id)
    print(result)

"""
Get wallet address from environment or derive from private key
"""
import os
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON

def get_wallet_address() -> str:
    """
    Get wallet address from environment or derive from private key
    Returns:
        str: Wallet address
    """
    address = os.getenv("FUNDER")
    if address:
        return address
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
        if not key:
            raise ValueError("No wallet address or private key found")
        creds = ApiCreds(
            api_key=os.getenv("CLOB_API_KEY"),
            api_secret=os.getenv("CLOB_SECRET"),
            api_passphrase=os.getenv("CLOB_PASS_PHRASE"),
        )
        client = ClobClient(host, key=key, chain_id=POLYGON, creds=creds)
        address = client.get_address()
        return address
    except Exception as e:
        raise ValueError(f"Could not determine wallet address: {e}")

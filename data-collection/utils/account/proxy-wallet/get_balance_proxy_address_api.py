"""
Get account balance via Polymarket API
"""
import os
from pathlib import Path
from typing import Dict, Any
from dotenv import load_dotenv
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, BalanceAllowanceParams, AssetType
from py_clob_client.constants import POLYGON

# Load environment variables from PMTerminal/config/.env
# Go up from proxy-wallet -> account -> utils -> PMTerminal
pmterminal_root = Path(__file__).parent.parent.parent.parent
env_path = pmterminal_root / "config" / ".env"
load_dotenv(env_path)

def get_api_balance() -> Dict[str, Any]:
    """
    Get account balance via Polymarket API
    Returns:
        Dict with balance information from API
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
        print("💰 Fetching account balance via API...")
        collateral_balance = client.get_balance_allowance(
            params=BalanceAllowanceParams(asset_type=AssetType.COLLATERAL)
        )
        print("✅ API balance retrieved successfully")
        return collateral_balance
    except Exception as e:
        print(f"❌ Error getting API balance: {e}")
        raise

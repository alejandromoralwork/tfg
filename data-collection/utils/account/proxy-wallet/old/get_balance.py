"""
USDC Balance Utility for Polygon
Get USDC balance directly from blockchain - no API needed
Always uses POLYGON mainnet and loads from config/.env
"""

import os
from typing import Dict, Any, Optional
from web3 import Web3
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, BalanceAllowanceParams, AssetType
from py_clob_client.constants import POLYGON

# Load environment variables from config folder
load_dotenv("config/.env")

# USDC Contract address on Polygon mainnet
USDC_CONTRACT_ADDRESS = "0x2791bca1f2de4661ed88a30c99a7a9449aa84174"

# Polygon RPC URL
RPC_URL = os.getenv("POLYGON_RPC_URL", "https://polygon-rpc.com")

# USDC ABI - just the balanceOf function
USDC_ABI = [
    {
        "constant": True,
        "inputs": [{"name": "_owner", "type": "address"}],
        "name": "balanceOf",
        "outputs": [{"name": "balance", "type": "uint256"}],
        "type": "function"
    }
]


# =============================================================================
# API-Based Balance Functions (using Polymarket CLOB API)
# =============================================================================

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
        
        # Get collateral balance (USDC)
        collateral_balance = client.get_balance_allowance(
            params=BalanceAllowanceParams(asset_type=AssetType.COLLATERAL)
        )
        
        print("✅ API balance retrieved successfully")
        return collateral_balance
        
    except Exception as e:
        print(f"❌ Error getting API balance: {e}")
        raise


def get_wallet_address() -> str:
    """
    Get wallet address from environment or derive from private key
    
    Returns:
        str: Wallet address
    """
    # First try FUNDER address
    address = os.getenv("FUNDER")
    if address:
        return address
    
    # If no FUNDER, try to derive from private key
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


# =============================================================================
# Blockchain-Based Balance Functions (direct from Polygon)
# =============================================================================


def get_usdc_balance(address: str) -> int:
    """
    Get USDC balance for an address on Polygon mainnet
    
    Args:
        address: Wallet address to check
    
    Returns:
        int: USDC balance in cents (multiply by 100 to avoid decimals)
               Example: 1.50 USDC returns 150
    
    Raises:
        Exception: If network connection or contract call fails
    """
    try:
        # Connect to Polygon network
        w3 = Web3(Web3.HTTPProvider(RPC_URL))
        
        if not w3.is_connected():
            raise Exception("Failed to connect to Polygon network")
        
        # Create USDC contract instance
        usdc_contract = w3.eth.contract(
            address=Web3.to_checksum_address(USDC_CONTRACT_ADDRESS),
            abi=USDC_ABI
        )
        
        # Get balance (returns in wei with 6 decimals for USDC)
        balance_wei = usdc_contract.functions.balanceOf(
            Web3.to_checksum_address(address)
        ).call()
        
        # Convert to cents (integer) to avoid floating point issues
        # USDC has 6 decimals, so divide by 10^4 to get cents
        balance_cents = balance_wei // 10000
        
        return balance_cents
        
    except Exception as e:
        raise Exception(f"Failed to get USDC balance: {e}")


def get_my_balance() -> int:
    """
    Get USDC balance for the wallet address from config/.env
    
    Returns:
        int: USDC balance in cents
    
    Raises:
        Exception: If wallet address cannot be determined or balance check fails
    """
    try:
        wallet_address = get_wallet_address()
        return get_usdc_balance(wallet_address)
    except Exception as e:
        raise Exception(f"Failed to get wallet balance: {e}")


def format_balance(balance_cents: int) -> str:
    """
    Format balance from cents to human-readable string
    
    Args:
        balance_cents: Balance in cents
        
    Returns:
        str: Formatted balance (e.g., "1.50 USDC")
    """
    usdc_amount = balance_cents / 100
    return f"{usdc_amount:.2f} USDC"


def get_balance() -> Dict[str, Any]:
    """
    Get comprehensive balance information using POLYGON as primary source
    Combines API and blockchain data for accuracy
    
    Returns:
        Dict with comprehensive balance information
    """
    try:
        print("💰 Getting comprehensive balance information...")
        
        # Get wallet address
        wallet_address = get_wallet_address()
        print(f"📧 Wallet: {wallet_address}")
        
        # Method 1: API Balance (Polymarket's view)
        try:
            api_balance = get_api_balance()
            api_balance_value = float(api_balance.get('balance', 0))
            api_allowance = float(api_balance.get('allowance', 0))
        except Exception as e:
            print(f"⚠️ API balance failed: {e}")
            api_balance_value = 0
            api_allowance = 0
            api_balance = {}
        
        # Method 2: Blockchain Balance (Direct from Polygon - most accurate)
        try:
            blockchain_balance_cents = get_usdc_balance(wallet_address)
            blockchain_balance_usdc = blockchain_balance_cents / 100
        except Exception as e:
            print(f"⚠️ Blockchain balance failed: {e}")
            blockchain_balance_cents = 0
            blockchain_balance_usdc = 0
        
        # Create comprehensive summary
        balance_summary = {
            'wallet_address': wallet_address,
            'primary_balance_usdc': blockchain_balance_usdc,  # Use blockchain as primary
            'api_balance': api_balance_value,
            'api_allowance': api_allowance,
            'blockchain_balance_usdc': blockchain_balance_usdc,
            'blockchain_balance_cents': blockchain_balance_cents,
            'balance_match': abs(blockchain_balance_usdc - api_balance_value) < 0.01,
            'raw_api_data': api_balance
        }
        
        print("✅ Comprehensive balance retrieved")
        return balance_summary
        
    except Exception as e:
        print(f"❌ Error getting balance: {e}")
        raise


def get_balance_summary() -> Dict[str, Any]:
    """
    Get formatted balance summary (backwards compatibility)
    
    Returns:
        Dict with formatted balance summary
    """
    balance_data = get_balance()
    
    return {
        'balance': round(balance_data['primary_balance_usdc'], 2),
        'allowance': round(balance_data.get('api_allowance', 0), 2),
        'raw_balance': balance_data
    }


def main():
    """Example usage showing comprehensive balance checking"""
    print("💰 Comprehensive Balance Utility")
    print("=" * 50)
    
    try:
        # Get comprehensive balance
        balance_data = get_balance()
        
        print(f"\n� Balance Summary:")
        print(f"   Primary Balance: ${balance_data['primary_balance_usdc']:.2f} USDC")
        print(f"   API Balance: ${balance_data['api_balance']:.2f} USDC")
        print(f"   API Allowance: ${balance_data['api_allowance']:.2f} USDC")
        print(f"   Blockchain Balance: ${balance_data['blockchain_balance_usdc']:.2f} USDC")
        print(f"   Balances Match: {'✅' if balance_data['balance_match'] else '❌'}")
        
        print(f"\n💼 Wallet Info:")
        print(f"   Address: {balance_data['wallet_address']}")
        
        if not balance_data['balance_match']:
            print(f"\n⚠️ Warning: API and blockchain balances don't match!")
            print(f"   Using blockchain balance as primary (more accurate)")
        
        # Legacy format for backwards compatibility
        print(f"\n📋 Legacy Format:")
        legacy_summary = get_balance_summary()
        print(f"   Balance: ${legacy_summary['balance']}")
        print(f"   Allowance: ${legacy_summary['allowance']}")
        
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
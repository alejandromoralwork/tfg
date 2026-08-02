"""
Get USDC balance for an address on Polygon mainnet
"""
import os
from web3 import Web3
from typing import Any
USDC_CONTRACT_ADDRESS = os.getenv("USDC_CONTRACT_ADDRESS")
USDC_BRIDGED_CONTRACT_ADDRESS = os.getenv("USDC_BRIDGED_CONTRACT_ADDRESS")
RPC_URL = os.getenv("POLYGON_RPC_URL", "https://polygon-rpc.com")
USDC_ABI = [
    {
        "constant": True,
        "inputs": [{"name": "_owner", "type": "address"}],
        "name": "balanceOf",
        "outputs": [{"name": "balance", "type": "uint256"}],
        "type": "function"
    }
]

def get_usdc_balance(address: str) -> int:
    """
    Get USDC balance for an address on Polygon mainnet
    Args:
        address: Wallet address to check
    Returns:
        int: USDC balance in cents (multiply by 100 to avoid decimals)
    Raises:
        Exception: If network connection or contract call fails
    """
    try:
        w3 = Web3(Web3.HTTPProvider(RPC_URL))
        if not w3.is_connected():
            raise Exception("Failed to connect to Polygon network")
        usdc_contract = w3.eth.contract(
            address=Web3.to_checksum_address(USDC_CONTRACT_ADDRESS),
            abi=USDC_ABI
        )
        balance_wei = usdc_contract.functions.balanceOf(
            Web3.to_checksum_address(address)
        ).call()
        balance_cents = balance_wei // 10000
        return balance_cents
    except Exception as e:
        raise Exception(f"Failed to get USDC balance: {e}")

#!/usr/bin/env python3
"""
Comprehensive USDC Checker
Check both USDC and USDC.e balances on derived address
"""

import os
from pathlib import Path
from typing import Optional
from web3 import Web3
from dotenv import load_dotenv

# Load .env file from PMTerminal/config directory
# Go up from derived-wallet -> account -> utils -> PMTerminal
pmterminal_root = Path(__file__).parent.parent.parent.parent
env_path = pmterminal_root / "config" / ".env"
load_dotenv(env_path)

def get_balance_derived_address(token_type: Optional[str] = None) -> float:
    """
    Get the balance for the derived address.
    Args:
        token_type: "USDC" for USDC (native + bridged), "POL" for POL/MATIC.
                    Defaults to "USDC" when not specified or empty.
    Returns:
        float: Balance in USDC or POL
    """
    # Default to USDC if not provided
    if not token_type:
        token_type = "USDC"
    
    private_key = os.getenv("PRIVATE_KEY")
    if not private_key:
        raise ValueError(
            "PRIVATE_KEY not found in environment variables. "
            f"Please ensure config/.env exists at: {env_path}"
        )
    
    web3 = Web3(Web3.HTTPProvider("https://polygon-rpc.com"))
    account = web3.eth.account.from_key(private_key)
    derived_address = account.address

    if token_type.upper() == "POL":
        # Return POL/MATIC balance
        try:
            matic_balance = web3.eth.get_balance(derived_address) / 10**18
            return matic_balance
        except Exception as e:
            print(f"❌ POL/MATIC check failed: {e}")
            return 0.0
    elif token_type.upper() == "USDC":
        # USDC contract addresses on Polygon
        usdc_contracts = [
            "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",  # USDC (native)
            "0xA0b86a33E6c6936C0a94b4e7B8FA4d3b5e76E57E"   # USDC.e (bridged)
        ]
        erc20_abi = [
            {
                "constant": True,
                "inputs": [{"name": "_owner", "type": "address"}],
                "name": "balanceOf",
                "outputs": [{"name": "balance", "type": "uint256"}],
                "payable": False,
                "stateMutability": "view",
                "type": "function"
            }
        ]
        total_usdc = 0.0
        for contract_address in usdc_contracts:
            try:
                token_contract = web3.eth.contract(
                    address=Web3.to_checksum_address(contract_address),
                    abi=erc20_abi
                )
                balance_wei = token_contract.functions.balanceOf(
                    Web3.to_checksum_address(derived_address)
                ).call()
                balance_usdc = balance_wei / 10**6
                total_usdc += balance_usdc
            except Exception:
                pass  # Silently ignore errors for each contract
        return total_usdc
    else:
        raise ValueError("token_type must be 'USDC' or 'POL'")

if __name__ == "__main__":
    # Example usage
    print("USDC Balance:", get_balance_derived_address("USDC"))
    print("POL/MATIC Balance:", get_balance_derived_address("POL"))
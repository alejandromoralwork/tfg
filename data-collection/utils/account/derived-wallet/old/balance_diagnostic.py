#!/usr/bin/env python3
"""
Balance Diagnostic Script
========================
Compare API vs Blockchain balance and check allowances
"""

import os
import sys
from typing import Dict, Any

# Add parent directory to path for imports
sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds, BalanceAllowanceParams, AssetType
from dotenv import load_dotenv
from py_clob_client.constants import POLYGON
from web3 import Web3
import time

# Load environment variables from config/.env
load_dotenv("config/.env")


def check_blockchain_balance():
    """Check balance directly on blockchain"""
    try:
        print("🔍 Checking Blockchain Balance...")
        
        # POLYGON mainnet
        rpc_url = "https://polygon-rpc.com"
        web3 = Web3(Web3.HTTPProvider(rpc_url))
        
        # USDC contract on Polygon
        usdc_contract_address = Web3.to_checksum_address("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
        # Polymarket Exchange contract
        exchange_contract_address = Web3.to_checksum_address("0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E")
        
        # Your wallet
        funder_address = Web3.to_checksum_address("0xf937dbe9976ac34157b30dd55bdbf248458f6b43")
        
        # USDC ABI
        usdc_abi = [
            {
                "constant": True,
                "inputs": [
                    {"name": "owner", "type": "address"},
                    {"name": "spender", "type": "address"}
                ],
                "name": "allowance",
                "outputs": [{"name": "", "type": "uint256"}],
                "type": "function"
            },
            {
                "constant": True,
                "inputs": [{"name": "account", "type": "address"}],
                "name": "balanceOf",
                "outputs": [{"name": "", "type": "uint256"}],
                "type": "function"
            }
        ]
        
        usdc_contract = web3.eth.contract(address=usdc_contract_address, abi=usdc_abi)
        
        # Check USDC balance
        balance = usdc_contract.functions.balanceOf(funder_address).call()
        balance_usdc = balance / 1e6  # USDC has 6 decimals
        
        # Check allowance for Polymarket Exchange
        allowance = usdc_contract.functions.allowance(funder_address, exchange_contract_address).call()
        allowance_usdc = allowance / 1e6  # USDC has 6 decimals
        
        print(f"   Blockchain Balance: ${balance_usdc:.2f}")
        if allowance_usdc > 1000000:  # If very large number
            print(f"   Blockchain Allowance: UNLIMITED")
        else:
            print(f"   Blockchain Allowance: ${allowance_usdc:.2f}")
        
        return balance_usdc, allowance_usdc
        
    except Exception as e:
        print(f"   ❌ Blockchain Error: {e}")
        return 0, 0


def main():
    try:
        check_blockchain_balance()
    except Exception as e:
        print(f"❌ Diagnosis failed: {e}")

if __name__ == "__main__":
    main()
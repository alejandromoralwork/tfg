#!/usr/bin/env python3
"""
Balance Diagnostic Script
========================
Check balance directly on Polygon blockchain
"""

#OK

import os
import sys
import argparse
from pathlib import Path
from typing import Tuple
from dotenv import load_dotenv
from web3 import Web3

# Load environment variables from PMTerminal/config/.env
# Go up from proxy-wallet -> account -> utils -> PMTerminal
pmterminal_root = Path(__file__).parent.parent.parent.parent
env_path = pmterminal_root / "config" / ".env"
load_dotenv(env_path)


def check_blockchain_balance(
    address: str = None,
    rpc_url: str = None,
    verbose: bool = False
) -> Tuple[float, float]:
    """
    Check balance directly on blockchain
    
    Args:
        address: Wallet address to check (uses DERIVED from .env if not provided)
        rpc_url: Custom RPC URL (uses POLYGON_RPC_URL from .env or default if not provided)
        verbose: Enable verbose output
    
    Returns:
        Tuple[float, float]: (balance_usdc, allowance_usdc)
    """
    try:
        if verbose:
            print("🔍 Checking Blockchain Balance...")
        
        # Get RPC URL
        if not rpc_url:
            rpc_url = os.getenv("POLYGON_RPC_URL", "https://polygon-rpc.com")
        
        if verbose:
            print(f"   Using RPC: {rpc_url}")
        
        web3 = Web3(Web3.HTTPProvider(rpc_url))
        
        if not web3.is_connected():
            raise Exception(f"Failed to connect to Polygon network at {rpc_url}")
        
        if verbose:
            print(f"   ✅ Connected to Polygon (Chain ID: {web3.eth.chain_id})")
        
        # Get contract addresses from environment
        usdc_address = os.getenv("USDC_CONTRACT_ADDRESS", "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
        exchange_address = os.getenv("EXCHANGE_CONTRACT_ADDRESS", "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E")
        
        usdc_contract_address = Web3.to_checksum_address(usdc_address)
        exchange_contract_address = Web3.to_checksum_address(exchange_address)
        
        if verbose:
            print(f"   USDC Contract: {usdc_contract_address}")
            print(f"   Exchange Contract: {exchange_contract_address}")
        
        # Get wallet address
        if not address:
            # Try DERIVED first, then PROXY_ADDRESS, then FUNDER
            address = os.getenv("DERIVED") or os.getenv("PROXY_ADDRESS") or os.getenv("FUNDER")
            if not address:
                raise Exception(
                    f"No address provided and DERIVED/PROXY_ADDRESS/FUNDER not found in {env_path}\n"
                    "Please add DERIVED=0x..., PROXY_ADDRESS=0x..., or FUNDER=0x... to the .env file"
                )
        
        funder_address = Web3.to_checksum_address(address)
        
        if verbose:
            print(f"   Checking address: {funder_address}")

        # USDC ABI (minimal required functions)
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
        
        # Display results
        print(f"   💰 Blockchain Balance: ${balance_usdc:.2f}")
        
        if allowance_usdc > 1000000:  # If very large number (essentially unlimited)
            print(f"   ✅ Blockchain Allowance: UNLIMITED")
        else:
            print(f"   ✅ Blockchain Allowance: ${allowance_usdc:.2f}")
        
        return balance_usdc, allowance_usdc
        
    except Exception as e:
        print(f"   ❌ Blockchain Error: {e}")
        if verbose:
            import traceback
            traceback.print_exc()
        return 0, 0


def main():
    """Main function to run from command line"""
    parser = argparse.ArgumentParser(
        description='Check USDC balance on Polygon blockchain',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Check default address from .env
  python get_balance_proxy_address_blockchain.py
  
  # Check specific address
  python get_balance_proxy_address_blockchain.py --address 0x1234...
  
  # Use custom RPC
  python get_balance_proxy_address_blockchain.py --rpc https://polygon-mainnet.infura.io/v3/YOUR_KEY
  
  # Verbose output
  python get_balance_proxy_address_blockchain.py -v
  
  # JSON output
  python get_balance_proxy_address_blockchain.py --format json
        """
    )
    
    parser.add_argument(
        '--address',
        type=str,
        help='Wallet address to check (default: DERIVED from .env)'
    )
    
    parser.add_argument(
        '--rpc',
        type=str,
        help='Custom Polygon RPC URL (default: POLYGON_RPC_URL from .env or polygon-rpc.com)'
    )
    
    parser.add_argument(
        '--verbose', '-v',
        action='store_true',
        help='Enable verbose output with detailed information'
    )
    
    parser.add_argument(
        '--format',
        choices=['simple', 'json', 'table'],
        default='simple',
        help='Output format (default: simple)'
    )
    
    parser.add_argument(
        '--env-file',
        type=str,
        default=None,
        help='Path to custom .env file (default: PMTerminal/config/.env)'
    )
    
    args = parser.parse_args()
    
    # Load custom env file if specified
    if args.env_file:
        custom_env_path = Path(args.env_file)
        load_dotenv(custom_env_path)
        if args.verbose:
            print(f"📁 Loaded environment from: {custom_env_path.absolute()}")
    elif args.verbose:
        print(f"📁 Loaded environment from: {env_path.absolute()}")
    
    try:
        print("=" * 50)
        print("🔗 Polygon Blockchain Balance Check")
        print("=" * 50)
        
        balance, allowance = check_blockchain_balance(
            address=args.address,
            rpc_url=args.rpc,
            verbose=args.verbose
        )
        
        # Format output based on user preference
        if args.format == 'json':
            import json
            result = {
                "balance": balance,
                "allowance": allowance if allowance < 1000000 else "UNLIMITED",
                "currency": "USDC",
                "network": "Polygon"
            }
            print("\n" + json.dumps(result, indent=2))
            
        elif args.format == 'table':
            print("\n" + "=" * 50)
            print(f"{'Metric':<20} {'Value':>20}")
            print("-" * 50)
            print(f"{'Balance':<20} ${balance:>19.2f}")
            if allowance > 1000000:
                print(f"{'Allowance':<20} {'UNLIMITED':>20}")
            else:
                print(f"{'Allowance':<20} ${allowance:>19.2f}")
            print("=" * 50)
            
        else:  # simple
            print(f"\n✅ Balance check complete")
        
        # Exit code based on balance
        if balance > 0:
            sys.exit(0)
        else:
            if args.verbose:
                print("\n⚠️  Warning: Zero balance detected")
            sys.exit(1)
            
    except KeyboardInterrupt:
        print("\n\n⚠️  Interrupted by user")
        sys.exit(130)
        
    except Exception as e:
        print(f"\n❌ Fatal Error: {e}", file=sys.stderr)
        if args.verbose:
            import traceback
            traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
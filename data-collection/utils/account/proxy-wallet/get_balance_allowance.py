#!/usr/bin/env python3
"""
Enhanced Balance and Allowance Utility
=====================================
Get USDC balance and allowance for collateral and conditional tokens.
Includes wallet diagnostics and contract-level allowance checking.
Always uses POLYGON mainnet.

Updated with lessons on allowance setup:
- Wallet address verification
- Direct web3 contract checking
- Comprehensive error handling
"""

import os
import sys
from typing import Dict, Any, Optional
from web3 import Web3
from web3.constants import MAX_INT



import os
from pathlib import Path
from typing import Optional
from dotenv import load_dotenv
from enum import Enum


# Load .env file from PMTerminal/config directory
# Go up from derived-wallet -> account -> utils -> PMTerminal
pmterminal_root = Path(__file__).parent.parent.parent.parent
env_path = pmterminal_root / "config" / ".env"
load_dotenv(env_path)


# Contract addresses and ABIs
USDC_ADDRESS = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"  # USDC on Polygon
CTF_ADDRESS = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"   # CTF Contract

# Polymarket exchange contracts
CONTRACTS = {
    "CTF_EXCHANGE": "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E",
    "NEG_RISK_CTF_EXCHANGE": "0xC5d563A36AE78145C45a50134d48A1215220f80a",
    "NEG_RISK_ADAPTER": "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296"
}

# Contract ABIs
ERC20_BALANCE_ABI = """[{
    "constant": true,
    "inputs": [{"name": "_owner", "type": "address"}],
    "name": "balanceOf",
    "outputs": [{"name": "balance", "type": "uint256"}],
    "type": "function"
}, {
    "constant": true,
    "inputs": [{"name": "_owner", "type": "address"}, {"name": "_spender", "type": "address"}],
    "name": "allowance",
    "outputs": [{"name": "", "type": "uint256"}],
    "type": "function"
}]"""

ERC1155_APPROVAL_ABI = """[{
    "inputs": [{"internalType": "address", "name": "account", "type": "address"}, {"internalType": "address", "name": "operator", "type": "address"}],
    "name": "isApprovedForAll",
    "outputs": [{"internalType": "bool", "name": "", "type": "bool"}],
    "stateMutability": "view",
    "type": "function"
}]"""

def get_wallet_info() -> Dict[str, str]:
    """Get wallet information and verify private key matches"""
    try:
        from eth_account import Account
        
        private_key = os.getenv("PRIVATE_KEY")
        funder_address = os.getenv("FUNDER")
        derived_address = os.getenv("DERIVED")
        
        if private_key:
            account = Account.from_key(private_key)
            actual_derived = account.address
        else:
            actual_derived = "No private key"
        
        return {
            "funder": funder_address,
            "derived_config": derived_address,
            "actual_derived": actual_derived,
            "private_key_match": actual_derived.lower() == (derived_address or "").lower() if actual_derived != "No private key" else False
        }
    except Exception as e:
        return {"error": str(e)}

def setup_web3() -> Optional[Web3]:
    """Setup Web3 connection to Polygon"""
    try:
        rpc_urls = [
            "https://polygon-rpc.com",
            "https://rpc-mainnet.matic.quiknode.pro",
            "https://polygon.llamarpc.com"
        ]
        
        for rpc_url in rpc_urls:
            try:
                web3 = Web3(Web3.HTTPProvider(rpc_url))
                
                # Add POA middleware for Polygon
                try:
                    from web3.middleware import ExtraDataToPOAMiddleware
                    web3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
                except ImportError:
                    try:
                        from web3.middleware import geth_poa_middleware
                        web3.middleware_onion.inject(geth_poa_middleware, layer=0)
                    except ImportError:
                        pass
                
                if web3.is_connected() and web3.eth.chain_id == 137:
                    return web3
            except Exception:
                continue
        return None
    except Exception as e:
        print(f"❌ Web3 setup failed: {e}")
        return None

def get_direct_balances(wallet_address: str) -> Dict[str, Any]:
    """Get balances directly from blockchain"""
    web3 = setup_web3()
    if not web3:
        return {"error": "Could not connect to Polygon"}
    
    try:
        # Get POL balance
        pol_balance_wei = web3.eth.get_balance(wallet_address)
        pol_balance = web3.from_wei(pol_balance_wei, 'ether')
        
        # Get USDC balance
        usdc_contract = web3.eth.contract(address=USDC_ADDRESS, abi=ERC20_BALANCE_ABI)
        usdc_balance_raw = usdc_contract.functions.balanceOf(wallet_address).call()
        usdc_balance = usdc_balance_raw / 1e6  # USDC has 6 decimals
        
        return {
            "pol_balance": float(pol_balance),
            "usdc_balance": float(usdc_balance),
            "usdc_balance_raw": usdc_balance_raw,
            "wallet": wallet_address
        }
    except Exception as e:
        return {"error": str(e)}

def check_contract_allowances(wallet_address: str) -> Dict[str, Any]:
    """Check allowances for all Polymarket contracts"""
    web3 = setup_web3()
    if not web3:
        return {"error": "Could not connect to Polygon"}
    
    try:
        usdc_contract = web3.eth.contract(address=USDC_ADDRESS, abi=ERC20_BALANCE_ABI)
        ctf_contract = web3.eth.contract(address=CTF_ADDRESS, abi=ERC1155_APPROVAL_ABI)
        
        allowances = {}
        
        for contract_name, contract_address in CONTRACTS.items():
            # Check USDC allowance
            usdc_allowance = usdc_contract.functions.allowance(wallet_address, contract_address).call()
            usdc_unlimited = usdc_allowance >= (2**255)  # Check if essentially unlimited
            
            # Check CTF approval
            ctf_approved = ctf_contract.functions.isApprovedForAll(wallet_address, contract_address).call()
            
            allowances[contract_name] = {
                "address": contract_address,
                "usdc_allowance": usdc_allowance,
                "usdc_unlimited": usdc_unlimited,
                "ctf_approved": ctf_approved,
                "ready_to_trade": usdc_unlimited and ctf_approved
            }
        
        return allowances
    except Exception as e:
        return {"error": str(e)}

# NOTE: API balance/allowance checking is unreliable and removed
# The API often shows incorrect balances due to wallet address mismatches
# Use direct blockchain checking instead

def get_balance_allowance(asset_type: AssetType, token_id: Optional[str] = None) -> Dict[str, Any]:
    """
    DEPRECATED: API balance checking is unreliable
    Use direct blockchain checking instead via get_direct_balances()
    """
    print("⚠️ API balance checking is unreliable - use direct blockchain checking")
    return {"error": "Use direct blockchain checking instead"}

def get_collateral_balance_allowance() -> Dict[str, Any]:
    """DEPRECATED: Use direct blockchain checking"""
    return get_balance_allowance(AssetType.COLLATERAL)

def get_conditional_balance_allowance(token_id: str) -> Dict[str, Any]:
    """DEPRECATED: Use direct blockchain checking"""
    return get_balance_allowance(AssetType.CONDITIONAL, token_id)

def check_all_balances() -> Dict[str, Any]:
    """Check all balance and allowance information with enhanced diagnostics"""
    try:
        print("💰 Enhanced Balance & Allowance Check")
        print("=" * 60)
        
        # 1. Wallet diagnostics
        print("🔍 1. WALLET DIAGNOSTICS")
        wallet_info = get_wallet_info()
        
        if "error" not in wallet_info:
            print(f"   FUNDER (config):     {wallet_info['funder']}")
            print(f"   DERIVED (config):    {wallet_info['derived_config']}")
            print(f"   ACTUAL (from key):   {wallet_info['actual_derived']}")
            print(f"   ✅ Match: {wallet_info['private_key_match']}")
            
            # Use the correct wallet address
            wallet_address = wallet_info['actual_derived']
        else:
            print(f"   ❌ Error: {wallet_info['error']}")
            return {'status': 'error', 'error': wallet_info['error']}
        
        print()
        
        # 2. Direct blockchain balances
        print("🔍 2. BLOCKCHAIN BALANCES")
        direct_balances = get_direct_balances(wallet_address)
        
        if "error" not in direct_balances:
            print(f"   POL Balance:  {direct_balances['pol_balance']:.6f} POL")
            print(f"   USDC Balance: {direct_balances['usdc_balance']:.6f} USDC")
            
            # Check if sufficient for trading
            has_gas = direct_balances['pol_balance'] > 0.01  # Need POL for gas
            has_usdc = direct_balances['usdc_balance'] > 0    # Need USDC to trade
            
            print(f"   💨 Gas (POL): {'✅' if has_gas else '❌'} {'Sufficient' if has_gas else 'Need more POL'}")
            print(f"   💰 USDC:     {'✅' if has_usdc else '❌'} {'Available' if has_usdc else 'No USDC found'}")
        else:
            print(f"   ❌ Error: {direct_balances['error']}")
        
        print()
        
        # 3. Contract allowances
        print("🔍 3. CONTRACT ALLOWANCES")
        allowances = check_contract_allowances(wallet_address)
        
        if "error" not in allowances:
            for contract_name, data in allowances.items():
                status = "✅ READY" if data['ready_to_trade'] else "❌ NEEDS SETUP"
                print(f"   {contract_name:<22} {status}")
                print(f"      USDC: {'✅ Unlimited' if data['usdc_unlimited'] else '❌ Not set'}")
                print(f"      CTF:  {'✅ Approved' if data['ctf_approved'] else '❌ Not set'}")
                print()
        else:
            print(f"   ❌ Error: {allowances['error']}")
        
        print()
        print("=" * 60)
        
        # Summary
        all_ready = True
        if "error" not in allowances:
            for data in allowances.values():
                if not data['ready_to_trade']:
                    all_ready = False
                    break
        
        if all_ready and "error" not in direct_balances and direct_balances['usdc_balance'] > 0:
            print("🎉 STATUS: READY TO TRADE!")
        else:
            print("⚠️  STATUS: SETUP REQUIRED")
            if "error" not in direct_balances and direct_balances['usdc_balance'] == 0:
                print("   - Transfer USDC to wallet")
            if not all_ready:
                print("   - Run: python utils/set_allowances.py")
        
        return {
            'wallet_info': wallet_info,
            'direct_balances': direct_balances,
            'allowances': allowances,
            'ready_to_trade': all_ready,
            'status': 'success'
        }
        
    except Exception as e:
        print(f"❌ Error checking balances: {e}")
        return {'status': 'error', 'error': str(e)}

def quick_allowance_check() -> bool:
    """Quick check if all allowances are set - returns True if ready to trade"""
    try:
        wallet_info = get_wallet_info()
        if "error" in wallet_info or not wallet_info.get('private_key_match'):
            return False
        
        wallet_address = wallet_info['actual_derived']
        allowances = check_contract_allowances(wallet_address)
        
        if "error" in allowances:
            return False
        
        # Check if all contracts are ready
        for data in allowances.values():
            if not data['ready_to_trade']:
                return False
        
        return True
    except:
        return False

def main():
    """Main function for command line usage"""
    import argparse
    
    parser = argparse.ArgumentParser(description='Enhanced balance and allowance checker')
    parser.add_argument('--token-id', help='Token ID for conditional token lookup')
    parser.add_argument('--collateral-only', action='store_true', help='Only check collateral (API method)')
    parser.add_argument('--quick-check', action='store_true', help='Quick allowance check (returns exit code)')
    parser.add_argument('--wallet-only', action='store_true', help='Only check wallet diagnostics')
    parser.add_argument('--direct-only', action='store_true', help='Only check direct blockchain balances')
    
    args = parser.parse_args()
    
    if args.quick_check:
        ready = quick_allowance_check()
        print(f"Ready to trade: {ready}")
        sys.exit(0 if ready else 1)
    elif args.wallet_only:
        print("🔍 Wallet Diagnostics Only")
        wallet_info = get_wallet_info()
        for key, value in wallet_info.items():
            print(f"   {key}: {value}")
    elif args.direct_only:
        print("🔍 Direct Blockchain Check")
        wallet_info = get_wallet_info()
        if "error" not in wallet_info and wallet_info.get('private_key_match'):
            wallet_address = wallet_info['actual_derived']
            balances = get_direct_balances(wallet_address)
            allowances = check_contract_allowances(wallet_address)
            print(f"Balances: {balances}")
            print(f"Allowances: {allowances}")
        else:
            print("❌ Wallet configuration error")
    elif args.token_id:
        print(f"⚠️ Token-specific checking not available - API methods deprecated")
        print("Use the main check for comprehensive blockchain analysis")
    elif args.collateral_only:
        print("⚠️ API collateral checking deprecated - use --direct-only instead")
        print("🔍 Switching to direct blockchain check...")
        wallet_info = get_wallet_info()
        if "error" not in wallet_info and wallet_info.get('private_key_match'):
            wallet_address = wallet_info['actual_derived']
            balances = get_direct_balances(wallet_address)
            print(f"Direct USDC Balance: {balances.get('usdc_balance', 'Error')} USDC")
        else:
            print("❌ Wallet configuration error")
    else:
        # Default: comprehensive check
        result = check_all_balances()

if __name__ == "__main__":
    main()
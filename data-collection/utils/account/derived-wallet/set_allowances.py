#!/usr/bin/env python3
"""
Set All Polymarket Allowances - Working Version
==============================================
Sets USDC and CTF allowances for all Polymarket contracts.
This fixes the "not enough balance/allowance" error!
Based on confirmed working script with web3 6.14.0.
Always uses POLYGON mainnet.
"""

import os
from web3 import Web3
from web3.constants import MAX_INT
from dotenv import load_dotenv

# Skip middleware for now - not always needed
try:
    from web3.middleware import geth_poa_middleware
except ImportError:
    geth_poa_middleware = None

# Load environment variables from config/.env
load_dotenv("config/.env")

# Use your correct Polymarket credentials
rpc_url = "https://polygon-rpc.com"  # Polygon mainnet RPC
priv_key = os.getenv("PRIVATE_KEY")
pub_key = os.getenv("DERIVED")  # Use the wallet that matches your private key

print(f"🔑 Using wallet: {pub_key}")
print(f"⚠️  Make sure this wallet has MATIC for gas and USDC!")

chain_id = 137  # Polygon mainnet

erc20_approve = """[{"constant": false,"inputs": [{"name": "_spender","type": "address" },{ "name": "_value", "type": "uint256" }],"name": "approve","outputs": [{ "name": "", "type": "bool" }],"payable": false,"stateMutability": "nonpayable","type": "function"}]"""
erc1155_set_approval = """[{"inputs": [{ "internalType": "address", "name": "operator", "type": "address" },{ "internalType": "bool", "name": "approved", "type": "bool" }],"name": "setApprovalForAll","outputs": [],"stateMutability": "nonpayable","type": "function"}]"""
                
usdc_address = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"  # USDC on Polygon
ctf_address = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045"   # CTF Contract

web3 = Web3(Web3.HTTPProvider(rpc_url))

# Add POA middleware for Polygon
try:
    from web3.middleware import ExtraDataToPOAMiddleware
    web3.middleware_onion.inject(ExtraDataToPOAMiddleware, layer=0)
except ImportError:
    # Fallback for older versions
    if geth_poa_middleware:
        web3.middleware_onion.inject(geth_poa_middleware, layer=0)

print("🚀 Setting All Polymarket Allowances")
print("=" * 50)
print(f"📧 Wallet: {pub_key}")
print(f"🌐 Network: Polygon Mainnet (Chain ID: {chain_id})")
print(f"💰 USDC Contract: {usdc_address}")
print(f"🎭 CTF Contract: {ctf_address}")
print()

nonce = web3.eth.get_transaction_count(pub_key)
print(f"📊 Starting nonce: {nonce}")

usdc = web3.eth.contract(address=usdc_address, abi=erc20_approve)
ctf = web3.eth.contract(address=ctf_address, abi=erc1155_set_approval)

# CTF Exchange
print("🔄 CTF Exchange - USDC Approval")
raw_usdc_approve_txn = usdc.functions.approve("0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E", int(MAX_INT, 0)
).build_transaction({
    "chainId": chain_id, 
    "from": pub_key, 
    "nonce": nonce,
    "gas": 100000,
    "gasPrice": web3.to_wei('30', 'gwei')
})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(raw_usdc_approve_txn, private_key=priv_key)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.raw_transaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print("✅ USDC approval done!")
print(f"🔗 Tx: {usdc_approve_tx_receipt.transactionHash.hex()}")

nonce = web3.eth.get_transaction_count(pub_key)

print("🔄 CTF Exchange - CTF Approval")
raw_ctf_approval_txn = ctf.functions.setApprovalForAll("0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E", True).build_transaction({
    "chainId": chain_id, 
    "from": pub_key, 
    "nonce": nonce,
    "gas": 100000,
    "gasPrice": web3.to_wei('30', 'gwei')
})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(raw_ctf_approval_txn, private_key=priv_key)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.raw_transaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print("✅ CTF approval done!")
print(f"🔗 Tx: {ctf_approval_tx_receipt.transactionHash.hex()}")

nonce = web3.eth.get_transaction_count(pub_key)

# Neg Risk CTF Exchange
print("🔄 Neg Risk CTF Exchange - USDC Approval")
raw_usdc_approve_txn = usdc.functions.approve("0xC5d563A36AE78145C45a50134d48A1215220f80a", int(MAX_INT, 0)
).build_transaction({
    "chainId": chain_id, 
    "from": pub_key, 
    "nonce": nonce,
    "gas": 100000,
    "gasPrice": web3.to_wei('30', 'gwei')
})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(raw_usdc_approve_txn, private_key=priv_key)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.raw_transaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print("✅ USDC approval done!")
print(f"🔗 Tx: {usdc_approve_tx_receipt.transactionHash.hex()}")

nonce = web3.eth.get_transaction_count(pub_key)

print("🔄 Neg Risk CTF Exchange - CTF Approval")
raw_ctf_approval_txn = ctf.functions.setApprovalForAll("0xC5d563A36AE78145C45a50134d48A1215220f80a", True).build_transaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(raw_ctf_approval_txn, private_key=priv_key)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.raw_transaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print("✅ CTF approval done!")
print(f"🔗 Tx: {ctf_approval_tx_receipt.transactionHash.hex()}")

nonce = web3.eth.get_transaction_count(pub_key)

# Neg Risk Adapter
print("🔄 Neg Risk Adapter - USDC Approval")
raw_usdc_approve_txn = usdc.functions.approve("0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296", int(MAX_INT, 0)
).build_transaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_usdc_approve_tx = web3.eth.account.sign_transaction(raw_usdc_approve_txn, private_key=priv_key)
send_usdc_approve_tx = web3.eth.send_raw_transaction(signed_usdc_approve_tx.raw_transaction)
usdc_approve_tx_receipt = web3.eth.wait_for_transaction_receipt(send_usdc_approve_tx, 600)
print("✅ USDC approval done!")
print(f"� Tx: {usdc_approve_tx_receipt.transactionHash.hex()}")

nonce = web3.eth.get_transaction_count(pub_key)

print("🔄 Neg Risk Adapter - CTF Approval")
raw_ctf_approval_txn = ctf.functions.setApprovalForAll("0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296", True).build_transaction({"chainId": chain_id, "from": pub_key, "nonce": nonce})
signed_ctf_approval_tx = web3.eth.account.sign_transaction(raw_ctf_approval_txn, private_key=priv_key)
send_ctf_approval_tx = web3.eth.send_raw_transaction(signed_ctf_approval_tx.raw_transaction)
ctf_approval_tx_receipt = web3.eth.wait_for_transaction_receipt(send_ctf_approval_tx, 600)
print("✅ CTF approval done!")
print(f"� Tx: {ctf_approval_tx_receipt.transactionHash.hex()}")

print()
print("🎉 ALL ALLOWANCES SET SUCCESSFULLY!")
print("💡 You should now be able to trade on Polymarket!")
print("🚀 Try running your order_utility.py again!")
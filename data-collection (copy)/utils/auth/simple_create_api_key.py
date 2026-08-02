import os
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON

load_dotenv("config/.env")


def main():
    host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
    key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
    
    if not key:
        print("❌ Error: No private key found in config/.env")
        print("💡 Make sure PRIVATE_KEY is set in config/.env")
        return
    
    # Ensure private key has 0x prefix
    if not key.startswith('0x'):
        key = '0x' + key
    
    print(f"🔑 Using private key: {key[:10]}...")
    print(f"🌐 Connecting to: {host}")
    
    chain_id = POLYGON
    client = ClobClient(host, key=key, chain_id=chain_id)

    try:
        print("📡 Testing basic connection...")
        ok_result = client.get_ok()
        print(f"✅ Basic connection successful: {ok_result}")
        
        print("🔐 Creating API key with L1 authentication...")
        result = client.create_api_key()
        print("✅ API key created successfully!")
        print("📋 Result:")
        print(result)
        
    except Exception as e:
        print(f"❌ Error: {e}")
        print("\n💡 This might be normal if:")
        print("1. You already have API keys created")
        print("2. Your wallet needs to be verified on Polymarket first")
        print("3. API key creation is restricted")
        print("\n🌐 Try creating keys manually at: https://polymarket.com/profile")


if __name__ == "__main__":
    main()
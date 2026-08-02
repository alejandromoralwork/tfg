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
    
    print(f"🔑 Using private key: {key[:10]}...")
    print(f"🌐 Connecting to: {host}")
    
    chain_id = POLYGON
    client = ClobClient(host, key=key, chain_id=chain_id)

    try:
        print("� First checking existing API keys...")
        existing_keys = client.get_api_keys()
        print("📋 Existing API keys:")
        print(existing_keys)
        
        if existing_keys:
            print("ℹ️  You already have API keys. Do you want to create more? (y/n)")
            # For now, let's just show existing keys
            return
        
        print("�📡 Creating new API key...")
        result = client.create_api_key()
        print("✅ API key created successfully!")
        print(result)
    except Exception as e:
        print(f"❌ Error: {e}")
        print("💡 Make sure your private key is valid and you have permission to create API keys")


if __name__ == "__main__":
    main()
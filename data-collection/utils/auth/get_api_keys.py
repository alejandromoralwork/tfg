"""
Get API keys from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import List, Dict, Any
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON

# Load from config folder
load_dotenv("config/.env")


def get_api_keys() -> List[Dict[str, Any]]:
    """
    Get all API keys for the account
    
    Returns:
        List of API key dictionaries
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
        
        print("🔑 Fetching API keys...")
        api_keys = client.get_api_keys()
        
        print(f"✅ Retrieved {len(api_keys)} API keys")
        return api_keys
        
    except Exception as e:
        print(f"❌ Error getting API keys: {e}")
        raise


def create_api_key() -> Dict[str, Any]:
    """
    Create a new API key
    
    Returns:
        New API key data
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
        
        print("🔑 Creating new API key...")
        new_api_key = client.create_api_key()
        
        print("✅ New API key created successfully")
        return new_api_key
        
    except Exception as e:
        print(f"❌ Error creating API key: {e}")
        raise


def analyze_api_keys() -> Dict[str, Any]:
    """
    Analyze existing API keys
    
    Returns:
        Dict with API key analysis
    """
    try:
        api_keys = get_api_keys()
        
        if not api_keys:
            return {
                'total_keys': 0,
                'active_keys': 0,
                'analysis': 'No API keys found'
            }
        
        total_keys = len(api_keys)
        active_keys = 0
        key_details = []
        
        for i, key_data in enumerate(api_keys, 1):
            # Extract key information
            key_id = key_data.get('id', f'Key_{i}')
            status = key_data.get('status', 'Unknown')
            created = key_data.get('created_at', 'Unknown')
            
            if status.lower() == 'active':
                active_keys += 1
            
            key_details.append({
                'key_id': key_id,
                'status': status,
                'created': created,
                'raw_data': key_data
            })
        
        analysis = {
            'total_keys': total_keys,
            'active_keys': active_keys,
            'inactive_keys': total_keys - active_keys,
            'key_details': key_details,
            'raw_keys': api_keys
        }
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing API keys: {e}")
        raise


def main():
    """Example usage"""
    print("🔑 Get API Keys Utility")
    print("=" * 40)
    
    try:
        # Get API key analysis
        print("\n1️⃣ API Keys analysis:")
        analysis = analyze_api_keys()
        
        print(f"   Total API Keys: {analysis['total_keys']}")
        print(f"   Active Keys: {analysis['active_keys']}")
        print(f"   Inactive Keys: {analysis['inactive_keys']}")
        
        # Show key details
        if analysis['key_details']:
            print(f"\n📋 Key Details:")
            for i, key_detail in enumerate(analysis['key_details'], 1):
                print(f"   {i}. ID: {key_detail['key_id']}")
                print(f"      Status: {key_detail['status']}")
                print(f"      Created: {key_detail['created']}")
        else:
            print(f"\n� No API keys found")
        
        # Option to create new key
        print(f"\n2️⃣ Create new API key:")
        response = input("Do you want to create a new API key? (yes/no): ").lower()
        
        if response == "yes":
            new_key = create_api_key()
            print(f"✅ New API key created:")
            print(f"   {new_key}")
        else:
            print("❌ API key creation cancelled")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()

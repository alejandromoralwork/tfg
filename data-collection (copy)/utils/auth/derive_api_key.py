"""
Derive API key from private key on Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any, Optional
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.constants import POLYGON

# Load from config folder
load_dotenv("config/.env")


def derive_api_key() -> Dict[str, Any]:
    """
    Derive API key from private key
    
    Returns:
        Dict with derived API key credentials
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
        
        if not key:
            raise ValueError("No private key found in config/.env")
        
        # Ensure private key has 0x prefix
        if not key.startswith('0x'):
            key = '0x' + key
        
        client = ClobClient(host, key=key, chain_id=POLYGON)
        
        print("🔑 Deriving API key from private key...")
        derived_key = client.derive_api_key()
        
        print("✅ API key derived successfully")
        return derived_key
        
    except Exception as e:
        print(f"❌ Error deriving API key: {e}")
        raise


def save_derived_credentials(api_creds: Dict[str, Any], env_file: str = "config/.env") -> bool:
    """
    Save derived credentials to .env file
    
    Args:
        api_creds: API credentials dict
        env_file: Path to .env file
        
    Returns:
        True if saved successfully
    """
    try:
        # Read existing .env content
        env_lines = []
        if os.path.exists(env_file):
            with open(env_file, 'r') as f:
                env_lines = f.readlines()
        
        # Prepare new credentials
        new_creds = {
            'CLOB_API_KEY': api_creds.get('key', ''),
            'CLOB_SECRET': api_creds.get('secret', ''),
            'CLOB_PASS_PHRASE': api_creds.get('passphrase', '')
        }
        
        # Update or add credentials
        for cred_name, cred_value in new_creds.items():
            if cred_value:
                found = False
                for i, line in enumerate(env_lines):
                    if line.startswith(f"{cred_name}="):
                        env_lines[i] = f"{cred_name}={cred_value}\n"
                        found = True
                        break
                
                if not found:
                    env_lines.append(f"{cred_name}={cred_value}\n")
        
        # Write back to file
        with open(env_file, 'w') as f:
            f.writelines(env_lines)
        
        print(f"✅ Credentials saved to {env_file}")
        return True
        
    except Exception as e:
        print(f"❌ Error saving credentials: {e}")
        return False


def create_api_key() -> Dict[str, Any]:
    """
    Create new API key
    
    Returns:
        Dict with new API key credentials
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
        
        if not key:
            raise ValueError("No private key found in config/.env")
        
        # Ensure private key has 0x prefix
        if not key.startswith('0x'):
            key = '0x' + key
        
        client = ClobClient(host, key=key, chain_id=POLYGON)
        
        print("🔑 Creating new API key...")
        new_key = client.create_api_key()
        
        print("✅ New API key created successfully")
        return new_key
        
    except Exception as e:
        print(f"❌ Error creating API key: {e}")
        raise


def get_or_create_api_key() -> Dict[str, Any]:
    """
    Try to derive existing API key, create new one if needed
    
    Returns:
        Dict with API key credentials
    """
    try:
        # First try to derive existing key
        try:
            derived_key = derive_api_key()
            if derived_key:
                print("📋 Using existing derived API key")
                return derived_key
        except Exception as e:
            print(f"ℹ️  No existing API key to derive: {e}")
        
        # If no existing key, create new one
        print("🔐 Creating new API key...")
        new_key = create_api_key()
        return new_key
        
    except Exception as e:
        print(f"❌ Error getting or creating API key: {e}")
        raise


def main():
    """Example usage"""
    print("🔑 Derive API Key Utility")
    print("=" * 40)
    
    try:
        # Check if we have credentials already
        existing_key = os.getenv("CLOB_API_KEY")
        if existing_key:
            print("✅ Found existing API key in config/.env")
            print(f"   API Key: {existing_key[:10]}...")
            response = input("Do you want to derive/create a new key anyway? (yes/no): ").lower()
            if response != "yes":
                print("❌ Operation cancelled")
                return
        
        print("\n1️⃣ Getting or creating API key:")
        api_creds = get_or_create_api_key()
        
        # Display credentials
        print(f"\n� API Credentials:")
        for key_name, value in api_creds.items():
            if value:
                display_value = f"{value[:10]}..." if len(str(value)) > 10 else value
                print(f"   {key_name}: {display_value}")
        
        # Option to save to .env
        print(f"\n2️⃣ Save to config/.env:")
        response = input("Do you want to save these credentials to config/.env? (yes/no): ").lower()
        
        if response == "yes":
            saved = save_derived_credentials(api_creds)
            if saved:
                print("✅ Credentials saved successfully!")
                print("� You can now use all authenticated API functions")
            else:
                print("❌ Failed to save credentials")
        else:
            print("❌ Credentials not saved")
            print("💡 You'll need to manually add these to config/.env:")
            for key_name, value in api_creds.items():
                if value:
                    print(f"   {key_name.upper()}={value}")
                    
    except Exception as e:
        print(f"❌ Error: {e}")
        print("\n💡 Troubleshooting steps:")
        print("1. Ensure your wallet is connected to Polymarket.com first")
        print("2. Make some trades or deposits to activate your account")
        print("3. Visit https://polymarket.com/profile to create keys manually")
        print("4. Check if your region/wallet is supported")


if __name__ == "__main__":
    main()
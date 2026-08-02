"""
Simple Authentication Wrapper for Polymarket CLOB
Clean, minimal authentication that loads from config/.env
"""

import os
from typing import Optional
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import AMOY, POLYGON

class SimplePolymarketAuth:
    """Simple authentication wrapper for Polymarket CLOB"""
    
    def __init__(self, chain_id: int = POLYGON):
        """
        Initialize authentication wrapper
        
        Args:
            chain_id: Blockchain network (POLYGON for mainnet, AMOY for testnet)
        """
        self.chain_id = chain_id
        self._load_credentials()
    
    def _load_credentials(self):
        """Load credentials from config/.env file"""
        
        # Load from config/.env file
        from pathlib import Path
        config_env = Path("config/.env")
        if config_env.exists():
            load_dotenv("config/.env")
        else:
            load_dotenv()  # fallback to root folder
        
        # Extract credentials
        self.host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        self.private_key = os.getenv("PRIVATE_KEY") or os.getenv("PK")
        self.funder_address = os.getenv("FUNDER")
        
        # Handle different possible environment variable names
        api_key = (os.getenv("api_key") or 
                  os.getenv("CLOB_API_KEY") or 
                  os.getenv("API_KEY"))
        
        api_secret = (os.getenv("api_secret") or 
                     os.getenv("CLOB_SECRET") or 
                     os.getenv("API_SECRET"))
        
        api_passphrase = (os.getenv("api_passphrase") or 
                         os.getenv("CLOB_PASS_PHRASE") or 
                         os.getenv("API_PASSPHRASE"))
        
        self.api_creds = ApiCreds(
            api_key=api_key,
            api_secret=api_secret,
            api_passphrase=api_passphrase,
        )
        
        # Validate credentials
        self._validate_credentials()
    
    def _validate_credentials(self):
        """Validate that all required credentials are present"""
        missing_vars = []
        
        # Check API credentials
        if not self.api_creds.api_key:
            missing_vars.append("api_key/CLOB_API_KEY")
        if not self.api_creds.api_secret:
            missing_vars.append("api_secret/CLOB_SECRET")
        if not self.api_creds.api_passphrase:
            missing_vars.append("api_passphrase/CLOB_PASS_PHRASE")
        
        # Check private key
        if not self.private_key:
            missing_vars.append("PRIVATE_KEY/PK")
        
        if missing_vars:
            raise ValueError(f"Missing required credentials: {', '.join(missing_vars)}")
    
    def get_client(self) -> ClobClient:
        """Get authenticated CLOB client"""
        return ClobClient(
            host=self.host,
            key=self.private_key,
            chain_id=self.chain_id,
            creds=self.api_creds
        )
    
    def test_connection(self) -> bool:
        """Test API connection"""
        try:
            client = self.get_client()
            result = client.get_ok()
            return result is not None
        except Exception as e:
            print(f"Connection test failed: {e}")
            return False


# Convenience functions for quick setup
def get_authenticated_client(chain_id: int = POLYGON) -> ClobClient:
    """
    Quick function to get an authenticated CLOB client
    
    Args:
        chain_id: Blockchain network (POLYGON for mainnet, AMOY for testnet)
    
    Returns:
        ClobClient: Authenticated client
    """
    auth = SimplePolymarketAuth(chain_id=chain_id)
    return auth.get_client()


def get_testnet_client() -> ClobClient:
    """Quick function to get an authenticated testnet client"""
    return get_authenticated_client(chain_id=AMOY)


def get_mainnet_client() -> ClobClient:
    """Quick function to get an authenticated mainnet client"""
    return get_authenticated_client(chain_id=POLYGON)


# Test functionality
if __name__ == "__main__":
    print("🔐 Simple Polymarket Authentication Test")
    print("=" * 50)
    
    try:
        print("📡 Testing authentication...")
        auth = SimplePolymarketAuth()
        
        if auth.test_connection():
            print("✅ Connection successful!")
            print(f"🔗 Host: {auth.host}")
            print(f"📍 Chain: {auth.chain_id}")
            print(f"💳 Funder: {auth.funder_address}")
        else:
            print("❌ Connection failed")
        
    except Exception as e:
        print(f"❌ Error: {e}")
        print("💡 Make sure you have valid credentials in config/.env")
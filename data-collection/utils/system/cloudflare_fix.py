"""
Cloudflare bypass utilities for Polymarket API
Patches the py-clob-client to use browser-like headers
"""

import sys
import os

# Add the py-clob-client to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'py-clob-client-main'))

def patch_user_agent():
    """
    Patch the py-clob-client to use a more realistic User-Agent
    """
    try:
        from py_clob_client.http_helpers import helpers
        
        # Store original function
        original_overload_headers = helpers.overloadHeaders
        
        def patched_overload_headers(method: str, headers: dict) -> dict:
            """Modified version with browser-like User-Agent"""
            if headers is None:
                headers = dict()
            
            # Use a realistic browser User-Agent instead of "py_clob_client"
            headers["User-Agent"] = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            
            headers["Accept"] = "*/*"
            headers["Connection"] = "keep-alive"
            headers["Content-Type"] = "application/json"
            
            # Add more browser-like headers
            headers["Accept-Language"] = "en-US,en;q=0.9"
            headers["Cache-Control"] = "no-cache"
            headers["Pragma"] = "no-cache"
            headers["Sec-Fetch-Dest"] = "empty"
            headers["Sec-Fetch-Mode"] = "cors"
            headers["Sec-Fetch-Site"] = "cross-site"
            
            if method == "GET":
                headers["Accept-Encoding"] = "gzip, deflate, br"
            
            return headers
        
        # Replace the function
        helpers.overloadHeaders = patched_overload_headers
        print("✅ User-Agent patched to bypass Cloudflare")
        return True
        
    except Exception as e:
        print(f"❌ Failed to patch User-Agent: {e}")
        return False

def get_proxy_config():
    """
    Get proxy configuration from environment variables
    """
    proxy_config = {}
    
    http_proxy = os.getenv('HTTP_PROXY') or os.getenv('http_proxy')
    https_proxy = os.getenv('HTTPS_PROXY') or os.getenv('https_proxy')
    
    if http_proxy:
        proxy_config['http'] = http_proxy
    if https_proxy:
        proxy_config['https'] = https_proxy
    
    return proxy_config if proxy_config else None

def test_connection():
    """
    Test connection to Polymarket API
    """
    try:
        import requests
        
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        }
        
        proxy_config = get_proxy_config()
        
        response = requests.get(
            "https://clob.polymarket.com/time",
            headers=headers,
            proxies=proxy_config,
            timeout=10
        )
        
        if response.status_code == 200:
            print("✅ Connection to Polymarket API successful")
            return True
        else:
            print(f"❌ API returned status code: {response.status_code}")
            return False
            
    except Exception as e:
        print(f"❌ Connection test failed: {e}")
        return False

if __name__ == "__main__":
    print("🔧 Testing Cloudflare bypass...")
    
    # Test basic connection
    if test_connection():
        print("📡 API connection working")
    else:
        print("🚫 API connection blocked")
    
    # Patch User-Agent
    patch_user_agent()
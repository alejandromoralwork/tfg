#!/usr/bin/env python3
"""
Simple health check function for Gamma API
"""

import requests
from typing import bool


def get_ok() -> bool:
    """
    Health check for Gamma API - checks if the API is responding
    
    Returns:
        bool: True if API is healthy (returns "OK"), False otherwise
    
    Example:
        # Check if API is available
        if get_ok():
            print("API is healthy")
        else:
            print("API is down or not responding")
    """
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    
    # Root endpoint for health check
    url = f"{base_url}/"
    
    # Set up headers
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    try:
        # Make the request with a short timeout for quick health check
        response = requests.get(url, headers=headers, timeout=10)
        
        # Check if status is 200 and response contains "OK"
        if response.status_code == 200:
            try:
                # Try to parse as JSON
                data = response.json()
                # Check if response is "OK" (either string or in a dict)
                if data == "OK" or (isinstance(data, dict) and data.get("status") == "OK"):
                    return True
            except:
                # If not JSON, check if text response is "OK"
                if response.text.strip().upper() == "OK":
                    return True
        
        return False
        
    except requests.exceptions.Timeout:
        # API took too long to respond
        return False
    
    except requests.exceptions.RequestException:
        # Network error, connection refused, etc.
        return False
    
    except Exception:
        # Any other unexpected error
        return False


# Helper function with more detailed info (optional)
def get_api_status() -> dict:
    """
    Get detailed API status information
    
    Returns:
        dict: Detailed status information including response time and error details
    
    Example:
        status = get_api_status()
        print(f"Healthy: {status['healthy']}")
        print(f"Response time: {status['response_time_ms']}ms")
    """
    import time
    
    # Base URL for Gamma API
    base_url = "https://gamma-api.polymarket.com"
    url = f"{base_url}/"
    
    headers = {
        'User-Agent': 'PolyTrading/1.0',
        'Accept': 'application/json'
    }
    
    start_time = time.time()
    
    try:
        response = requests.get(url, headers=headers, timeout=10)
        response_time = (time.time() - start_time) * 1000  # Convert to milliseconds
        
        if response.status_code == 200:
            try:
                data = response.json()
                if data == "OK" or (isinstance(data, dict) and data.get("status") == "OK"):
                    return {
                        "healthy": True,
                        "status_code": 200,
                        "response_time_ms": round(response_time, 2),
                        "response": data,
                        "error": None
                    }
            except:
                if response.text.strip().upper() == "OK":
                    return {
                        "healthy": True,
                        "status_code": 200,
                        "response_time_ms": round(response_time, 2),
                        "response": response.text.strip(),
                        "error": None
                    }
        
        return {
            "healthy": False,
            "status_code": response.status_code,
            "response_time_ms": round(response_time, 2),
            "response": response.text[:100] if response.text else None,
            "error": f"Unexpected status code: {response.status_code}"
        }
        
    except requests.exceptions.Timeout:
        return {
            "healthy": False,
            "status_code": None,
            "response_time_ms": round((time.time() - start_time) * 1000, 2),
            "response": None,
            "error": "Request timeout (>10s)"
        }
    
    except requests.exceptions.RequestException as e:
        return {
            "healthy": False,
            "status_code": None,
            "response_time_ms": round((time.time() - start_time) * 1000, 2),
            "response": None,
            "error": f"Network error: {str(e)}"
        }
    
    except Exception as e:
        return {
            "healthy": False,
            "status_code": None,
            "response_time_ms": round((time.time() - start_time) * 1000, 2),
            "response": None,
            "error": f"Unexpected error: {str(e)}"
        }


# Example usage and quick test function
if __name__ == "__main__":
    print("Testing Gamma API Health Check")
    print("=" * 40)
    
    # Test 1: Simple boolean check
    print("1. Simple health check...")
    is_healthy = get_ok()
    print(f"   API Status: {'✅ HEALTHY' if is_healthy else '❌ UNHEALTHY'}")
    
    # Test 2: Detailed status
    print("\n2. Detailed health check...")
    status = get_api_status()
    print(f"   Healthy: {'✅ Yes' if status['healthy'] else '❌ No'}")
    print(f"   Status Code: {status['status_code']}")
    print(f"   Response Time: {status['response_time_ms']}ms")
    
    if status['error']:
        print(f"   Error: {status['error']}")
    else:
        print(f"   Response: {status['response']}")
    
    # Test 3: Usage in conditional logic
    print("\n3. Usage example...")
    if get_ok():
        print("   ✅ API is ready - safe to make other API calls")
    else:
        print("   ❌ API is down - avoid making API calls")
"""
Get server time from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any
from datetime import datetime
from dotenv import load_dotenv

from py_clob_client.client import ClobClient

# Load from config folder
load_dotenv("config/.env")


def get_server_time() -> Dict[str, Any]:
    """
    Get server time from Polymarket
    
    Returns:
        Dict with server time information
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print("🕐 Fetching server time...")
        server_time = client.get_server_time()
        
        print("✅ Server time retrieved successfully")
        return server_time
        
    except Exception as e:
        print(f"❌ Error getting server time: {e}")
        raise


def get_time_comparison() -> Dict[str, Any]:
    """
    Compare server time with local time
    
    Returns:
        Dict with time comparison data
    """
    try:
        server_time_data = get_server_time()
        
        # Extract server timestamp (handle both dict and direct timestamp)
        if isinstance(server_time_data, dict):
            server_timestamp = server_time_data.get('timestamp', 0)
        else:
            server_timestamp = server_time_data
        
        if isinstance(server_timestamp, str):
            server_timestamp = int(server_timestamp)
        
        # Convert to seconds if in milliseconds
        if server_timestamp > 10**12:  # If timestamp is in milliseconds
            server_timestamp = server_timestamp / 1000
        
        # Get local time
        local_timestamp = datetime.now().timestamp()
        
        # Calculate difference
        time_diff = abs(server_timestamp - local_timestamp)
        
        # Format times
        server_datetime = datetime.fromtimestamp(server_timestamp)
        local_datetime = datetime.fromtimestamp(local_timestamp)
        
        comparison = {
            'server_timestamp': server_timestamp,
            'local_timestamp': local_timestamp,
            'server_datetime': server_datetime.strftime('%Y-%m-%d %H:%M:%S'),
            'local_datetime': local_datetime.strftime('%Y-%m-%d %H:%M:%S'),
            'time_difference_seconds': round(time_diff, 2),
            'time_difference_ms': round(time_diff * 1000, 2),
            'clocks_synchronized': time_diff < 5,  # Within 5 seconds
            'raw_server_data': server_time_data
        }
        
        return comparison
        
    except Exception as e:
        print(f"❌ Error comparing times: {e}")
        raise


def check_connection() -> bool:
    """
    Check connection to Polymarket by getting server time
    
    Returns:
        True if connection successful, False otherwise
    """
    try:
        get_server_time()
        return True
    except Exception:
        return False


def main():
    """Example usage"""
    print("🕐 Get Server Time Utility")
    print("=" * 40)
    
    try:
        # Basic server time
        print("\n1️⃣ Server time:")
        server_time = get_server_time()
        print(f"   Raw data: {server_time}")
        
        # Time comparison
        print("\n2️⃣ Time comparison:")
        comparison = get_time_comparison()
        print(f"   Server time: {comparison['server_datetime']}")
        print(f"   Local time: {comparison['local_datetime']}")
        print(f"   Difference: {comparison['time_difference_seconds']} seconds")
        print(f"   Synchronized: {comparison['clocks_synchronized']}")
        
        # Connection check
        print("\n3️⃣ Connection check:")
        connected = check_connection()
        print(f"   Connection status: {'✅ Connected' if connected else '❌ Failed'}")
        
        if comparison['time_difference_seconds'] > 10:
            print("\n⚠️ WARNING: Large time difference detected!")
            print("   This might cause issues with order timestamps")
        
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
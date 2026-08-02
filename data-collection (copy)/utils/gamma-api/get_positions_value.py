"""
Get User Positions Value - Total USD value of user positions

Function to fetch the total USD value of a user's positions on Polymarket.
"""

import os
import sys

# Add the project root to the Python path
project_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, project_root)

import requests
from typing import Dict, List, Any, Optional, Union


def get_user_positions_value(
    user: str,
    market: Optional[Union[str, List[str]]] = None
) -> List[Dict[str, Any]]:
    """
    Fetch the total USD value of a user's positions
    
    Args:
        user: The address of the user (required)
        market: Market condition ID(s) - single string or list of strings (optional)
        
    Returns:
        List[Dict[str, Any]]: List containing user and their total position value
        
    Example:
        >>> # Get total value of all positions
        >>> value = get_user_positions_value("0x6af75d4e4aaf700450efbac3708cce1665810ff1")
        >>> print(f"Total portfolio value: ${value[0]['value']}")
        
        >>> # Get value for specific market(s)
        >>> market_value = get_user_positions_value(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     market="0x2c95c926e924f243aab41e96a90d22fcaf8cf273a678a07c49abb95fde489678"
        ... )
        
        >>> # Get value for multiple markets
        >>> multi_value = get_user_positions_value(
        ...     user="0x6af75d4e4aaf700450efbac3708cce1665810ff1",
        ...     market=["market1", "market2", "market3"]
        ... )
    """
    
    try:
        print(f"💰 Getting positions value for user: {user}")
        
        # Build query parameters
        params = {"user": user}
        
        # Handle market parameter (single or multiple)
        if market is not None:
            if isinstance(market, list):
                params["market"] = ",".join(market)
                print(f"   📊 Filtering by markets: {len(market)} markets")
            else:
                params["market"] = market
                print(f"   📊 Filtering by market: {market}")
        else:
            print(f"   📊 Getting total portfolio value")
        
        # Make API request
        base_url = "https://data-api.polymarket.com"
        url = f"{base_url}/value"
        
        print(f"📡 Making request to: {url}")
        print(f"📋 Parameters: {params}")
        
        response = requests.get(url, params=params)
        response.raise_for_status()
        
        value_data = response.json()
        
        if value_data and len(value_data) > 0:
            total_value = value_data[0].get('value', 0)
            print(f"✅ Portfolio value retrieved: ${total_value:.6f}")
            
            if total_value > 1000:
                print(f"   💎 High-value portfolio: ${total_value:.2f}")
            elif total_value > 100:
                print(f"   💰 Medium-value portfolio: ${total_value:.2f}")
            else:
                print(f"   💵 Small-value portfolio: ${total_value:.2f}")
        else:
            print(f"⚠️ No value data returned")
        
        return value_data
        
    except requests.exceptions.HTTPError as e:
        if e.response.status_code == 404:
            print(f"❌ User not found or no positions: {user}")
            return [{"user": user, "value": 0}]
        else:
            print(f"❌ HTTP error getting positions value: {e}")
            raise
    except requests.exceptions.RequestException as e:
        print(f"❌ Network error getting positions value: {e}")
        raise
    except Exception as e:
        print(f"❌ Unexpected error getting positions value: {e}")
        raise


def get_user_total_value(user: str) -> float:
    """
    Get just the total value as a float (convenience function)
    
    Args:
        user: User address
        
    Returns:
        float: Total portfolio value in USD
    """
    try:
        value_data = get_user_positions_value(user)
        if value_data and len(value_data) > 0:
            return float(value_data[0].get('value', 0))
        return 0.0
    except Exception:
        return 0.0


def get_user_market_value(user: str, market: str) -> float:
    """
    Get value for a specific market (convenience function)
    
    Args:
        user: User address
        market: Market condition ID
        
    Returns:
        float: Market-specific position value in USD
    """
    try:
        value_data = get_user_positions_value(user, market=market)
        if value_data and len(value_data) > 0:
            return float(value_data[0].get('value', 0))
        return 0.0
    except Exception:
        return 0.0


def compare_user_values(users: List[str]) -> List[Dict[str, Any]]:
    """
    Compare portfolio values across multiple users
    
    Args:
        users: List of user addresses
        
    Returns:
        List of user values sorted by value (highest first)
    """
    user_values = []
    
    print(f"🔍 Comparing portfolio values for {len(users)} users...")
    
    for i, user in enumerate(users, 1):
        try:
            print(f"   📊 ({i}/{len(users)}) Processing {user}")
            
            value_data = get_user_positions_value(user)
            if value_data and len(value_data) > 0:
                user_values.append({
                    'user': user,
                    'value': value_data[0].get('value', 0),
                    'status': 'success'
                })
                print(f"      ✅ Value: ${value_data[0].get('value', 0):.6f}")
            else:
                user_values.append({
                    'user': user,
                    'value': 0,
                    'status': 'no_data'
                })
                print(f"      ⚠️ No positions found")
        except Exception as e:
            print(f"      ❌ Error: {e}")
            user_values.append({
                'user': user,
                'value': 0,
                'status': 'error',
                'error': str(e)
            })
    
    # Sort by value (highest first)
    user_values.sort(key=lambda x: x['value'], reverse=True)
    
    print(f"✅ Comparison complete - {len(user_values)} users processed")
    return user_values


# Example usage and testing
if __name__ == "__main__":
    print("💰 Get User Positions Value - Testing\n")
    
    # Example user address from the API documentation
    test_user = "0x6af75d4e4aaf700450efbac3708cce1665810ff1"
    
    try:
        print(f"🔍 Testing positions value for: {test_user}")
        
        # Test 1: Get total portfolio value
        print(f"\n1️⃣ Total Portfolio Value:")
        value_data = get_user_positions_value(test_user)
        
        if value_data and len(value_data) > 0:
            total_value = value_data[0].get('value', 0)
            print(f"✅ Success!")
            print(f"   💰 Total Value: ${total_value}")
            print(f"   👤 User: {value_data[0].get('user', 'N/A')}")
            
            # Show value categories
            if total_value > 10000:
                print(f"   🏆 Whale status: Very high value portfolio")
            elif total_value > 1000:
                print(f"   🐋 High value portfolio")
            elif total_value > 100:
                print(f"   🐟 Medium value portfolio") 
            else:
                print(f"   🦐 Small value portfolio")
        else:
            print("   ⚠️ No value data returned")
        
        # Test 2: Convenience function
        print(f"\n2️⃣ Convenience Function Test:")
        simple_value = get_user_total_value(test_user)
        print(f"✅ Simple value: ${simple_value}")
        
        # Test 3: Test with multiple users (if you have more addresses)
        print(f"\n3️⃣ Multiple Users Comparison:")
        test_users = [test_user]  # Add more user addresses here for comparison
        
        if len(test_users) == 1:
            print("   💡 Add more user addresses to test comparison feature")
            print(f"   📊 Single user value: ${simple_value}")
        else:
            comparisons = compare_user_values(test_users)
            
            print(f"   🏆 Ranking by portfolio value:")
            for i, comp in enumerate(comparisons, 1):
                status_icon = "✅" if comp['status'] == 'success' else "⚠️"
                print(f"      {i}. {status_icon} {comp['user']}: ${comp['value']:.6f}")
        
        # Test 4: Value formatting examples
        if value_data and len(value_data) > 0:
            raw_value = value_data[0].get('value', 0)
            print(f"\n4️⃣ Value Formatting Examples:")
            print(f"   Raw value: {raw_value}")
            print(f"   2 decimals: ${raw_value:.2f}")
            print(f"   6 decimals: ${raw_value:.6f}")
            print(f"   Scientific: {raw_value:.2e}")
            
    except Exception as e:
        print(f"❌ Test failed: {e}")
        print(f"\n💡 Make sure to:")
        print(f"   1. Have valid user address")
        print(f"   2. Check network connection")
        print(f"   3. Verify Data API endpoint is accessible")
        print(f"   4. User might not have any positions")
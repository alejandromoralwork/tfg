"""
Get midpoint prices from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any, List
from dotenv import load_dotenv

from py_clob_client.client import ClobClient

# Load from config folder
load_dotenv("config/.env")


def get_midpoint(token_id: str) -> Dict[str, Any]:
    """
    Get midpoint price for a specific token
    
    Args:
        token_id: The token ID to get midpoint for
        
    Returns:
        Dict with midpoint price information
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Fetching midpoint for token: {token_id[:20]}...")
        midpoint = client.get_midpoint(token_id)
        
        print("✅ Midpoint retrieved successfully")
        return midpoint
        
    except Exception as e:
        print(f"❌ Error getting midpoint: {e}")
        raise


def get_multiple_midpoints(token_ids: List[str]) -> Dict[str, Any]:
    """
    Get midpoint prices for multiple tokens
    
    Args:
        token_ids: List of token IDs
        
    Returns:
        Dict with midpoint prices for all tokens
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"💰 Fetching midpoints for {len(token_ids)} tokens...")
        midpoints = client.get_midpoints(token_ids)
        
        print("✅ Midpoints retrieved successfully")
        return midpoints
        
    except Exception as e:
        print(f"❌ Error getting midpoints: {e}")
        raise


def compare_midpoints(token_ids: List[str]) -> List[Dict[str, Any]]:
    """
    Compare midpoint prices across tokens
    
    Args:
        token_ids: List of token IDs to compare
        
    Returns:
        List of midpoint comparisons sorted by price
    """
    try:
        comparisons = []
        
        for token_id in token_ids:
            try:
                midpoint_data = get_midpoint(token_id)
                midpoint = float(midpoint_data.get('mid', 0))
                
                comparison = {
                    'token_id': token_id,
                    'midpoint': round(midpoint, 4),
                    'implied_probability_pct': round(midpoint * 100, 2),
                    'raw_data': midpoint_data
                }
                
                comparisons.append(comparison)
                
            except Exception as e:
                print(f"⚠️ Failed to get midpoint for {token_id[:20]}...: {e}")
                continue
        
        # Sort by midpoint (highest first)
        comparisons.sort(key=lambda x: x['midpoint'], reverse=True)
        
        print(f"📊 Compared midpoints for {len(comparisons)} tokens")
        return comparisons
        
    except Exception as e:
        print(f"❌ Error comparing midpoints: {e}")
        raise


def main():
    """Example usage"""
    print("💰 Get Midpoint Utility")
    print("=" * 40)
    
    # Example token IDs
    token_ids = [
        "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "52114319501245915516055106046884209969926127482827954674443846427813813222426"
    ]
    
    try:
        # Single token midpoint
        print("\n1️⃣ Single token midpoint:")
        midpoint = get_midpoint(token_ids[0])
        print(f"   Midpoint data: {midpoint}")
        
        # Multiple token midpoints
        print(f"\n2️⃣ Multiple token midpoints:")
        try:
            midpoints = get_multiple_midpoints(token_ids)
            print(f"   Retrieved midpoints for {len(midpoints)} tokens")
            for token_id, data in midpoints.items():
                print(f"   {token_id[:20]}...: {data}")
        except Exception as e:
            print(f"   Multiple midpoints not available: {e}")
            # Fall back to individual calls
            for token_id in token_ids:
                try:
                    midpoint = get_midpoint(token_id)
                    print(f"   {token_id[:20]}...: {midpoint}")
                except Exception as e:
                    print(f"   Failed for {token_id[:20]}...: {e}")
        
        # Midpoint comparison
        print(f"\n3️⃣ Midpoint comparison:")
        comparisons = compare_midpoints(token_ids)
        
        for i, comp in enumerate(comparisons, 1):
            print(f"   {i}. {comp['token_id'][:20]}... - ${comp['midpoint']} ({comp['implied_probability_pct']}%)")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
"""
Get spread data from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any, List
from dotenv import load_dotenv

from py_clob_client.client import ClobClient

# Load from config folder
load_dotenv("config/.env")


def get_spread(token_id: str) -> Dict[str, Any]:
    """
    Get spread for a specific token
    
    Args:
        token_id: The token ID to get spread for
        
    Returns:
        Dict with spread information
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"📊 Fetching spread for token: {token_id[:20]}...")
        spread = client.get_spread(token_id)
        
        print("✅ Spread retrieved successfully")
        return spread
        
    except Exception as e:
        print(f"❌ Error getting spread: {e}")
        raise


def get_spreads(token_ids: List[str]) -> Dict[str, Any]:
    """
    Get spreads for multiple tokens
    
    Args:
        token_ids: List of token IDs
        
    Returns:
        Dict with spread information for all tokens
    """
    try:
        host = os.getenv("CLOB_API_URL", "https://clob.polymarket.com")
        client = ClobClient(host)
        
        print(f"📊 Fetching spreads for {len(token_ids)} tokens...")
        spreads = client.get_spreads(token_ids)
        
        print("✅ Spreads retrieved successfully")
        return spreads
        
    except Exception as e:
        print(f"❌ Error getting spreads: {e}")
        raise


def analyze_spread(token_id: str) -> Dict[str, Any]:
    """
    Analyze spread with additional calculations
    
    Args:
        token_id: The token ID to analyze
        
    Returns:
        Dict with detailed spread analysis
    """
    try:
        spread_data = get_spread(token_id)
        
        # Extract spread components
        bid = float(spread_data.get('bid', 0))
        ask = float(spread_data.get('ask', 0))
        
        # Calculate spread metrics
        absolute_spread = ask - bid
        relative_spread = (absolute_spread / bid * 100) if bid > 0 else 0
        midpoint = (bid + ask) / 2 if bid > 0 and ask > 0 else 0
        
        # Calculate market efficiency metrics
        efficiency_score = max(0, 100 - relative_spread)  # Higher is better
        liquidity_indicator = "High" if relative_spread < 2 else "Medium" if relative_spread < 5 else "Low"
        
        analysis = {
            'token_id': token_id,
            'bid': round(bid, 4),
            'ask': round(ask, 4),
            'absolute_spread': round(absolute_spread, 4),
            'relative_spread_pct': round(relative_spread, 2),
            'midpoint': round(midpoint, 4),
            'efficiency_score': round(efficiency_score, 1),
            'liquidity_indicator': liquidity_indicator,
            'tight_spread': relative_spread < 1,  # Less than 1% spread
            'raw_data': spread_data
        }
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing spread: {e}")
        raise


def compare_spreads(token_ids: List[str]) -> List[Dict[str, Any]]:
    """
    Compare spreads across multiple tokens
    
    Args:
        token_ids: List of token IDs to compare
        
    Returns:
        List of spread analyses sorted by efficiency
    """
    try:
        analyses = []
        
        for token_id in token_ids:
            try:
                analysis = analyze_spread(token_id)
                analyses.append(analysis)
            except Exception as e:
                print(f"⚠️ Failed to analyze spread for {token_id[:20]}...: {e}")
                continue
        
        # Sort by efficiency score (highest first)
        analyses.sort(key=lambda x: x['efficiency_score'], reverse=True)
        
        print(f"📊 Compared spreads for {len(analyses)} tokens")
        return analyses
        
    except Exception as e:
        print(f"❌ Error comparing spreads: {e}")
        raise


def find_tight_spreads(token_ids: List[str], max_spread_pct: float = 1.0) -> List[Dict[str, Any]]:
    """
    Find tokens with tight spreads
    
    Args:
        token_ids: List of token IDs to check
        max_spread_pct: Maximum spread percentage to consider "tight"
        
    Returns:
        List of tokens with tight spreads
    """
    try:
        tight_spreads = []
        
        print(f"🔍 Finding tokens with spreads < {max_spread_pct}%...")
        
        for token_id in token_ids:
            try:
                analysis = analyze_spread(token_id)
                if analysis['relative_spread_pct'] <= max_spread_pct:
                    tight_spreads.append(analysis)
            except Exception as e:
                print(f"⚠️ Failed to check spread for {token_id[:20]}...: {e}")
                continue
        
        # Sort by spread (tightest first)
        tight_spreads.sort(key=lambda x: x['relative_spread_pct'])
        
        print(f"✅ Found {len(tight_spreads)} tokens with tight spreads")
        return tight_spreads
        
    except Exception as e:
        print(f"❌ Error finding tight spreads: {e}")
        raise


def main():
    """Example usage"""
    print("📊 Get Spread Utility")
    print("=" * 40)
    
    # Example token IDs
    token_ids = [
        "71321045679252212594626385532706912750332728571942532289631379312455583992563",
        "52114319501245915516055106046884209969926127482827954674443846427813813222426"
    ]
    
    try:
        # Single token spread analysis
        print("\n1️⃣ Single token spread analysis:")
        analysis = analyze_spread(token_ids[0])
        print(f"   Token: {analysis['token_id'][:20]}...")
        print(f"   Bid: ${analysis['bid']}")
        print(f"   Ask: ${analysis['ask']}")
        print(f"   Spread: {analysis['relative_spread_pct']}%")
        print(f"   Midpoint: ${analysis['midpoint']}")
        print(f"   Liquidity: {analysis['liquidity_indicator']}")
        print(f"   Efficiency Score: {analysis['efficiency_score']}/100")
        
        # Multiple token spreads
        print("\n2️⃣ Multiple token spreads:")
        spreads = get_spreads(token_ids)
        print(f"   Retrieved spreads for {len(spreads)} tokens")
        for token_id, spread_data in spreads.items():
            bid = spread_data.get('bid', 0)
            ask = spread_data.get('ask', 0)
            spread_pct = ((ask - bid) / bid * 100) if bid > 0 else 0
            print(f"   {token_id[:20]}...: {spread_pct:.2f}% spread")
        
        # Spread comparison
        print("\n3️⃣ Spread comparison:")
        comparisons = compare_spreads(token_ids)
        
        for i, comp in enumerate(comparisons, 1):
            print(f"   {i}. {comp['token_id'][:20]}... - {comp['relative_spread_pct']}% ({comp['liquidity_indicator']} liquidity)")
        
        # Find tight spreads
        print("\n4️⃣ Tight spreads (< 2%):")
        tight = find_tight_spreads(token_ids, max_spread_pct=2.0)
        
        if tight:
            for spread in tight:
                print(f"   {spread['token_id'][:20]}... - {spread['relative_spread_pct']}% spread")
        else:
            print("   No tight spreads found")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
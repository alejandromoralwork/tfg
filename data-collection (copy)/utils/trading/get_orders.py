"""
Get orders from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import List, Dict, Any, Optional
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON

# Load from config folder
load_dotenv("config/.env")


def get_orders() -> List[Dict[str, Any]]:
    """
    Get all orders
    
    Returns:
        List of order dictionaries
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
        
        print("📋 Fetching all orders...")
        orders = client.get_orders()
        
        print(f"✅ Retrieved {len(orders)} orders")
        return orders
        
    except Exception as e:
        print(f"❌ Error getting orders: {e}")
        raise


def get_order(order_id: str) -> Dict[str, Any]:
    """
    Get specific order by ID
    
    Args:
        order_id: The order ID to retrieve
        
    Returns:
        Order dictionary
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
        
        print(f"📋 Fetching order: {order_id}")
        order = client.get_order(order_id)
        
        print("✅ Order retrieved successfully")
        return order
        
    except Exception as e:
        print(f"❌ Error getting order: {e}")
        raise


def get_open_orders() -> List[Dict[str, Any]]:
    """
    Get only open orders
    
    Returns:
        List of open order dictionaries
    """
    try:
        all_orders = get_orders()
        open_orders = []
        
        for order in all_orders:
            status = order.get('status', '').lower()
            if status in ['open', 'partial']:
                open_orders.append(order)
        
        print(f"📊 Found {len(open_orders)} open orders out of {len(all_orders)} total")
        return open_orders
        
    except Exception as e:
        print(f"❌ Error filtering open orders: {e}")
        raise


def get_orders_summary() -> Dict[str, Any]:
    """
    Get summary of all orders
    
    Returns:
        Dict with order summary statistics
    """
    try:
        orders = get_orders()
        
        total_orders = len(orders)
        open_count = 0
        filled_count = 0
        cancelled_count = 0
        
        buy_orders = 0
        sell_orders = 0
        
        for order in orders:
            status = order.get('status', '').lower()
            side = order.get('side', '').lower()
            
            if status in ['open', 'partial']:
                open_count += 1
            elif status == 'filled':
                filled_count += 1
            elif status == 'cancelled':
                cancelled_count += 1
            
            if side == 'buy':
                buy_orders += 1
            elif side == 'sell':
                sell_orders += 1
        
        summary = {
            'total_orders': total_orders,
            'open_orders': open_count,
            'filled_orders': filled_count,
            'cancelled_orders': cancelled_count,
            'buy_orders': buy_orders,
            'sell_orders': sell_orders,
            'orders': orders
        }
        
        return summary
        
    except Exception as e:
        print(f"❌ Error creating orders summary: {e}")
        raise


def main():
    """Example usage"""
    print("📋 Get Orders Utility")
    print("=" * 40)
    
    try:
        # Get orders summary
        summary = get_orders_summary()
        
        print(f"\n📊 Orders Summary:")
        print(f"   Total Orders: {summary['total_orders']}")
        print(f"   Open Orders: {summary['open_orders']}")
        print(f"   Filled Orders: {summary['filled_orders']}")
        print(f"   Cancelled Orders: {summary['cancelled_orders']}")
        print(f"   Buy Orders: {summary['buy_orders']}")
        print(f"   Sell Orders: {summary['sell_orders']}")
        
        # Show recent orders
        orders = summary['orders']
        if orders:
            print(f"\n📋 Recent Orders:")
            for i, order in enumerate(orders[:5], 1):  # Show first 5
                order_id = order.get('id', 'Unknown')[:10]
                side = order.get('side', 'Unknown')
                status = order.get('status', 'Unknown')
                price = order.get('price', 0)
                size = order.get('size', 0)
                print(f"   {i}. {order_id}... {side.upper()} {status} ${price} x {size}")
        else:
            print("\n📋 No orders found")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
"""
Cancel orders on Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import Dict, Any, List
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON

# Load from config folder
load_dotenv("config/.env")


def cancel_order(order_id: str) -> Dict[str, Any]:
    """
    Cancel a specific order
    
    Args:
        order_id: The order ID to cancel
        
    Returns:
        Cancellation response
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
        
        print(f"❌ Cancelling order: {order_id}")
        response = client.cancel_order(order_id)
        
        print("✅ Order cancelled successfully")
        return response
        
    except Exception as e:
        print(f"❌ Error cancelling order: {e}")
        raise


def cancel_all_orders() -> Dict[str, Any]:
    """
    Cancel all open orders
    
    Returns:
        Cancellation response
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
        
        print("❌ Cancelling all orders...")
        response = client.cancel_all_orders()
        
        print("✅ All orders cancelled successfully")
        return response
        
    except Exception as e:
        print(f"❌ Error cancelling all orders: {e}")
        raise


def cancel_orders(order_ids: List[str]) -> List[Dict[str, Any]]:
    """
    Cancel multiple specific orders
    
    Args:
        order_ids: List of order IDs to cancel
        
    Returns:
        List of cancellation responses
    """
    try:
        responses = []
        
        print(f"❌ Cancelling {len(order_ids)} orders...")
        
        for order_id in order_ids:
            try:
                response = cancel_order(order_id)
                responses.append({
                    'order_id': order_id,
                    'success': True,
                    'response': response
                })
            except Exception as e:
                responses.append({
                    'order_id': order_id,
                    'success': False,
                    'error': str(e)
                })
        
        successful = sum(1 for r in responses if r['success'])
        print(f"✅ Successfully cancelled {successful}/{len(order_ids)} orders")
        
        return responses
        
    except Exception as e:
        print(f"❌ Error cancelling orders: {e}")
        raise


def cancel_orders_by_side(side: str) -> List[Dict[str, Any]]:
    """
    Cancel all orders on a specific side (BUY or SELL)
    
    Args:
        side: "BUY" or "SELL"
        
    Returns:
        List of cancellation responses
    """
    try:
        from utils.trading.get_orders import get_orders
        
        all_orders = get_orders()
        target_orders = []
        
        for order in all_orders:
            order_side = order.get('side', '').upper()
            status = order.get('status', '').lower()
            
            if order_side == side.upper() and status in ['open', 'partial']:
                target_orders.append(order.get('id'))
        
        if target_orders:
            print(f"❌ Found {len(target_orders)} {side} orders to cancel")
            return cancel_orders(target_orders)
        else:
            print(f"📋 No open {side} orders found")
            return []
            
    except Exception as e:
        print(f"❌ Error cancelling {side} orders: {e}")
        raise


def main():
    """Example usage"""
    print("❌ Cancel Orders Utility")
    print("=" * 40)
    
    try:
        from utils.trading.get_orders import get_open_orders
        
        # Check current open orders
        open_orders = get_open_orders()
        
        if not open_orders:
            print("📋 No open orders to cancel")
            return
        
        print(f"\n📊 Found {len(open_orders)} open orders:")
        for i, order in enumerate(open_orders[:5], 1):  # Show first 5
            order_id = order.get('id', 'Unknown')[:10]
            side = order.get('side', 'Unknown')
            price = order.get('price', 0)
            size = order.get('size', 0)
            print(f"   {i}. {order_id}... {side.upper()} ${price} x {size}")
        
        print(f"\n⚠️ WARNING: This will cancel real orders!")
        response = input("Do you want to cancel orders? (yes/no): ").lower()
        
        if response == "yes":
            cancel_method = input("Cancel (1) specific order, (2) all orders, or (3) by side? ").strip()
            
            if cancel_method == "1":
                order_id = input("Enter order ID to cancel: ").strip()
                if order_id:
                    result = cancel_order(order_id)
                    print(f"Result: {result}")
                    
            elif cancel_method == "2":
                result = cancel_all_orders()
                print(f"Result: {result}")
                
            elif cancel_method == "3":
                side = input("Enter side (BUY/SELL): ").strip()
                if side.upper() in ['BUY', 'SELL']:
                    results = cancel_orders_by_side(side)
                    print(f"Results: {results}")
                else:
                    print("❌ Invalid side. Use BUY or SELL")
            else:
                print("❌ Invalid option")
        else:
            print("❌ Order cancellation cancelled")
            
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
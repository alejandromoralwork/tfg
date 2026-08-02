"""
Get notifications from Polymarket
Always uses POLYGON mainnet
"""

import os
from typing import List, Dict, Any
from dotenv import load_dotenv

from py_clob_client.client import ClobClient
from py_clob_client.clob_types import ApiCreds
from py_clob_client.constants import POLYGON

# Load from config folder
load_dotenv("config/.env")


def get_notifications() -> List[Dict[str, Any]]:
    """
    Get account notifications
    
    Returns:
        List of notification dictionaries
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
        
        print("🔔 Fetching notifications...")
        notifications = client.get_notifications()
        
        print(f"✅ Retrieved {len(notifications)} notifications")
        return notifications
        
    except Exception as e:
        print(f"❌ Error getting notifications: {e}")
        raise


def drop_notifications() -> Dict[str, Any]:
    """
    Drop/clear all notifications
    
    Returns:
        Response from drop operation
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
        
        print("🗑️ Dropping all notifications...")
        response = client.drop_notifications()
        
        print("✅ All notifications dropped")
        return response
        
    except Exception as e:
        print(f"❌ Error dropping notifications: {e}")
        raise


def analyze_notifications() -> Dict[str, Any]:
    """
    Analyze notifications and categorize them
    
    Returns:
        Dict with notification analysis
    """
    try:
        notifications = get_notifications()
        
        if not notifications:
            return {
                'total_notifications': 0,
                'categories': {},
                'recent_notifications': []
            }
        
        # Categorize notifications
        categories = {
            'order_fills': 0,
            'order_updates': 0,
            'balance_changes': 0,
            'market_updates': 0,
            'system_notifications': 0,
            'other': 0
        }
        
        recent_notifications = []
        
        for notification in notifications:
            # Extract notification type/category
            notification_type = notification.get('type', '').lower()
            message = notification.get('message', '').lower()
            
            # Categorize based on type and content
            if 'fill' in notification_type or 'fill' in message:
                categories['order_fills'] += 1
            elif 'order' in notification_type or 'order' in message:
                categories['order_updates'] += 1
            elif 'balance' in notification_type or 'balance' in message:
                categories['balance_changes'] += 1
            elif 'market' in notification_type or 'market' in message:
                categories['market_updates'] += 1
            elif 'system' in notification_type or 'system' in message:
                categories['system_notifications'] += 1
            else:
                categories['other'] += 1
            
            # Keep recent notifications (last 10)
            if len(recent_notifications) < 10:
                recent_notifications.append(notification)
        
        analysis = {
            'total_notifications': len(notifications),
            'categories': categories,
            'recent_notifications': recent_notifications,
            'has_unread': any(not notification.get('read', True) for notification in notifications),
            'raw_notifications': notifications
        }
        
        return analysis
        
    except Exception as e:
        print(f"❌ Error analyzing notifications: {e}")
        raise


def get_unread_notifications() -> List[Dict[str, Any]]:
    """
    Get only unread notifications
    
    Returns:
        List of unread notification dictionaries
    """
    try:
        notifications = get_notifications()
        unread = []
        
        for notification in notifications:
            if not notification.get('read', True):  # Assuming 'read' field exists
                unread.append(notification)
        
        print(f"📬 Found {len(unread)} unread notifications")
        return unread
        
    except Exception as e:
        print(f"❌ Error getting unread notifications: {e}")
        raise


def format_notifications(notifications: List[Dict[str, Any]]) -> List[str]:
    """
    Format notifications for display
    
    Args:
        notifications: List of notification dictionaries
        
    Returns:
        List of formatted notification strings
    """
    try:
        formatted = []
        
        for i, notification in enumerate(notifications, 1):
            # Extract notification details
            notification_type = notification.get('type', 'Unknown')
            message = notification.get('message', 'No message')
            timestamp = notification.get('timestamp', 0)
            read_status = "📖" if notification.get('read', True) else "📬"
            
            # Format timestamp
            if timestamp:
                from datetime import datetime
                if timestamp > 10**12:  # Milliseconds
                    timestamp = timestamp / 1000
                time_str = datetime.fromtimestamp(timestamp).strftime('%Y-%m-%d %H:%M:%S')
            else:
                time_str = 'Unknown time'
            
            # Format notification
            formatted_notification = f"{read_status} {i}. [{notification_type}] {message} ({time_str})"
            formatted.append(formatted_notification)
        
        return formatted
        
    except Exception as e:
        print(f"❌ Error formatting notifications: {e}")
        return [f"Error formatting notification {i}: {str(e)}" for i in range(len(notifications))]


def main():
    """Example usage"""
    print("🔔 Get Notifications Utility")
    print("=" * 40)
    
    try:
        # Get notification analysis
        print("\n1️⃣ Notification analysis:")
        analysis = analyze_notifications()
        
        print(f"   Total Notifications: {analysis['total_notifications']}")
        print(f"   Has Unread: {analysis['has_unread']}")
        
        print(f"\n📊 Categories:")
        for category, count in analysis['categories'].items():
            if count > 0:
                print(f"   {category.replace('_', ' ').title()}: {count}")
        
        # Show recent notifications
        recent = analysis['recent_notifications']
        if recent:
            print(f"\n📬 Recent notifications:")
            formatted = format_notifications(recent[:5])  # Show first 5
            for notification in formatted:
                print(f"   {notification}")
        else:
            print(f"\n📬 No recent notifications")
        
        # Check for unread notifications
        print("\n2️⃣ Unread notifications:")
        unread = get_unread_notifications()
        
        if unread:
            formatted_unread = format_notifications(unread)
            for notification in formatted_unread:
                print(f"   {notification}")
        else:
            print("   No unread notifications")
        
        # Option to clear notifications
        if analysis['total_notifications'] > 0:
            print(f"\n3️⃣ Clear notifications:")
            response = input("Do you want to clear all notifications? (yes/no): ").lower()
            
            if response == "yes":
                drop_result = drop_notifications()
                print(f"✅ Notifications cleared: {drop_result}")
            else:
                print("❌ Notifications not cleared")
        
    except Exception as e:
        print(f"❌ Error: {e}")


if __name__ == "__main__":
    main()
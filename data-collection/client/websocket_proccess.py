"""
WebSocket Manager - Core Business Logic
Wraps PolymarketWebsocketsClient for use in PMTerminal
"""

import sys
from pathlib import Path

# Add project roots to path BEFORE imports
THIS_FILE = Path(__file__).resolve()
DATA_COLLECTION_ROOT = THIS_FILE.parent.parent
WORKSPACE_ROOT = DATA_COLLECTION_ROOT.parent

for path in (DATA_COLLECTION_ROOT, WORKSPACE_ROOT):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))


import asyncio
from typing import Callable, Optional, Any, Dict, List
from collections.abc import Callable as CallableType
import json
from threading import Thread
from types import SimpleNamespace

# ============= WEBSOCKET CLIENT IMPORT STRATEGY =============
# Attempts to load the PolymarketWebsocketsClient with fallback to stub mode
WS_CLIENT_AVAILABLE = False
HAS_TYPED_MODELS = False

try:
    from lomond import WebSocket
    from lomond.persist import persist
except Exception:
    WebSocket = None
    persist = None

ApiCreds = Any
OrderBookSummaryEvent = dict
PriceChangeEvent = dict
LastTradePriceEvent = dict
OrderEvent = dict
TradeEvent = dict
LiveDataOrderBookSummaryEvent = dict
LiveDataTradeEvent = dict

try:
    # First, try package-style imports when running from workspace root
    from client.py_clob_client.clob_types import ApiCreds
    from client.py_ws_client.types.websockets_types import (
        OrderBookSummaryEvent,
        PriceChangeEvent,
        LastTradePriceEvent,
        OrderEvent,
        TradeEvent,
        LiveDataOrderBookSummaryEvent,
        LiveDataTradeEvent,
    )
    HAS_TYPED_MODELS = True
except (ImportError, SyntaxError):
    try:
        # Fallback when running this file directly from data-collection/client
        from py_clob_client.clob_types import ApiCreds
        from py_ws_client.types.websockets_types import (
            OrderBookSummaryEvent,
            PriceChangeEvent,
            LastTradePriceEvent,
            OrderEvent,
            TradeEvent,
            LiveDataOrderBookSummaryEvent,
            LiveDataTradeEvent,
        )
        HAS_TYPED_MODELS = True
    except (ImportError, SyntaxError):
        HAS_TYPED_MODELS = False

try:
    # Try to load the PolymarketWebsocketsClient (works on Python 3.6+)
    try:
        from client.py_ws_client.websockets_client import PolymarketWebsocketsClient
        WS_CLIENT_AVAILABLE = True
        print(f"[WebSocket] ✓ WebSocket client loaded (Python {sys.version_info.major}.{sys.version_info.minor})")
    except (ImportError, SyntaxError) as e:
        try:
            from py_ws_client.websockets_client import PolymarketWebsocketsClient
            WS_CLIENT_AVAILABLE = True
            print(f"[WebSocket] ✓ WebSocket client loaded (direct package import)")
        except (ImportError, SyntaxError):
            print(f"[WebSocket] Warning: Could not load websocket client: {e}")
            WS_CLIENT_AVAILABLE = False

except (ImportError, SyntaxError) as e:
    print(f"[WebSocket] Warning: WebSocket import failed: {e}")
    print(f"[WebSocket] Running in STUB mode (WebSocket features disabled)")
    WS_CLIENT_AVAILABLE = False

# Create stub classes if nothing else worked
if not WS_CLIENT_AVAILABLE:
    print(f"[WebSocket] Running in STUB mode (features disabled)")
    
    class ApiCreds:
        def __init__(self, **kwargs):
            pass
    
    class PolymarketWebsocketsClient:
        def __init__(self, *args, **kwargs):
            self.url_market = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
            self.url_user = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
            self.url_live_data = "wss://ws-live-data.polymarket.com"

        def market_socket(self, token_ids: list[str], process_event: Callable):
            if WebSocket is None or persist is None:
                raise RuntimeError("lomond is required for websocket fallback mode")

            websocket = WebSocket(self.url_market)
            for event in persist(websocket):
                if event.name == "ready":
                    websocket.send_json(assets_ids=token_ids)
                elif event.name == "text":
                    process_event(event)

        def user_socket(self, creds: ApiCreds, process_event: Callable):
            raise RuntimeError("User websocket unavailable in fallback mode")

        def live_data_socket(self, subscriptions: list[dict[str, Any]], process_event: Callable, creds: Optional[ApiCreds] = None):
            raise RuntimeError("Live data websocket unavailable in fallback mode")
    
    # Event stubs
    OrderBookSummaryEvent = dict
    PriceChangeEvent = dict
    LastTradePriceEvent = dict
    OrderEvent = dict
    TradeEvent = dict
    LiveDataOrderBookSummaryEvent = dict
    LiveDataTradeEvent = dict


class OrderBook:
    def __init__(self, market_slug: Optional[str] = None):
        self.token_id = None
        self.timestamp = None
        self.condition_id = None # Market ID
        self.market_slug = market_slug  # NEW: store market slug
        self.asks = []  # List of tuples: (price, shares)
        self.bids = []  # List of tuples: (price, shares)
        self.best_ask = None  # Tuple: (price, shares)
        self.best_bid = None  # Tuple: (price, shares)

    #there are 3 responses from ws;     

    def update_from_price_change(self, event):
        """
        Update order book from a PriceChangeEvent object.
        Args:
            event: PriceChangeEvent object containing a list of price_changes
        """
        if isinstance(event, dict):
            self.timestamp = event.get("timestamp") or event.get("t")
            self.condition_id = event.get("condition_id") or event.get("market") or event.get("m")
            self.token_id = event.get("token_id") or event.get("asset_id")
            price_changes = event.get("price_changes") or event.get("pc") or []
        else:
            self.timestamp = getattr(event, "timestamp", None)
            self.condition_id = getattr(event, "condition_id", None)
            self.token_id = getattr(event, "token_id", None)
            price_changes = getattr(event, "price_changes", [])

        for pc in price_changes:
            side = pc.get("side") if isinstance(pc, dict) else getattr(pc, "side", None)
            best_ask = pc.get("best_ask", pc.get("ba")) if isinstance(pc, dict) else getattr(pc, "best_ask", None)
            best_bid = pc.get("best_bid", pc.get("bb")) if isinstance(pc, dict) else getattr(pc, "best_bid", None)
            price = pc.get("price", pc.get("p")) if isinstance(pc, dict) else getattr(pc, "price", None)
            size = pc.get("size", pc.get("s")) if isinstance(pc, dict) else getattr(pc, "size", None)

            if side == "SELL":
                self.best_ask = (float(best_ask), float(size))
                # Remove any existing ask with the same price
                self.asks = [ask for ask in self.asks if ask[0] != float(price)]
                self.asks.append((float(price), float(size)))
            elif side == "BUY":
                self.best_bid = (float(best_bid), float(size))
                # Remove any existing bid with the same price
                self.bids = [bid for bid in self.bids if bid[0] != float(price)]
                self.bids.append((float(price), float(size)))

    def update_from_summary(self, event):

        # bids/asks: list of dicts {"price": str, "size": str}
        if isinstance(event, dict):
            self.token_id = event.get("token_id") or event.get("asset_id")
            self.condition_id = event.get("condition_id") or event.get("market")
            self.timestamp = event.get("timestamp") or event.get("t")
            self.hash = event.get("hash") or event.get("h")
            raw_bids = event.get("bids") or []
            raw_asks = event.get("asks") or []
            self.bids = [
                (float(order.get("price", order.get("p"))), float(order.get("size", order.get("s"))))
                for order in raw_bids
            ]
            self.asks = [
                (float(order.get("price", order.get("p"))), float(order.get("size", order.get("s"))))
                for order in raw_asks
            ]
        else:
            self.token_id = getattr(event, "token_id", None)
            self.condition_id = getattr(event, "condition_id", None)
            self.timestamp = getattr(event, "timestamp", None)
            self.hash = getattr(event, "hash", None)
            self.bids = [(float(order.price), float(order.size)) for order in (event.bids or [])]
            self.asks = [(float(order.price), float(order.size)) for order in (event.asks or [])]
        self.best_bid = self.bids[0] if self.bids else None
        self.best_ask = self.asks[0] if self.asks else None


    def update_from_last_trade(self, event):
            """
            Update order book from a LastTradePrice event.
            Args:
                event: LastTradePrice object with price, size, side, token_id, condition_id, fee_rate_bps
            """
            if isinstance(event, dict):
                self.token_id = event.get("token_id") or event.get("asset_id")
                self.condition_id = event.get("condition_id") or event.get("market")
                self.timestamp = event.get("timestamp") or event.get("t")
                price = float(event.get("price")) if event.get("price") is not None else None
                size = float(event.get("size")) if event.get("size") is not None else None
                side = event.get("side")
            else:
                self.token_id = getattr(event, "token_id", None)
                self.condition_id = getattr(event, "condition_id", None)
                self.timestamp = getattr(event, "timestamp", None)
                price = getattr(event, "price", None)
                size = getattr(event, "size", None)
                side = getattr(event, "side", None)
            if side == "SELL":
                # Decrement shares at the ask price by trade size
                updated_asks = []
                found = False
                for ask in self.asks:
                    if ask[0] == price:
                        new_size = max(ask[1] - size, 0)
                        updated_asks.append((price, new_size))
                        found = True
                    else:
                        updated_asks.append(ask)
                # If price not found, add with size 0
                if not found:
                    updated_asks.append((price, 0))
                self.asks = updated_asks
                self.best_ask = self.asks[0] if self.asks else None
            elif side == "BUY":
                # Decrement shares at the bid price by trade size
                updated_bids = []
                found = False
                for bid in self.bids:
                    if bid[0] == price:
                        new_size = max(bid[1] - size, 0)
                        updated_bids.append((price, new_size))
                        found = True
                    else:
                        updated_bids.append(bid)
                # If price not found, add with size 0
                if not found:
                    updated_bids.append((price, 0))
                self.bids = updated_bids
                self.best_bid = self.bids[0] if self.bids else None

    def set_market_slug(self, market_slug: str):
        """Set the market slug for this orderbook"""
        self.market_slug = market_slug
    
    def get_orderbook(self, *fields):
        """
        Return orderbook state. If fields are specified, return only those fields. Otherwise, return all.
        Args:
            *fields: Optional field names to include in the result.
        Returns:
            dict of requested fields (or all if none specified)
        """
        all_fields = {
            "token_id": self.token_id,
            "market_slug": self.market_slug,  # NEW: include market slug
            "asks": self.asks,
            "bids": self.bids,
            "best_ask": self.best_ask,
            "best_bid": self.best_bid,
            "timestamp": getattr(self, "timestamp", None),
            "hash": getattr(self, "hash", None)
        }
        if fields:
            return {field: all_fields[field] for field in fields if field in all_fields}
        return all_fields



class WebSocketCore:
    """
    Core WebSocket manager for PMTerminal
    
    Handles connections to Polymarket WebSocket feeds:
    - Market data (orderbooks, price changess)
    - User data (orders, trades)
    - Live activity data
    
    No UI dependencies - returns parsed data to callbacks
    """
    
    def __init__(self, api_creds: Optional[ApiCreds] = None):
        """
        Initialize WebSocket manager
        
        Args:
            api_creds: API credentials for authenticated endpoints
        """
        # Check if WebSocket client is available
        self.ws_available = WS_CLIENT_AVAILABLE
        
        if self.ws_available:
            self.client = PolymarketWebsocketsClient()
        else:
            self.client = None
            print("[WebSocketCore] Running in STUB mode - WebSocket features disabled")
        
        self.api_creds = api_creds
        
        # Multiple orderbooks - one per token
        self.orderbooks: Dict[str, OrderBook] = {}  # {token_id: OrderBook instance}
        
        # Legacy single orderbook (for backward compatibility)
        self.orderbook = OrderBook()
        
        # Connection state (dict for compatibility with tests)
        self.connection_state = {
            'market': False,
            'user': False,
            'live_data': False
        }
        self.market_connected = False
        self.user_connected = False
        self.live_data_connected = False
        
        # Background threads
        self._market_thread: Optional[Thread] = None
        self._user_thread: Optional[Thread] = None
        self._live_data_thread: Optional[Thread] = None
        
        # Event callbacks
        self._orderbook_callback: Optional[Callable] = None
        self._trade_callback: Optional[Callable] = None
        self._order_callback: Optional[Callable] = None
        self._price_change_callback: Optional[Callable] = None
        self._last_trade_callback: Optional[Callable] = None
        self._live_data_callback: Optional[Callable] = None
    
    # ═══════════════════════════════════════════════════════════════
    #                    MARKET DATA FEED
    # ═══════════════════════════════════════════════════════════════
    
    def connect_market(
        self, 
        token_ids: List[str],
        on_orderbook: Optional[Callable] = None,
        on_price_change: Optional[Callable] = None,
        on_last_trade: Optional[Callable] = None
    ) -> None:
        """
        Connect to market WebSocket feed
        
        Args:
            token_ids: List of token IDs to subscribe to
            on_orderbook: Callback for orderbook updates
                         Signature: (token_id: str, data: dict) -> None
            on_price_change: Callback for price changes
                           Signature: (token_id: str, old_price: float, new_price: float) -> None
            on_last_trade: Callback for last trade price
                         Signature: (token_id: str, price: float) -> None
        
        Example:
            >>> ws.connect_market(
            ...     token_ids=["0x1234...", "0x5678..."],
            ...     on_orderbook=lambda tid, data: print(f"OrderBook {tid}: {data}")
            ... )
        """
        
        # Check if WebSocket is available
        if not self.ws_available:
            print("[WebSocketCore] Cannot connect: WebSocket client not available (stub mode)")
            print("[WebSocketCore] Ensure lomond package is installed: pip install lomond")
            return

        # Initialize separate OrderBook instance for each token
        for token_id in token_ids:
            self.orderbooks[token_id] = OrderBook()

        # Store callbacks
        self._orderbook_callback = on_orderbook
        self._price_change_callback = on_price_change
        self._last_trade_callback = on_last_trade
        
        # Create custom event processor
        def process_market_event(event):
            try:
                message = event.json
                
                # Handle batch messages
                if isinstance(message, list):
                    for item in message:
                        self._handle_market_message(item)
                    return
                
                # Handle single message
                self._handle_market_message(message)
                
            except Exception as e:
                print(f"Error processing market event: {e}")
        
        # Start WebSocket in background thread
        def run_market_socket():
            self.market_connected = True
            self.connection_state['market'] = True
            try:
                self.client.market_socket(
                    token_ids=token_ids,
                    process_event=process_market_event
                )
            finally:
                self.market_connected = False
                self.connection_state['market'] = False
        
        self._market_thread = Thread(target=run_market_socket, daemon=True)
        self._market_thread.start()
    
    def _handle_market_message(self, message: dict) -> None:
        """Process individual market message and update orderbook state"""
        import time
        event_type = message.get("event_type")
        
        # Extract token_id to route to correct orderbook
        token_id = message.get("token_id") or message.get("asset_id")
        
        # For price_change events, token_id is inside each price_change object
        # Process each price_change separately for its token
        if event_type == "price_change" and not token_id:
            price_changes = message.get("price_changes", []) or message.get("pc", [])
            
            # Process each price change separately (each may have different token_id)
            for pc in price_changes:
                # Extract token_id from the price_change object
                pc_token_id = pc.get("token_id") or pc.get("asset_id") or pc.get("a")
                
                if not pc_token_id:
                    print(f"[WS] Price change without token_id in price_changes array")
                    continue
                
                # Get or create OrderBook for this token
                if pc_token_id not in self.orderbooks:
                    self.orderbooks[pc_token_id] = OrderBook()
                
                orderbook = self.orderbooks[pc_token_id]
                
                # Create a temporary message with this specific price_change
                temp_message = message.copy()
                temp_message['price_changes'] = [pc]
                temp_message['token_id'] = pc_token_id  # Add token_id for the event
                
                try:
                    event = PriceChangeEvent(**temp_message)
                    orderbook.update_from_price_change(event)
                    
                    # DEBUG: Print update info
                    print(f"[WS] Price change for {pc_token_id[:8]}... at {time.strftime('%H:%M:%S')}")
                    
                    # Also update legacy single orderbook
                    self.orderbook = orderbook
                    
                    if self._price_change_callback:
                        self._price_change_callback(orderbook.get_orderbook())
                except Exception as e:
                    print(f"[WS] Error processing price_change for {pc_token_id[:8]}: {e}")
            
            # Return early since we processed all price changes
            return
        
        # For other event types, token_id should be at top level
        if not token_id:
            print(f"[WS] Message without token_id - event_type: {event_type}")
            return
        
        # Get or create OrderBook for this token
        if token_id not in self.orderbooks:
            self.orderbooks[token_id] = OrderBook()

        # Route to correct orderbook instance
        orderbook = self.orderbooks[token_id]

        if event_type == "book":
            event = OrderBookSummaryEvent(**message)
            orderbook.update_from_summary(event)
            
            # DEBUG: Print update info
            print(f"[WS] Updated orderbook for {token_id[:8]}... at {time.strftime('%H:%M:%S')} - {len(orderbook.bids)} bids, {len(orderbook.asks)} asks")
            
            # Also update legacy single orderbook (for backward compatibility)
            self.orderbook = orderbook
            
            if self._orderbook_callback:
                self._orderbook_callback(orderbook.get_orderbook())

        elif event_type == "last_trade_price":
            event = LastTradePriceEvent(**message)
            orderbook.update_from_last_trade(event)
            
            # DEBUG: Print update info
            print(f"[WS] Last trade for {token_id[:8]}... at {time.strftime('%H:%M:%S')}")
            
            # Also update legacy single orderbook
            self.orderbook = orderbook
            
            if self._last_trade_callback:
                self._last_trade_callback(orderbook.get_orderbook())
    
    # ═══════════════════════════════════════════════════════════════
    #                    USER DATA FEED (Authenticated)
    # ═══════════════════════════════════════════════════════════════
    
    def connect_user(
        self,
        on_order: Optional[Callable] = None,
        on_trade: Optional[Callable] = None
    ) -> None:
        """
        Connect to user WebSocket feed (requires API credentials)
        
        Args:
            on_order: Callback for order updates
                     Signature: (order_data: dict) -> None
            on_trade: Callback for trade updates
                     Signature: (trade_data: dict) -> None
        
        Raises:
            ValueError: If API credentials not provided
        """
        
        # Check if WebSocket is available
        if not self.ws_available:
            print("[WebSocketCore] Cannot connect user: WebSocket client not available (stub mode)")
            return
        
        if not self.api_creds:
            raise ValueError("API credentials required for user feed")
        
        # Store callbacks
        self._order_callback = on_order
        self._trade_callback = on_trade
        
        # Create custom event processor
        def process_user_event(event):
            try:
                message = event.json
                self._handle_user_message(message)
            except Exception as e:
                print(f"Error processing user event: {e}")
        
        # Start WebSocket in background thread
        def run_user_socket():
            self.user_connected = True
            self.connection_state['user'] = True
            try:
                self.client.user_socket(
                    creds=self.api_creds,
                    process_event=process_user_event
                )
            finally:
                self.user_connected = False
                self.connection_state['user'] = False
        
        self._user_thread = Thread(target=run_user_socket, daemon=True)
        self._user_thread.start()
    
    def _handle_user_message(self, message: dict) -> None:
        """Process individual user message"""
        event_type = message.get("event_type")
        
        if event_type == "order":
            # Order update
            event = OrderEvent(**message)
            
            if self._order_callback:
                data = {
                    "order_id": event.order_id,
                    "token_id": event.token_id,
                    "condition_id": event.condition_id,
                    "side": event.side,
                    "size": float(event.original_size),
                    "price": float(event.price),
                    "status": event.status,
                    "timestamp": event.timestamp
                }
                self._order_callback(data)
        
        elif event_type == "trade":
            # Trade execution
            event = TradeEvent(**message)
            if self._trade_callback:
                data = {
                    "trade_id": event.trade_id,
                    "token_id": event.token_id,
                    "condition_id": event.condition_id,
                    "side": event.side,
                    "size": float(event.size),
                    "price": float(event.price),
                    "status": event.status,
                    "timestamp": event.timestamp
                }
                self._trade_callback(data)
    
    # ═══════════════════════════════════════════════════════════════
    #                    LIVE DATA FEED
    # ═══════════════════════════════════════════════════════════════
    
    def connect_live_data(
        self,
        subscriptions: List[Dict[str, Any]],
        on_event: Callable[[str, dict], None]
    ) -> None:
        """
        Connect to live data feed
        
        Args:
            subscriptions: List of subscription configs
            on_event: Generic callback for all events
                     Signature: (event_type: str, data: dict) -> None
        
        Subscription examples:
            [
                {"asset_id": "0x1234..."},  # Book updates for specific asset
                {"market": "CRYPTO-ETH-USD"}   # All trades in market
            ]
        """
        
        # Check if WebSocket is available
        if not self.ws_available:
            print("[WebSocketCore] Cannot connect live data: WebSocket client not available (stub mode)")
            return
        
        self._live_data_callback = on_event
        
        def process_live_event(event):
            try:
                message = event.json
                event_type = message.get("type")
                if event_type and self._live_data_callback:
                    self._live_data_callback(event_type, message)
            except Exception as e:
                print(f"Error processing live event: {e}")
        
        def run_live_socket():
            self.live_data_connected = True
            self.connection_state['live_data'] = True
            try:
                self.client.live_data_socket(
                    subscriptions=subscriptions,
                    process_event=process_live_event,
                    creds=self.api_creds
                )
            finally:
                self.live_data_connected = False
                self.connection_state['live_data'] = False
        
        self._live_data_thread = Thread(target=run_live_socket, daemon=True)
        self._live_data_thread.start()
    
    # ═══════════════════════════════════════════════════════════════
    #                    CONNECTION MANAGEMENT
    # ═══════════════════════════════════════════════════════════════
    
    def disconnect_all(self) -> None:
        """Disconnect all WebSocket connections"""
        # Mark as disconnected (this will stop monitoring loops)
        self.market_connected = False
        self.user_connected = False
        self.live_data_connected = False
        
        # Update connection state dict
        self.connection_state = {
            'market': False,
            'user': False,
            'live_data': False
        }
        
        # Clear orderbooks
        self.orderbooks.clear()
        
        # Note: The WebSocket thread will continue until the next reconnect attempt
        # due to lomond's persist() behavior, but marking disconnected will prevent
        # our monitoring loops from processing data
    
    def is_connected(self) -> bool:
        """Check if any WebSocket is connected"""
        return (
            self.market_connected or 
            self.user_connected or 
            self.live_data_connected
        )
    
    def get_orderbook(self, token_id: str) -> Optional[Dict]:
        """
        Get orderbook data for a specific token
        
        Args:
            token_id: Token ID to get orderbook for
            
        Returns:
            dict with orderbook data, or None if token not found
        """
        if token_id in self.orderbooks:
            return self.orderbooks[token_id].get_orderbook()
        return None
    
    def get_all_orderbooks(self) -> Dict[str, Dict]:
        """
        Get all orderbook data for all subscribed tokens
        
        Returns:
            dict mapping token_id to orderbook data
        """
        return {
            token_id: ob.get_orderbook() 
            for token_id, ob in self.orderbooks.items()
        }
    
    def get_subscribed_tokens(self) -> list:
        """
        Get list of all currently subscribed token IDs
        
        Returns:
            list of token_id strings
        """
        return list(self.orderbooks.keys())
    
    def set_market_slugs(self, token_to_market: Dict[str, str]) -> None:
        """
        Set market slugs for orderbooks
        
        Args:
            token_to_market: Dict mapping token_id to market_slug
        """
        for token_id, market_slug in token_to_market.items():
            if token_id in self.orderbooks:
                self.orderbooks[token_id].set_market_slug(market_slug)
    
    @property
    def connection_status(self) -> Dict[str, bool]:
        """Get status of all connections"""
        return {
            "market": self.market_connected,
            "user": self.user_connected,
            "live_data": self.live_data_connected
        }


# ═══════════════════════════════════════════════════════════════
#                    HELPER FUNCTIONS
# ═══════════════════════════════════════════════════════════════

def create_orderbook_subscription(market_id: str, assets: List[str] = ["YES", "NO"]) -> dict:
    """Create orderbook subscription config"""
    return {
        "topic": "agg_orderbook",
        "market": market_id,
        "assets": assets
    }

def create_trades_subscription(market_id: str) -> dict:
    """Create trades subscription config"""
    return {
        "topic": "trades",
        "market": market_id
    }

def create_user_subscription(market_id: str) -> dict:
    """Create user orders subscription config (requires auth)"""
    return {
        "topic": "clob_user",
        "market": market_id
    }


# ...existing code...

# ═══════════════════════════════════════════════════════════════
#                    TESTING & DEMO
# ═══════════════════════════════════════════════════════════════

if __name__ == "__main__":
    """
    Test WebSocket Manager with real Polymarket market feed.

    Usage:
        python data-collection/client/websocket_proccess.py --token-id <TOKEN_ID>
    """
    import argparse
    import time
    from datetime import datetime, timezone

    parser = argparse.ArgumentParser(description="Run Polymarket market WS collector")
    parser.add_argument("--token-id", required=False, default="41389657506315640632371883405798742040263257472727535495730986964464899872806", help="Token ID to subscribe to")
    parser.add_argument("--duration", type=int, default=0, help="Seconds to run (0 = run forever)")
    parser.add_argument("--orderbook-log", default="orderbook_log.jsonl", help="Path for orderbook snapshots JSONL")
    parser.add_argument("--trade-log", default="trade_log.jsonl", help="Path for trade updates JSONL")
    args = parser.parse_args()

    orderbook_log_path = Path(args.orderbook_log)
    trade_log_path = Path(args.trade_log)

    def append_jsonl(path: Path, payload: dict) -> None:
        record = {
            "collected_at": datetime.now(timezone.utc).isoformat(),
            **payload,
        }
        with path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(record, default=str) + "\n")

    def print_orderbook(data):
        append_jsonl(orderbook_log_path, {"event": "orderbook", "data": data})
        # Pretty-print the full orderbook for easier inspection
        try:
            pretty = json.dumps(data, default=str, indent=2)
        except Exception:
            pretty = str(data)
        print(f"[ORDERBOOK] token={data.get('token_id')}\n{pretty}")

    def print_price_change(data):
        append_jsonl(orderbook_log_path, {"event": "price_change", "data": data})
        print(f"[PRICE_CHANGE] token={data.get('token_id')} best_bid={data.get('best_bid')} best_ask={data.get('best_ask')}")

    def print_last_trade(data):
        append_jsonl(trade_log_path, {"event": "last_trade_price", "data": data})
        print(f"[LAST_TRADE] token={data.get('token_id')} best_bid={data.get('best_bid')} best_ask={data.get('best_ask')}")

    data = WebSocketCore()


    data.connect_market(
        token_ids=[args.token_id],
        on_orderbook=print_orderbook,
        on_price_change=print_price_change,
        on_last_trade=print_last_trade
    )

    start = time.time()
    try:
        while True:
            time.sleep(2)
            if args.duration > 0 and (time.time() - start) >= args.duration:
                break
    except KeyboardInterrupt:
        pass
    finally:
        data.disconnect_all()
        print("Collector stopped.")
  
    
    
    
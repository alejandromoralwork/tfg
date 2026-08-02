#!/usr/bin/env python3
"""
Fast Terminal Trader for Polymarket
- Ultra-low-latency order book via WebSocket
- Single-key hotkeys for Buy/Sell operations
- Minimal UI optimized for speed and clarity in terminal

Requirements:
- Windows PowerShell or any terminal
- Python 3.10+
- pip install -r requirements.txt (py-clob-client, websocket-client, colorama)
- .env with CLOB_API_URL, API creds (if required for order placement)

Usage:
  python utils/terminal/fast_trader.py --yes-token <TOKEN_ID> [--no-token <TOKEN_ID>] [--size 50] [--price 0.45]
  # Hotkeys inside app:
  #   y    -> Buy YES at best ask (marketable IOC)
  #   n    -> Buy NO at best ask (if token provided)
  #   s    -> Sell YES at best bid (if inventory)
  #   a/z  -> Increase/Decrease size
  #   p/;  -> Increase/Decrease price (limit template)
  #   r    -> Reset PnL counters
  #   q    -> Quit
"""
import os
import sys
import json
import time
import threading
import traceback
import signal
from dataclasses import dataclass
from typing import Optional, Dict, Any, List

from colorama import Fore, Style, init as colorama_init
from polymarket_apis.clients.websockets_client import PolymarketWebsocketsClient

# Local client
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', 'py-clob-client-main')))
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import OrderArgs, OrderType
from py_clob_client.order_builder.constants import BUY, SELL

colorama_init(autoreset=True)

HTTP_HOST = os.environ.get('CLOB_API_URL', 'https://clob.polymarket.com')

@dataclass
class OrderBook:
    bids: List[Dict[str, Any]]
    asks: List[Dict[str, Any]]
    ts: Optional[float] = None

class FastTrader:
    def __init__(self, yes_token: str, no_token: Optional[str] = None, size: float = 50.0, price: float = 0.45):
        self.yes_token = yes_token
        self.no_token = no_token
        self.size = size
        self.price = price

        self.book: Dict[str, OrderBook] = {}
        self.ws_client: Optional[PolymarketWebsocketsClient] = None
        self.client = ClobClient(HTTP_HOST)
        self._running = True
        self._print_lock = threading.Lock()

    # ========== UI ==========
    def clear_screen(self):
        os.system('cls' if os.name == 'nt' else 'clear')

    def draw(self):
        with self._print_lock:
            self.clear_screen()
            print(f"FastTrader - YES {self._short(self.yes_token)}  NO {self._short(self.no_token) if self.no_token else '-'}  | Size {self.size:.2f}  Price {self.price:.4f}")
            print("=" * 80)
            for token in [self.yes_token] + ([self.no_token] if self.no_token else []):
                ob = self.book.get(token)
                print(f"Token: {self._short(token)}")
                if not ob:
                    print("  Waiting for order book...")
                    continue
                # Print top 10 of each side
                asks = ob.asks[:10]
                bids = ob.bids[:10]
                print(Fore.RED + "  ASKS (price \t size)")
                for a in asks:
                    print(Fore.RED + f"    {float(a['price']):.4f}\t{float(a['size']):.2f}")
                print(Fore.GREEN + "  BIDS (price \t size)")
                for b in bids:
                    print(Fore.GREEN + f"    {float(b['price']):.4f}\t{float(b['size']):.2f}")
                print(Style.RESET_ALL)
            print("- Hotkeys: y=Buy YES, n=Buy NO, s=Sell YES, a/z=size +/- , p/;=price +/- , r=reset, q=quit")

    def _short(self, tid: Optional[str]) -> str:
        if not tid:
            return '-'
        return tid[:10] + '...' + tid[-6:]

    # ========== WebSocket ==========
    def start_ws(self):
        # Use PolymarketWebsocketsClient to subscribe and stream book updates
        tokens = [self.yes_token] + ([self.no_token] if self.no_token else [])

        def handle_event(event):
            try:
                msg = event.json
                evs = msg if isinstance(msg, list) else [msg]
                for ev in evs:
                    if ev.get('event_type') == 'book':
                        token = ev.get('asset_id') or ev.get('token_id')
                        bids = ev.get('bids', [])
                        asks = ev.get('asks', [])
                        self.book[token] = OrderBook(bids=bids, asks=asks, ts=time.time())
                self.draw()
            except Exception:
                print('WS parse error:', traceback.format_exc())

        def run_socket():
            try:
                self.ws_client = PolymarketWebsocketsClient()
                self.ws_client.market_socket(tokens, process_event=handle_event)
            except Exception:
                print('WS fatal error:', traceback.format_exc())

        threading.Thread(target=run_socket, daemon=True).start()

    # ========== Orders ==========
    def buy_yes_market(self):
        return self._market_ioc(self.yes_token, BUY)

    def buy_no_market(self):
        if not self.no_token:
            print('NO token not provided')
            return None
        return self._market_ioc(self.no_token, BUY)

    def sell_yes_market(self):
        return self._market_ioc(self.yes_token, SELL)

    def _market_ioc(self, token_id: str, side: str):
        # Create a simple IOC order at best price from the book
        ob = self.book.get(token_id)
        if not ob:
            print('No orderbook yet')
            return None
        if side == BUY:
            best = ob.asks[0] if ob.asks else None
        else:
            best = ob.bids[0] if ob.bids else None
        if not best:
            print('No liquidity')
            return None
        price = float(best['price'])
        size = float(min(self.size, best.get('size', self.size)))
        try:
            args = OrderArgs(
                token_id=token_id,
                side=side,
                price=price,
                size=size,
                time_in_force='IOC'
            )
            signed = self.client.create_order(args)
            resp = self.client.post_order(signed, OrderType.GTD)
            print('Order response:', resp)
            return resp
        except Exception:
            print('Order error:', traceback.format_exc())
            return None

    # ========== Input ==========
    def input_loop(self):
        try:
            import msvcrt  # Windows single-key input
            while self._running:
                if msvcrt.kbhit():
                    ch = msvcrt.getch().decode('utf-8', errors='ignore').lower()
                    if ch == 'q':
                        self._running = False
                    elif ch == 'y':
                        self.buy_yes_market()
                    elif ch == 'n':
                        self.buy_no_market()
                    elif ch == 's':
                        self.sell_yes_market()
                    elif ch == 'a':
                        self.size = min(self.size + 5, 10000)
                    elif ch == 'z':
                        self.size = max(self.size - 5, 1)
                    elif ch == 'p':
                        self.price = min(self.price + 0.005, 0.99)
                    elif ch == ';':
                        self.price = max(self.price - 0.005, 0.01)
                    self.draw()
                time.sleep(0.01)
        except KeyboardInterrupt:
            self._running = False

    def stop(self):
        self._running = False
        try:
            if self.ws_client and hasattr(self.ws_client, 'close'):
                self.ws_client.close()
        except Exception:
            pass


def parse_args():
    import argparse
    p = argparse.ArgumentParser(description='Fast Terminal Trader for Polymarket')
    p.add_argument('--yes-token', required=True, help='YES outcome token id')
    p.add_argument('--no-token', help='NO outcome token id (optional)')
    p.add_argument('--size', type=float, default=50.0, help='default order size')
    p.add_argument('--price', type=float, default=0.45, help='default price for limit templates')
    return p.parse_args()


def main():
    args = parse_args()
    trader = FastTrader(args.yes_token, args.no_token, args.size, args.price)
    trader.start_ws()
    trader.draw()

    # Graceful shutdown
    def handle_sigint(sig, frame):
        trader.stop()
        sys.exit(0)
    signal.signal(signal.SIGINT, handle_sigint)

    trader.input_loop()


if __name__ == '__main__':
    print('[DEPRECATED] This script has moved to utils/quickshoot_terminal/fast_trader.py')
    main()

#!/usr/bin/env python3
"""
Pro Terminal Trader (Interactive TUI)
- Live orderbook panel (updates continuously)
- Interactive command prompt (type while stream runs)
- Clean, fast, keyboard-first

Dependencies: prompt_toolkit, rich, websocket-client, py-clob-client

Commands (type into the prompt):
  sub YES_TOKEN [NO_TOKEN]     # subscribe to tokens
  buy yes [SIZE]               # buy YES market IOC
  buy no [SIZE]                # buy NO market IOC
  sell yes [SIZE]              # sell YES market IOC
  size <NUMBER>                # set default size
  price <NUMBER>               # set template price
  info                         # show tokens and status
  help                         # list commands
  quit                         # exit

Usage:
  python utils/terminal/pro_trader.py --yes-token <YES_TOKEN> [--no-token <NO_TOKEN>] [--size 50]
"""
import os
import sys
import json
import time
import threading
import traceback
from dataclasses import dataclass
from typing import Optional, Dict, Any, List

from prompt_toolkit import PromptSession
from prompt_toolkit.patch_stdout import patch_stdout
from prompt_toolkit.completion import WordCompleter
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from polymarket_apis.clients.websockets_client import PolymarketWebsocketsClient

# Local client
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', 'py-clob-client-main')))
from py_clob_client.client import ClobClient
from py_clob_client.clob_types import OrderArgs, OrderType
from py_clob_client.order_builder.constants import BUY, SELL

console = Console()
DEFAULT_WS_URL = os.environ.get('POLY_WS_URL', 'wss://ws.clob.polymarket.com/market')
DEFAULT_HTTP_HOST = os.environ.get('CLOB_API_URL', 'https://clob.polymarket.com')

@dataclass
class OrderBook:
    bids: List[Dict[str, Any]]
    asks: List[Dict[str, Any]]
    ts: Optional[float] = None

class ProTrader:
    def __init__(self, yes_token: Optional[str], no_token: Optional[str], size: float = 50.0, price: float = 0.45, ws_url: Optional[str] = None, http_host: Optional[str] = None):
        self.yes_token = yes_token
        self.no_token = no_token
        self.size = size
        self.price = price
        self.ws_url = ws_url or DEFAULT_WS_URL
        self.http_host = http_host or DEFAULT_HTTP_HOST
        self.client = ClobClient(self.http_host)
        self.ws_client = None
        self.books: Dict[str, OrderBook] = {}
        self._running = True
        self._ui_lock = threading.Lock()

    # ===== Display =====
    def render(self):
        with self._ui_lock:
            console.clear()
            console.rule(f"ProTrader  YES: {self._short(self.yes_token)}  NO: {self._short(self.no_token)}  Size: {self.size:.2f}")
            console.print(f"[dim]WS: {self.ws_url}  |  API: {self.http_host}[/dim]")
            for token in [t for t in [self.yes_token, self.no_token] if t]:
                book = self.books.get(token)
                table = Table(title=f"Token {self._short(token)}", show_header=True, header_style="bold cyan")
                table.add_column("ASK Price", justify="right")
                table.add_column("ASK Size", justify="right")
                table.add_column("BID Price", justify="right")
                table.add_column("BID Size", justify="right")
                asks = book.asks[:10] if book else []
                bids = book.bids[:10] if book else []
                # pad rows
                for i in range(max(len(asks), len(bids))):
                    a = asks[i] if i < len(asks) else None
                    b = bids[i] if i < len(bids) else None
                    table.add_row(
                        f"{float(a['price']):.4f}" if a else "",
                        f"{float(a['size']):.2f}" if a else "",
                        f"{float(b['price']):.4f}" if b else "",
                        f"{float(b['size']):.2f}" if b else "",
                    )
                console.print(Panel(table, border_style="gray50"))
            console.print("Type commands below. 'help' to list.")

    def _short(self, tid: Optional[str]):
        if not tid:
            return '-'
        return tid[:10] + '...' + tid[-6:]

    # ===== WebSocket =====
    def start_ws(self):
        tokens = [t for t in [self.yes_token, self.no_token] if t]
        if not tokens:
            return
        # Close any previous socket
        try:
            if self.ws_client and hasattr(self.ws_client, 'close'):
                self.ws_client.close()
        except Exception:
            pass

        def handle_event(event):
            try:
                data = event.json
                events = data if isinstance(data, list) else [data]
                for ev in events:
                    if ev.get('event_type') == 'book':
                        token = ev.get('asset_id') or ev.get('token_id')
                        self.books[token] = OrderBook(
                            bids=ev.get('bids', []),
                            asks=ev.get('asks', []),
                            ts=time.time(),
                        )
                self.render()
            except Exception:
                console.print('[red]WS parse error[/red]')

        def run_socket():
            try:
                self.ws_client = PolymarketWebsocketsClient()
                self.ws_client.market_socket(tokens, process_event=handle_event)
            except Exception as e:
                console.print(f"[red]WS fatal error:[/red] {e}")

        threading.Thread(target=run_socket, daemon=True).start()

    # ===== Orders =====
    def _market_ioc(self, token_id: str, side: str, size_override: Optional[float] = None):
        book = self.books.get(token_id)
        if not book:
            console.print('[yellow]No book yet[/yellow]')
            return
        best = (book.asks[0] if side == BUY else book.bids[0]) if (book.asks or book.bids) else None
        if not best:
            console.print('[yellow]No liquidity[/yellow]')
            return
        price = float(best['price'])
        size = float(size_override or self.size)
        try:
            args = OrderArgs(token_id=token_id, side=side, price=price, size=size, time_in_force='IOC')
            signed = self.client.create_order(args)
            resp = self.client.post_order(signed, OrderType.GTD)
            console.print(f"[green]Order response:[/green] {resp}")
        except Exception as e:
            console.print(f"[red]Order error:[/red] {e}")

    # ===== Commands =====
    def handle_command(self, cmd: str):
        parts = cmd.strip().split()
        if not parts:
            return
        op = parts[0].lower()
        try:
            if op == 'help':
                console.print('Commands: sub <YES> [NO] | buy yes [size] | buy no [size] | sell yes [size] | size <n> | price <p> | info | quit')
            elif op == 'sub':
                self.yes_token = parts[1]
                self.no_token = parts[2] if len(parts) > 2 else None
                self.start_ws()
                self.render()
            elif op == 'buy' and len(parts) >= 2:
                side = parts[1]
                size = float(parts[2]) if len(parts) > 2 else None
                if side == 'yes' and self.yes_token:
                    self._market_ioc(self.yes_token, BUY, size)
                elif side == 'no' and self.no_token:
                    self._market_ioc(self.no_token, BUY, size)
                else:
                    console.print('[red]Token missing[/red]')
            elif op == 'sell' and len(parts) >= 2:
                side = parts[1]
                size = float(parts[2]) if len(parts) > 2 else None
                if side == 'yes' and self.yes_token:
                    self._market_ioc(self.yes_token, SELL, size)
                else:
                    console.print('[red]Only YES sell implemented[/red]')
            elif op == 'size' and len(parts) == 2:
                self.size = float(parts[1])
                self.render()
            elif op == 'price' and len(parts) == 2:
                self.price = float(parts[1])
                self.render()
            elif op == 'info':
                console.print(f"YES: {self.yes_token}\nNO: {self.no_token}\nSize: {self.size}")
            elif op == 'quit':
                self._running = False
            else:
                console.print('[yellow]Unknown command[/yellow]')
        except Exception:
            console.print('[red]Command error[/red]')


def parse_args():
    import argparse
    p = argparse.ArgumentParser(description='Pro Terminal Trader')
    p.add_argument('--yes-token', help='YES outcome token id')
    p.add_argument('--no-token', help='NO outcome token id (optional)')
    p.add_argument('--size', type=float, default=50.0, help='default order size')
    p.add_argument('--price', type=float, default=0.45, help='default price for limit templates')
    p.add_argument('--ws-url', default=DEFAULT_WS_URL, help='WebSocket URL for book stream')
    p.add_argument('--api-url', default=DEFAULT_HTTP_HOST, help='HTTP API base for orders')
    return p.parse_args()


def main():
    args = parse_args()
    trader = ProTrader(args.yes_token, args.no_token, args.size, args.price, args.ws_url, args.api_url)
    if args.yes_token:
        trader.start_ws()
    trader.render()

    completer = WordCompleter(['help','sub','buy','sell','size','price','info','quit'], ignore_case=True)
    session = PromptSession(completer=completer)

    with patch_stdout():
        while trader._running:
            try:
                cmd = session.prompt('> ')
                trader.handle_command(cmd)
            except (EOFError, KeyboardInterrupt):
                break

if __name__ == '__main__':
    print('[DEPRECATED] This script has moved to utils/quickshoot_terminal/pro_trader.py')
    main()

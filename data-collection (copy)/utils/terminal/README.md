Fast Terminal Trader (CLI)

A minimal, fast CLI to watch a Polymarket orderbook and place quick IOC orders with single-key hotkeys.

Requirements
- Python 3.10+
- Install deps from project requirements (py-clob-client, websocket-client, colorama)
- .env variables:
  - CLOB_API_URL (default https://clob.polymarket.com)
  - POLY_WS_URL (default wss://ws.clob.polymarket.com/market)
  - API credentials if your py_clob_client requires them for posting orders

Run
```powershell
python .\utils\terminal\fast_trader.py --yes-token <YES_TOKEN_ID> --no-token <NO_TOKEN_ID>
```

Hotkeys
- y: Buy YES at best ask (IOC)
- n: Buy NO at best ask (IOC)
- s: Sell YES at best bid (IOC)
- a / z: Increase / Decrease size
- p / ;: Increase / Decrease price (template)
- r: Reset (reserved)
- q: Quit

Notes
- The WebSocket subscription prioritizes the YES token; NO token is optional.
- Printing is throttled only by your terminal speed; the app redraws on each incoming book event.
- Order posting uses py_clob_client create_order + post_order (GTD with IOC time_in_force field).

---

Pro Terminal Trader (Interactive TUI)

A richer, interactive console UI that shows a live orderbook panel while you can type commands concurrently.

Requirements
- Python 3.10+
- Install deps from project requirements (py-clob-client, websocket-client, prompt_toolkit, rich)
- .env variables:
  - CLOB_API_URL (default https://clob.polymarket.com)
  - POLY_WS_URL (default wss://ws.clob.polymarket.com/market)
  - API credentials if your py_clob_client requires them for posting orders

Run
```powershell
python .\utils\terminal\pro_trader.py --yes-token <YES_TOKEN_ID> --no-token <NO_TOKEN_ID> --size 50
```

Commands
- help: Show available commands
- sub YES [NO]: Subscribe to tokens
- buy yes [SIZE]: Buy YES at best ask (IOC)
- buy no [SIZE]: Buy NO at best ask (IOC)
- sell yes [SIZE]: Sell YES at best bid (IOC)
- size <NUMBER>: Set default order size
- price <NUMBER>: Set a template price (future use)
- info: Show current tokens and settings
- quit: Exit the program

Notes
- YES-only subscription is recommended; NO token is optional and shown for visibility.
- The UI updates on every incoming book event; input remains responsive thanks to prompt_toolkit.
- Orders are created with py_clob_client and posted with IOC time-in-force semantics.

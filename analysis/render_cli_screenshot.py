"""Renders a real captured session of the simulator's interactive command prompt as a
terminal-style PNG, for the replication appendix figure.

The session below is real output of `target/release/market_sim.exe`, captured by piping
the listed commands into its stdin (see the comment above SESSION). Colours are not
present in the captured text (the `colored` crate disables ANSI codes when stdout is not
a terminal, which is the case for a piped/captured run), so they are re-applied here to
match exactly the `.cyan()/.green()/.red()/.yellow()/.blue()/.dimmed()/.bold()` calls in
`src/inputs/cli.rs` for each of these lines -- this is a faithful reconstruction of what
the same session looks like in a real terminal, not an invented one.

Usage: python analysis/render_cli_screenshot.py
Output: Thesis/figures/fig_cli_screenshot.png
"""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FONT_PATH = r"C:\Windows\Fonts\CascadiaMono.ttf"
FONT_SIZE = 17
OUT = Path(__file__).parent.parent / "Thesis" / "figures" / "fig_cli_screenshot.png"

BG = (18, 18, 20)
FG = (222, 222, 222)
DIM = (128, 128, 128)
CYAN = (86, 214, 219)
GREEN = (92, 214, 92)
RED = (233, 90, 90)
YELLOW = (224, 199, 90)
BLUE = (105, 170, 233)
BOLD_WHITE = (255, 255, 255)

def prompt(mode: str, cmd: str):
    """One prompt line as drawn in the real REPL: 'sim' and '>' dimmed, the mode tag
    in its own colour (blue for CDA, yellow for FBA, bold, matching cli.rs), and the
    command the user typed in the plain foreground colour."""
    tag_color = BLUE if mode == "CDA" else YELLOW
    return [("sim ", DIM, False), (f"[{mode}]", tag_color, True), ("> ", DIM, False), (cmd, FG, False)]


# Each entry is either a plain (text, color, bold) tuple, or (for prompt lines) a list
# of such tuples drawn left to right -- built to exactly match the plain text captured
# from a real run (see analysis/README.md for how to reproduce it: pipe the same ten
# commands into the built binary's stdin).
LINES = [
    ("=" * 70, CYAN, False),
    ("TFG Simulator", CYAN, True),
    ("=" * 70, CYAN, False),
    ("", FG, False),
    prompt("FBA", "engine cda"),
    ("[OK] Switched simulation matching engine mode to: Continuous", CYAN, False),
    ("", FG, False),
    prompt("CDA", "add sell 127.50 10 maker1"),
    ("[OK] Continuous order processed. Instant trades cleared: 0", GREEN, False),
    ("", FG, False),
    prompt("CDA", "add buy 127.50 4 taker1"),
    ("[OK] Continuous order processed. Instant trades cleared: 1", GREEN, False),
    ("   [TRADE] Qty: 4 at Price: 127.500000", FG, False),
    ("", FG, False),
    prompt("CDA", "orderbook"),
    ("---  Current Continuous Order Book State ---", BOLD_WHITE, True),
    ("Best Bid: n/a  |  Best Ask: 127.500000  |  Spread: n/a", FG, False),
    ("Bids (0):", GREEN, True),
    ("Asks (1):", RED, True),
    ("  ID: 1   | User: maker1   | Qty: 6    | Price: 127.500000", RED, False),
    ("--- CDA ---", FG, False),
    ("  quoted_spread_bps        : n/a", FG, False),
    ("  depth_at_best            : 6", FG, False),
    ("  trade_count              : 1", FG, False),
    ("  executed_volume          : 4", FG, False),
    ("  executed_notional        : 510", FG, False),
    ("  fill_rate                : 0.5714", FG, False),
    ("  book_imbalance           : -1.0000", FG, False),
    ("", FG, False),
    prompt("CDA", "engine batch"),
    ("[OK] Switched simulation matching engine mode to: Batch", CYAN, False),
    ("", FG, False),
    prompt("FBA", "add buy 127.40 6 Alejandro"),
    ("[OK] Queued order successfully in FBA discrete window buffer [ID: 3]", GREEN, False),
    ("", FG, False),
    prompt("FBA", "add sell 127.40 9 Jesus"),
    ("[OK] Queued order successfully in FBA discrete window buffer [ID: 4]", GREEN, False),
    ("", FG, False),
    prompt("FBA", "batch"),
    ("---  Current Pending FBA Window Accumulation Buffer ---", BOLD_WHITE, True),
    ("Best Unfilled Buy: 127.400000  |  Best Unfilled Sell: 127.400000", FG, False),
    ("Buy orders (1):", GREEN, True),
    ("  ID: 3   | User: Alejandro | Qty: 6    | Max Limit: 127.400000 USDT", GREEN, False),
    ("Sell orders (1):", RED, True),
    ("  ID: 4   | User: Jesus    | Qty: 9    | Max Limit: 127.400000 USDT", RED, False),
    ("", FG, False),
    prompt("FBA", "clear"),
    ("[FBA Window Closed] Computing Uniform Market Clearing...", CYAN, False),
    ("=" * 62, CYAN, False),
    ("[OK] Uniform Clearing Calculated Successfully!", GREEN, False),
    ("   Execution Rate (Uniform Price) : 127.400000 USDT", FG, False),
    ("   Total Executed Asset Mass       : 6 units", FG, False),
    ("", FG, False),
    ("Detailed Execution Trade Log:", FG, False),
    ("   Match ID #1   | Alejandro (Order #3) bought 6 units from Jesus (Order #4) @ 127.400000 USDT", FG, False),
    ("[INFO] 1 order(s) left unexecuted at this clearing price -- rolled over to the next window.", YELLOW, False),
    ("", FG, False),
    prompt("FBA", "log"),
    ("=" * 74, CYAN, False),
    ("SYSTEM HISTORICAL EXECUTION LOG".center(74), CYAN, True),
    ("=" * 74, CYAN, False),
    ("  Trade ID   | Eng  | Buyer        | Seller       | Quantity   | Price", BOLD_WHITE, True),
    ("  " + "-" * 72, FG, False),
    ("  #1         | CDA  | taker1       | maker1       | 4          | 127.500000", FG, False),
    ("  #1         | FBA  | Alejandro    | Jesus        | 6          | 127.400000", FG, False),
    ("=" * 74, CYAN, False),
    ("", FG, False),
    prompt("FBA", "exit"),
    ("Terminating simulator core workspace...", DIM, False),
]


def rows():
    """Normalises every LINES entry to a list of (text, color, bold) segments."""
    for entry in LINES:
        yield entry if isinstance(entry, list) else [entry]


TITLEBAR_H = 34
TITLEBAR_BG = (45, 45, 48)


def main():
    font = ImageFont.truetype(FONT_PATH, FONT_SIZE)
    bold_font = font  # Cascadia Mono ships as one weight here; bold is drawn via colour/emphasis only
    title_font = ImageFont.truetype(FONT_PATH, 14)
    line_h = FONT_SIZE + 7
    pad = 18
    max_w = max(sum(font.getlength(t) for t, _, _ in row) for row in rows())
    w = int(max_w) + 2 * pad
    h = line_h * len(LINES) + 2 * pad + TITLEBAR_H

    img = Image.new("RGB", (w, h), BG)
    draw = ImageDraw.Draw(img)

    # A window title bar, so the figure reads unambiguously as a terminal window and
    # not just coloured text: three (non-functional) traffic-light dots and a label.
    draw.rectangle([0, 0, w, TITLEBAR_H], fill=TITLEBAR_BG)
    for i, c in enumerate([(237, 106, 94), (245, 191, 79), (97, 194, 84)]):
        draw.ellipse([16 + i * 22, TITLEBAR_H // 2 - 6, 16 + i * 22 + 12, TITLEBAR_H // 2 + 6], fill=c)
    title = "market_sim.exe -- interactive prompt"
    draw.text(((w - title_font.getlength(title)) / 2, TITLEBAR_H // 2 - 7), title, font=title_font, fill=(200, 200, 200))

    y = pad + TITLEBAR_H
    for row in rows():
        x = pad
        for text, color, bold in row:
            f = bold_font if bold else font
            draw.text((x, y), text, font=f, fill=color)
            x += f.getlength(text)
        y += line_h

    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT)
    print(f"wrote {OUT} ({w}x{h})")


if __name__ == "__main__":
    main()

"""Interactive CLI to find token IDs by market slug or event slug.

Usage:
  - Run without args to enter interactive mode:
      python -m data_collection.utils.market.cli_find_tokens
  - Or pass `--slug` to run once non-interactively:
      python -m data_collection.utils.market.cli_find_tokens --slug hype-updown-5m-1785762000

This script reuses the project's `market_search` utilities which call the
Gamma API for comprehensive market data.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path
import json
from typing import List

# Make local package imports work when running as a module or script
THIS_FILE = Path(__file__).resolve()
DATA_COLLECTION_ROOT = THIS_FILE.parent
WORKSPACE_ROOT = DATA_COLLECTION_ROOT.parent
for p in (DATA_COLLECTION_ROOT, WORKSPACE_ROOT):
    if str(p) not in sys.path:
        sys.path.insert(0, str(p))

try:
    # Prefer lightweight Gamma API helpers when available to avoid fetching
    # the entire market catalog (which is slow and unnecessary).
    from data_collection.utils.gamma_api.get_markets_by_slug import get_markets_by_slug
    from data_collection.utils.gamma_api.get_events_by_slug import get_event_by_slug
    GAMMA_HELPERS = True
except Exception:
    # Fall back to market_search functions (legacy). These may perform
    # comprehensive fetches; use only if gamma-api helpers are unavailable.
    GAMMA_HELPERS = False
    try:
        from market_search import (
            get_market_by_slug,
            search_markets_by_event_slug,
            get_token_ids_only,
            search_markets,
        )
    except Exception:
        from data_collection.utils.market.market_search import (
            get_market_by_slug,
            search_markets_by_event_slug,
            get_token_ids_only,
            search_markets,
        )

    # If the folder is named with a hyphen (gamma-api) the above import may fail
    # in some layouts. Try a file-based import fallback to avoid breaking.
    if not GAMMA_HELPERS:
        try:
            import importlib.util
            from pathlib import Path

            base = Path(__file__).resolve().parents[2] / "utils"
            alt = base / "gamma-api"
            if alt.exists():
                spec = importlib.util.spec_from_file_location(
                    "gamma_api.get_markets_by_slug", str(alt / "get_markets_by_slug.py")
                )
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                get_markets_by_slug = getattr(mod, "get_markets_by_slug")

                spec2 = importlib.util.spec_from_file_location(
                    "gamma_api.get_events_by_slug", str(alt / "get_events_by_slug.py")
                )
                mod2 = importlib.util.module_from_spec(spec2)
                spec2.loader.exec_module(mod2)
                get_event_by_slug = getattr(mod2, "get_event_by_slug")
                GAMMA_HELPERS = True
        except Exception:
            pass


def pretty_print_tokens(slug: str, token_ids: List[str]) -> None:
    print(f"\nMarket slug: {slug}")
    if not token_ids:
        print("  (no token ids found)")
        return
    for i, t in enumerate(token_ids, 1):
        print(f"  {i}. {t}")


def _normalize_clob_token_ids(token_ids) -> List[str]:
    """Normalize clobTokenIds which may be a JSON string, list, or None."""
    if token_ids is None:
        return []
    if isinstance(token_ids, list):
        return token_ids
    if isinstance(token_ids, str):
        # Try to parse JSON array string
        try:
            parsed = json.loads(token_ids)
            if isinstance(parsed, list):
                return parsed
            # If it's a single string, return as single-element list
            return [str(parsed)]
        except Exception:
            # Fallback: return the raw string as single token id
            return [token_ids]
    # Unknown type
    return [str(token_ids)]


def lookup_and_print(slug: str) -> None:
    """Try exact market slug, then event slug search, then text search."""
    slug = slug.strip()
    if not slug:
        print("Empty input")
        return

    # 1) Exact market slug: prefer direct Gamma API lookup (no full fetch)
    if GAMMA_HELPERS:
        try:
            data = get_markets_by_slug(slug, fields=["slug", "clobTokenIds", "question"])
            # Gamma returns a full market dict; extract and normalize clobTokenIds
            token_ids = _normalize_clob_token_ids(data.get("clobTokenIds"))
            pretty_print_tokens(data.get("slug", slug), token_ids)
            return
        except Exception:
            # Not found or error; fall through to event search
            pass

    market = get_market_by_slug(slug)
    if market:
        token_ids = market.get("clobTokenIds", [])
        pretty_print_tokens(market.get("slug", slug), token_ids)
        return

    # 2) Event slug search: try Gamma API event endpoint first (no full fetch)
    if GAMMA_HELPERS:
        try:
            evt = get_event_by_slug(slug, fields=["markets", "slug", "title"])
            markets = evt.get("markets") or []
            if markets:
                print(f"\nFound {len(markets)} markets for event slug '{slug}':")
                for m in markets:
                    tok = _normalize_clob_token_ids(m.get("clobTokenIds", []))
                    pretty_print_tokens(m.get("slug", "<no-slug>"), tok)
                return
        except Exception:
            pass

    markets = search_markets_by_event_slug(slug)
    if markets:
        print(f"\nFound {len(markets)} markets for event slug '{slug}':")
        for i, toklist in enumerate(markets, 1):
            pretty_print_tokens(f"market-{i}", toklist)
        return

    # 3) Text search fallback
    results = search_markets(slug, limit=10)
    if results:
        print(f"\nSearch found {len(results)} markets matching '{slug}':")
        for r in results:
            pretty_print_tokens(r.get("slug", "<no-slug>"), r.get("clobTokenIds", []))
        return

    print(f"No markets found for '{slug}'")


def main() -> None:
    parser = argparse.ArgumentParser(description="Find Polymarket token IDs by market or event slug")
    parser.add_argument("--slug", type=str, help="Market slug or event slug to lookup", default=None)
    args = parser.parse_args()

    if args.slug:
        lookup_and_print(args.slug)
        return

    print("Enter a market slug (exact) or an event slug. Blank to exit.")
    try:
        while True:
            user = input("slug> ").strip()
            if not user:
                break
            lookup_and_print(user)
    except (KeyboardInterrupt, EOFError):
        print("\nExiting")


if __name__ == "__main__":
    main()

"""Lightweight tests for Gamma API helpers used by the CLI.

These tests call only the direct Gamma endpoints (single market / single event)
and avoid the comprehensive `get_all_active_markets()` path.

Run:
  python -m data_collection.utils.market.test_gamma_api_cli
"""
from __future__ import annotations

import sys
from typing import Any

try:
    from data_collection.utils.gamma_api.get_markets_by_slug import get_markets_by_slug
    from data_collection.utils.gamma_api.get_events_by_slug import get_event_by_slug
except Exception:
    # Try file-based import (gamma-api folder may be hyphenated)
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
        else:
            print("Gamma API helpers not available; aborting test")
            raise
    except Exception:
        print("Gamma API helpers not available; aborting test")
        raise


def assert_market_structure(mkt: dict[str, Any]) -> None:
    assert isinstance(mkt, dict), "market must be a dict"
    assert "slug" in mkt, "market missing slug"
    # clobTokenIds may be missing for some markets; allow empty but must be list if present
    if "clobTokenIds" in mkt:
        val = mkt["clobTokenIds"]
        assert val is None or isinstance(val, (list, str)), "clobTokenIds must be a list or string or None"


def test_get_market_by_slug(slug: str) -> None:
    print(f"Testing get_markets_by_slug('{slug}')")
    m = get_markets_by_slug(slug)
    assert_market_structure(m)
    print("  OK: market fetched (keys):", list(m.keys()))


def test_get_event_by_slug(slug: str) -> None:
    print(f"Testing get_event_by_slug('{slug}')")
    e = get_event_by_slug(slug)
    assert isinstance(e, dict), "event must be a dict"
    markets = e.get("markets") or []
    print(f"  OK: event fetched, markets count: {len(markets)}")
    # spot-check first market structure if present
    if markets:
        assert_market_structure(markets[0])


def main() -> None:
    # Use a known market slug the project used earlier
    market_slug = "hype-updown-5m-1785762000"
    event_slug = "hype-updown-5m-1785762000"

    try:
        test_get_market_by_slug(market_slug)
    except AssertionError as ae:
        print("Market assertion failed:", ae)
    except Exception as e:
        print("Market fetch error:", e)

    try:
        test_get_event_by_slug(event_slug)
    except AssertionError as ae:
        print("Event assertion failed:", ae)
    except Exception as e:
        print("Event fetch error:", e)


if __name__ == "__main__":
    main()

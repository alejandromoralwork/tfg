"""Does the simulated CDA reproduce the venue's own executions? (whole month)

The archive records, for every order, a `filled` event with the size left after the fill. An order that
is filled at the moment it is submitted is a taker (its `timestampDiff` is 0). Every trade has exactly one
taker, so the sum of the taker fills is the volume the venue traded (checked on eight real trades in
the appendix). verify_raw_data.py writes it per second and side. This script compares it with the executed
volume of the two simulated engines.

A second estimate comes from the maker side: makers that were filled completely (their `filled` records)
plus the part of maker fills that only becomes visible when the order is cancelled with a smaller size.
The first is a lower bound, the sum of both an upper bound for the maker-side volume; the taker-side sum
must lie between them.

The simulator never reads `filled` events, so all of this is independent of the simulation.

Output: analysis/output/verification_replay_fidelity.json
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pandas as pd

REPO = Path(__file__).resolve().parents[1]
RES = REPO / "results" / "sol"
OUT = Path(__file__).parent / "output"
SCALE = 1e6


def corr(a, b):
    return float(np.corrcoef(a, b)[0, 1])


def block_sum(x, k):
    n = len(x) // k * k
    return x[:n].reshape(-1, k).sum(axis=1)


def main():
    z = np.load(OUT / "verification_venue_volume.npz")
    raw = json.loads((OUT / "verification_raw.json").read_text())
    tb, ts_ = z["taker_buy"] / SCALE, z["taker_sell"] / SCALE
    venue = tb + ts_                                       # every trade has one taker
    cda = pd.read_csv(RES / "cda_timeseries.csv", usecols=["executed_volume", "trade_count"])
    fba = pd.read_csv(RES / "fba_timeseries.csv", usecols=["executed_volume", "trade_count"])
    c, f = cda["executed_volume"].to_numpy(float), fba["executed_volume"].to_numpy(float)
    all_fills = (z["buy"].sum() + z["sell"].sum()) / SCALE
    maker_completed = all_fills - venue.sum()
    st2 = (raw["reduction_volume_status_2_buy_1e6"] + raw["reduction_volume_status_2_sell_1e6"]) / SCALE
    out = {
        "venue_taker_volume": float(venue.sum()), "venue_taker_buy": float(tb.sum()), "venue_taker_sell": float(ts_.sum()),
        "venue_maker_completed": float(maker_completed),
        "venue_maker_upper": float(maker_completed + st2),
        "venue_cancel_reductions_status2": float(st2),
        "buyer_side_fills": float(z["buy"].sum() / SCALE), "seller_side_fills": float(z["sell"].sum() / SCALE),
        "venue_fill_events": int(z["events"].sum()),
        "cda_volume": float(c.sum()), "fba_volume": float(f.sum()),
        "cda_trades": int(cda["trade_count"].sum()), "fba_trades": int(fba["trade_count"].sum()),
        "cda_over_venue": float(c.sum() / venue.sum()), "fba_over_venue": float(f.sum() / venue.sum()),
    }
    for label, k in (("second", 1), ("minute", 60), ("hour", 3600), ("day", 86400)):
        v, cc, ff = block_sum(venue, k), block_sum(c, k), block_sum(f, k)
        out[f"corr_{label}"] = {"venue_vs_cda": corr(v, cc), "venue_vs_fba": corr(v, ff), "cda_vs_fba": corr(cc, ff), "n": int(len(v))}
    day_rows = []
    for d in range(31):
        s = slice(d * 86400, (d + 1) * 86400)
        day_rows.append({"day": d + 1, "venue": float(venue[s].sum()), "cda": float(c[s].sum()), "fba": float(f[s].sum())})
    out["daily"] = day_rows
    r = np.array([x["cda"] / x["venue"] for x in day_rows])
    out["daily_cda_over_venue"] = {"min": float(r.min()), "median": float(np.median(r)), "max": float(r.max())}
    # share of the simulated volume that falls in seconds where the venue had no taker fill at all
    no_venue = venue == 0
    out["cda_volume_in_seconds_without_venue_trade_share"] = float(c[no_venue].sum() / c.sum())
    out["seconds_with_venue_trade"] = int((~no_venue).sum())
    out["seconds_with_cda_trade"] = int((c > 0).sum())
    (OUT / "verification_replay_fidelity.json").write_text(json.dumps(out, indent=1))
    print(json.dumps({k: v for k, v in out.items() if k != "daily"}, indent=1))


if __name__ == "__main__":
    main()

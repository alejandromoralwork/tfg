"""Were the two books ever emptied in the middle of the month run?

The simulator saves a checkpoint after every input file. If a run is stopped and started again it
continues from the checkpoint, but `simulate_cmd.rs` creates the engines again ("books start fresh even
on resume"): every resting order and every pending batch order is lost and the batch clock restarts.
The output files do not say whether that happened, so it is measured here from the results.

Signature of an emptied CDA book: the average depth of the whole book (`total_book_depth`) falls by
more than 40 percent from one minute to the next, exactly at the start of an hour (the moment a new
input file starts), and rebuilds only slowly as new orders arrive. A restarted FBA batch clock shows
as a change in how many one-second buckets contain trades of two different batches
(`intra_interval_price_dispersion` above zero).

Output: analysis/output/verification_resets.json
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pandas as pd

RES = Path(__file__).resolve().parents[1] / "results" / "sol"
OUT = Path(__file__).parent / "output"


def main():
    c = pd.read_csv(RES / "cda_timeseries.csv", usecols=["interval_start_ns", "total_book_depth"])
    t = pd.to_datetime(c["interval_start_ns"], unit="ns")
    depth = pd.Series(c["total_book_depth"].to_numpy(), index=t).resample("1min").mean()
    before = depth.rolling(10, min_periods=5).mean().shift(1)
    ratio = depth / before
    at_hour = ratio[(ratio.index.minute == 0) & (ratio < 0.6)]
    events = []
    for ts, r in at_hour.items():
        pre = float(before[ts])
        # minutes until the depth is back above 90 percent of the pre-drop level (within 6 hours), else None
        after = depth[ts: ts + pd.Timedelta(hours=6)]
        back = after[after >= 0.9 * pre]
        events.append({"time": str(ts), "depth_before": pre, "depth_first_minute": float(depth[ts]), "ratio": float(r),
                       "minutes_to_recover_90pct": None if back.empty else int((back.index[0] - ts).total_seconds() // 60),
                       "depth_after_1h": float(depth[ts + pd.Timedelta(hours=1)]) if ts + pd.Timedelta(hours=1) in depth.index else None})
    f = pd.read_csv(RES / "fba_timeseries.csv", usecols=["interval_start_ns", "intra_interval_price_dispersion"])
    tf = pd.to_datetime(f["interval_start_ns"], unit="ns")
    disp = f["intra_interval_price_dispersion"]
    multi = disp > 1e-6
    first_multi = str(tf[multi].iloc[0])
    day = pd.DataFrame({"n": disp.notna().to_numpy(), "nz": multi.to_numpy()}, index=tf).resample("D").sum()
    before_first = tf < pd.Timestamp(first_multi)
    out = {
        "reset_events": events,
        "n_events": len(events),
        "first_event": events[0]["time"] if events else None,
        "last_event": events[-1]["time"] if events else None,
        "fba_multi_price_buckets": int(multi.sum()),
        "fba_trading_buckets": int(disp.notna().sum()),
        "fba_first_multi_price_bucket": first_multi,
        "fba_trading_buckets_before_first": int(disp[before_first].notna().sum()),
        "fba_multi_price_before_first": int(multi[before_first].sum()),
        "fba_multi_price_share_after_first": float(multi[~before_first].sum() / disp[~before_first].notna().sum()),
        "book_depth_daily_median": {str(k.date()): float(v) for k, v in depth.resample("D").median().items()},
    }
    (OUT / "verification_resets.json").write_text(json.dumps(out, indent=1))
    print(json.dumps({k: v for k, v in out.items() if k != "book_depth_daily_median"}, indent=1))


if __name__ == "__main__":
    main()

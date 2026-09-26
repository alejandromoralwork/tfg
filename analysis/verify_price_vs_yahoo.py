"""Are the simulated prices close to the real market price of SOL?

Downloads hourly SOL-USD candles for December 2025 from Yahoo Finance (public chart endpoint; hourly is the
finest resolution Yahoo keeps for a month this old), computes the volume-weighted average price of each
simulated engine for every hour from the result files, and compares.

  sim price  = sum(executed_notional) / sum(executed_volume) over the hour, for each engine (USD)
  reference  = Yahoo typical price (open + high + low + close) / 4 of the same hour

Caveats stated in the appendix: Yahoo SOL-USD is a spot composite, the simulated venue is a perpetual
future, so a small basis is expected; hours without simulated trades are skipped.

Output: analysis/output/verification_price_yahoo.json and Thesis/figures/fig_pricevsyahoo.png
"""
from __future__ import annotations

import json
import urllib.request
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

HERE = Path(__file__).parent
RES = HERE.parent / "results" / "sol"
CACHE = HERE / "output" / "yahoo_sol_usd_1h.json"
FIG = HERE.parent / "Thesis" / "figures" / "fig_pricevsyahoo.png"
CDA_COLOR, FBA_COLOR, REF_COLOR = "#1b9e77", "#d95f02", "#333333"


def yahoo():
    if not CACHE.exists():
        url = "https://query1.finance.yahoo.com/v8/finance/chart/SOL-USD?period1=1764460800&period2=1767398400&interval=1h"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        CACHE.write_text(urllib.request.urlopen(req, timeout=60).read().decode("utf8"))
    r = json.loads(CACHE.read_text())["chart"]["result"][0]
    q = r["indicators"]["quote"][0]
    df = pd.DataFrame({"t": pd.to_datetime(r["timestamp"], unit="s"), "open": q["open"], "high": q["high"], "low": q["low"], "close": q["close"]}).dropna()
    df["typical"] = (df.open + df.high + df.low + df.close) / 4
    return df.set_index("t")


def sim_hourly(engine):
    d = pd.read_csv(RES / f"{engine}_timeseries.csv", usecols=["interval_start_ns", "executed_volume", "executed_notional"])
    d["t"] = pd.to_datetime(d["interval_start_ns"], unit="ns").dt.floor("h")
    g = d.groupby("t").agg(vol=("executed_volume", "sum"), notional=("executed_notional", "sum"))
    g = g[g.vol > 0]
    return (g.notional / g.vol / 1e6).rename(engine)          # notional is in 1e-6 USD units


def stats(dev_bps, inside):
    a = dev_bps.abs()
    return {"hours": int(len(dev_bps)), "mean_bps": float(dev_bps.mean()), "mean_abs_bps": float(a.mean()), "median_abs_bps": float(a.median()),
            "p95_abs_bps": float(a.quantile(0.95)), "max_abs_bps": float(a.max()), "rmse_bps": float(np.sqrt((dev_bps ** 2).mean())),
            "share_inside_candle_range": float(inside.mean())}


def main():
    y = yahoo()
    y = y[(y.index >= "2025-12-01") & (y.index < "2026-01-01")]
    out, series = {}, {}
    for eng in ("cda", "fba"):
        s = sim_hourly(eng)
        j = pd.concat([s, y], axis=1, join="inner")
        dev = (j[eng] / j["typical"] - 1) * 1e4
        inside = (j[eng] >= j["low"] * 0.9999) & (j[eng] <= j["high"] * 1.0001)
        out[eng] = stats(dev, inside)
        series[eng] = (j, dev)
    both = pd.concat([series["cda"][0]["cda"], series["fba"][0]["fba"]], axis=1, join="inner")
    out["cda_vs_fba_mean_abs_bps"] = float(((both.cda / both.fba - 1) * 1e4).abs().mean())
    out["yahoo_hours"] = int(len(y))
    out["yahoo_month_range"] = [float(y.low.min()), float(y.high.max())]
    # daily correlation of hourly returns (does the simulated price move with the market?)
    ret = pd.concat([series["cda"][0]["cda"].pct_change(), series["fba"][0]["fba"].pct_change(), series["cda"][0]["typical"].pct_change()], axis=1, keys=["cda", "fba", "yahoo"]).dropna()
    out["hourly_return_corr_cda_yahoo"] = float(ret.cda.corr(ret.yahoo))
    out["hourly_return_corr_fba_yahoo"] = float(ret.fba.corr(ret.yahoo))
    (HERE / "output" / "verification_price_yahoo.json").write_text(json.dumps(out, indent=1))

    fig, axes = plt.subplots(2, 1, figsize=(9.5, 6.2), sharex=True, gridspec_kw={"height_ratios": [2, 1]})
    ax = axes[0]
    ax.fill_between(y.index, y.low, y.high, color="#cccccc", label="Yahoo hourly range (low to high)")
    ax.plot(y.index, y.typical, color=REF_COLOR, linewidth=0.8, label="Yahoo SOL-USD, typical price")
    ax.plot(series["cda"][0].index, series["cda"][0]["cda"], color=CDA_COLOR, linewidth=0.8, label="CDA, hourly VWAP")
    ax.plot(series["fba"][0].index, series["fba"][0]["fba"], color=FBA_COLOR, linewidth=0.8, label="FBA, hourly VWAP")
    ax.set_ylabel("SOL price (USD)")
    ax.set_title("Simulated and real SOL price, December 2025")
    ax.legend(frameon=False, ncol=2, fontsize=8)
    ax = axes[1]
    ax.axhline(0, color="#999999", linewidth=0.6)
    ax.plot(series["cda"][1].index, series["cda"][1], color=CDA_COLOR, linewidth=0.7, label="CDA")
    ax.plot(series["fba"][1].index, series["fba"][1], color=FBA_COLOR, linewidth=0.7, label="FBA")
    ax.set_ylabel("Simulated minus Yahoo (bps)")
    ax.legend(frameon=False, ncol=2, fontsize=8)
    fig.autofmt_xdate()
    fig.tight_layout()
    fig.savefig(FIG, dpi=150, bbox_inches="tight")
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()

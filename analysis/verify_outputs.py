"""Invariants that must hold in every row of the two month-long result files.

Reads results/sol/{fba,cda}_timeseries.csv (2,678,400 rows each) and counts, for each
invariant, how many rows were checked and how many violate it. Invariants come from
the definitions of the metrics, not from the code that computes them.

Output: analysis/output/verification_outputs.json
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pandas as pd

REPO = Path(__file__).resolve().parents[1]
RES = REPO / "results" / "sol"
OUT = Path(__file__).parent / "output"
W = 1_000_000_000


def checkpoint_anchor() -> int:
    for line in (RES / "checkpoint.txt").read_text().splitlines():
        if line.startswith("anchor "):
            return int(line.split()[1])
    raise RuntimeError


def rows(name: str, checked: int, violations: int, results: list):
    results.append({"invariant": name, "rows_checked": int(checked), "violations": int(violations)})


def main():
    anchor = checkpoint_anchor()
    report = {}
    for eng in ("fba", "cda"):
        df = pd.read_csv(RES / f"{eng}_timeseries.csv")
        R: list = []
        n = len(df)
        start = df["interval_start_ns"].to_numpy(dtype=np.int64)
        rows("grid: first bucket starts at the first record", 1, start[0] != anchor, R)
        rows("grid: consecutive buckets exactly 1 s apart", n - 1, (np.diff(start) != W).sum(), R)
        rows("grid: bucket width is 1 s in every row", n, (df["interval_width_ns"] != W).sum(), R)
        rows("grid: 31 x 86,400 rows", 1, n != 31 * 86_400, R)

        vol = df["executed_volume"].to_numpy(dtype=float)
        notional = df["executed_notional"].to_numpy(dtype=float)
        vwap = df["vwap"].to_numpy(dtype=float)
        traded = vol > 0
        rows("trades: vwap present exactly when volume > 0", n, (traded != ~np.isnan(vwap)).sum(), R)
        rel = np.abs(vwap[traded] * vol[traded] - notional[traded]) / notional[traded]
        rows("trades: vwap x volume = notional (7-digit CSV precision)", traded.sum(), (rel > 2e-6).sum(), R)
        rows("trades: trade_count > 0 exactly when volume > 0", n, ((df["trade_count"] > 0) != traded).sum(), R)
        rows("trades: volume >= trade count (whole-unit trades)", traded.sum(), (vol[traded] < df["trade_count"].to_numpy()[traded]).sum(), R)

        # effective - realized = impact, for every horizon
        for h in ("1s", "5s", "30s"):
            e, r, p = (df[c].to_numpy(dtype=float) for c in ("effective_spread_bps", f"realized_spread_bps_{h}", f"price_impact_bps_{h}"))
            have = ~np.isnan(e) & ~np.isnan(r)
            rows(f"identity: impact_{h} = effective - realized_{h}", have.sum(), (np.abs(p[have] - (e[have] - r[have])) > 2e-6).sum(), R)
            rows(f"identity: impact_{h} present exactly when effective and realized are", n, (have != ~np.isnan(p)).sum(), R)

        q = df["quoted_spread_bps"].to_numpy(dtype=float)
        ok = ~np.isnan(q)
        rows("book: quoted spread strictly positive (book never crossed)", ok.sum(), (q[ok] <= 0).sum(), R)
        for col, lo, hi in (("book_imbalance", -1, 1), ("boundary_concentration", 0, 1), ("unexecuted_residual_share", 0, 1)):
            v = df[col].to_numpy(dtype=float)
            ok = ~np.isnan(v)
            rows(f"range: {col} within [{lo}, {hi}]", ok.sum(), ((v[ok] < lo) | (v[ok] > hi)).sum(), R)
        for col in ("intra_interval_price_dispersion", "trader_surplus", "amihud_illiquidity", "realized_volatility",
                    "effective_spread_bps", "executed_volume", "executed_notional", "depth_at_best"):
            v = df[col].to_numpy(dtype=float)
            ok = ~np.isnan(v)
            rows(f"range: {col} >= 0", ok.sum(), (v[ok] < 0).sum(), R)
        if eng == "cda":
            w10, w50, w100, tot = (df[c].to_numpy(dtype=float) for c in ("depth_within_10bps", "depth_within_50bps", "depth_within_100bps", "total_book_depth"))
            ok = ~np.isnan(w10)
            rows("depth: within 10 bps <= within 50 bps", ok.sum(), (w10[ok] > w50[ok] + 1e-3).sum(), R)
            rows("depth: within 50 bps <= within 100 bps", ok.sum(), (w50[ok] > w100[ok] + 1e-3).sum(), R)
            rows("depth: within 100 bps <= total book depth", ok.sum(), (w100[ok] > tot[ok] + 1e-3).sum(), R)
            rows("depth: no FBA-only column filled (boundary, residual)", n, (df["boundary_concentration"].notna() | df["unexecuted_residual_share"].notna()).sum(), R)
        else:
            rows("structure: CDA-only columns empty (book imbalance, total depth)", n, (df["book_imbalance"].notna() | df["total_book_depth"].notna()).sum(), R)
            disp = df["intra_interval_price_dispersion"].to_numpy(dtype=float)
            ok = ~np.isnan(disp)
            report["fba_dispersion_exactly_zero_share"] = float((disp[ok] == 0).mean())
            rows("uniform price: dispersion is zero (rows where it is not: buckets with two batch clears)", ok.sum(), (disp[ok] != 0).sum(), R)
        rows("structure: pricing error empty in every row (no oracle in the data)", n, df["pricing_error_bps"].notna().sum(), R)

        fr = df["fill_rate"].to_numpy(dtype=float)
        ok = ~np.isnan(fr)
        over = ok & (fr > 1 + 1e-9)
        rows("range: fill_rate within [0, 1]", ok.sum(), over.sum(), R)
        report[f"{eng}_fill_rate_over_one"] = {
            "rows": int(over.sum()), "max": float(np.nanmax(fr)),
            "start_ns": [int(x) for x in start[over]][:10],
            "mean_fill_rate": float(np.nanmean(fr)),
            "mean_fill_rate_without_them": float(np.nanmean(fr[~over])),
        }
        report[eng] = R
        report[f"{eng}_rows"] = n

    # trade totals: sum of the columns against the checkpoint summary lines
    ck = (RES / "checkpoint.txt").read_text().splitlines()
    for eng in ("fba", "cda"):
        line = next(l for l in ck if l.startswith(f"{eng}_summary "))
        rows_ck, trades_ck, vol_ck, not_ck = (int(x) for x in line.split(" ", 1)[1].split(";")[:4])
        df = pd.read_csv(RES / f"{eng}_timeseries.csv", usecols=["trade_count", "executed_volume"])
        report[f"{eng}_totals_vs_checkpoint"] = {
            "rows": [len(df), rows_ck], "trades": [int(df.trade_count.sum()), trades_ck], "volume": [int(df.executed_volume.sum()), vol_ck],
        }
    OUT.mkdir(exist_ok=True)
    (OUT / "verification_outputs.json").write_text(json.dumps(report, indent=1))
    for eng in ("fba", "cda"):
        print(f"== {eng}")
        for r in report[eng]:
            print(f"  {r['violations']:>9,} / {r['rows_checked']:>9,}  {r['invariant']}")
        print("  fill_rate>1:", report[f"{eng}_fill_rate_over_one"])
        print("  totals:", report[f"{eng}_totals_vs_checkpoint"])


if __name__ == "__main__":
    main()

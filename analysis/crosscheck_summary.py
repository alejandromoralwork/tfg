"""Cross-check: recompute per-column count/mean/min/max from the timeseries CSVs
(pure pandas) and compare with the running summary the Rust recorder wrote into
results/sol/checkpoint.txt (folded incrementally during the replay)."""
import pandas as pd
import config

cols = ["quoted_spread_bps", "depth_at_best", "effective_spread_bps",
        "realized_spread_bps_5s", "price_impact_bps_5s", "amihud_illiquidity",
        "kyle_lambda", "intra_interval_price_dispersion", "executed_volume",
        "executed_notional", "vwap", "trade_count"]

ck = {}
for line in config.CHECKPOINT_TXT.read_text().splitlines():
    if line.startswith(("fba_summary ", "cda_summary ")):
        k, v = line.split(" ", 1)
        ck[k[:3]] = v

def parse(s):
    parts = s.split(";")
    head = parts[:4]  # rows;trades;volume;notional
    out = {}
    for p in parts[4:]:
        name, rest = p.split("=")
        n, total, mn, mx = rest.split(":")
        out[name] = (int(n), float(total), float(mn), float(mx))
    return head, out

worst = 0.0
for eng, path in (("fba", config.FBA_CSV), ("cda", config.CDA_CSV)):
    head, summ = parse(ck[eng])
    df = pd.read_csv(path, usecols=[c for c in cols if c != "trade_count"] + ["trade_count"])
    print(f"== {eng}: rows {len(df):,} (checkpoint {head[0]}), trades {int(df.trade_count.sum()):,} (checkpoint {head[1]})")
    for c in cols:
        if c not in summ or c == "trade_count":
            continue
        n, total, mn, mx = summ[c]
        s = df[c].dropna()
        ok_n = len(s) == n
        rel = abs(s.sum() - total) / max(abs(total), 1e-12)
        worst = max(worst, rel)
        print(f"{c:34s} n {'OK' if ok_n else 'DIFF'} ({len(s):,} vs {n:,})  sum rel.err {rel:.2e}  "
              f"min {s.min():.6g}/{mn:.6g}  max {s.max():.6g}/{mx:.6g}")
print("worst relative error in sums:", worst)

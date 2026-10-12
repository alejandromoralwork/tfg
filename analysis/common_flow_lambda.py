"""Kyle's lambda for both engines against one common order-flow regressor.

The simulator's own `kyle_lambda` column is not comparable across engines:
the CDA regresses on one taker sweep's executed flow, the FBA on the net
quantity of every order waiting in the batch, including the whole
rolled-over residual (src/metrics/timeseries.rs, `net_order_flow`). The two
regressors are on scales orders of magnitude apart.

This module gives both engines the same regressor: the venue's own net
signed taker flow per second,

    x_t = (taker_buy_t - taker_sell_t) / 1e6 / 1e3    (1,000 SOL, + = net buying)

from analysis/output/verification_venue_volume.npz (written by
verify_raw_data.py from the raw `filled` records; the simulator never reads
those, so x is independent of both engines). Each engine's price change is
read from its own CSV VWAP, carried forward over seconds without a trade:

    y_t(h) = (P_{t+h} - P_{t-1}) / P_{t-1} * 1e4      (bps), h in {0, 5} s

and lambda_e = sum(x y_e) / sum(x^2), OLS through the origin, over the
seconds where x != 0 and both engines' y exist. Since x and the sample are
shared, regressing d = y_fba - y_cda on x gives lambda_fba - lambda_cda
exactly; its HAC standard error is the test of the difference. Flow is in
thousands of SOL, so lambda is in bps per 1,000 SOL (per-SOL values are
~1e-4 and would print as 0.00).
"""

import numpy as np
import pandas as pd
import statsmodels.api as sm

import config
import paired_stats

HORIZONS = {"common_flow_lambda": 0, "common_flow_lambda_5s": 5}
N_BINS = 20
NS_PER_SEC = 1_000_000_000
FLOW_UNIT_SOL = 1_000


def load_flow(n_rows):
    z = np.load(config.OUTPUT_DIR / "verification_venue_volume.npz")
    x = (z["taker_buy"].astype(np.float64) - z["taker_sell"].astype(np.float64)) / 1e6 / FLOW_UNIT_SOL
    assert len(x) == n_rows, f"venue flow has {len(x)} rows, timeseries {n_rows}"
    return x


def price_changes(price, h):
    """price: per-second traded price with NaN where nothing traded."""
    p = pd.Series(price).ffill().to_numpy(dtype=np.float64)
    prev = np.full_like(p, np.nan)
    prev[1:] = p[:-1]
    ahead = np.full_like(p, np.nan)
    ahead[: len(p) - h] = p[h:]
    with np.errstate(invalid="ignore", divide="ignore"):
        return (ahead - prev) / prev * 1e4


def slope(x, y):
    return float(np.dot(x, y) / np.dot(x, x))


def estimate(x, y_cda, y_fba):
    n = len(x)
    lags = paired_stats.newey_west_lags(n)
    res = sm.OLS(y_fba - y_cda, x).fit(cov_type="HAC", cov_kwds={"maxlags": lags})
    return dict(
        cda_mean=slope(x, y_cda),
        fba_mean=slope(x, y_fba),
        diff_mean=float(res.params[0]),
        hac_se=float(res.bse[0]),
        t_stat=float(res.tvalues[0]),
        p_value=float(res.pvalues[0]),
        n_paired=int(n),
        hac_lags=lags,
    )


def binned(x, y_cda, y_fba, n_bins=N_BINS):
    bins = pd.qcut(x, n_bins, duplicates="drop")
    df = pd.DataFrame({"x": x, "cda": y_cda, "fba": y_fba, "bin": bins})
    return df.groupby("bin", observed=True)[["x", "cda", "fba"]].mean().reset_index(drop=True)


def compute(merged):
    m = merged.sort_values("interval_start_ns").reset_index(drop=True)
    steps = np.diff(m["interval_start_ns"].to_numpy())
    assert (steps == NS_PER_SEC).all(), "timeseries grid is not contiguous 1 s buckets"
    x_all = load_flow(len(m))

    out = {"stats": {}, "binned": None}
    for key, h in HORIZONS.items():
        y_c = price_changes(m["vwap_cda"].to_numpy(), h)
        y_f = price_changes(m["vwap_fba"].to_numpy(), h)
        mask = (x_all != 0) & np.isfinite(y_c) & np.isfinite(y_f)
        x, y_c, y_f = x_all[mask], y_c[mask], y_f[mask]
        stats = estimate(x, y_c, y_f)
        assert abs(stats["diff_mean"] - (stats["fba_mean"] - stats["cda_mean"])) < 1e-9 * max(
            1.0, abs(stats["cda_mean"])), "difference regression disagrees with the two slopes"
        out["stats"][key] = stats
        if h == 0:
            out["binned"] = binned(x, y_c, y_f)
    out["flow_abs_total"] = float(np.abs(x_all).sum())
    out["seconds_with_flow"] = int((x_all != 0).sum())
    return out


def _self_test():
    rng = np.random.default_rng(0)
    x = rng.normal(0, 50, 200_000)
    y_c = 2.0 * x + rng.normal(0, 5, len(x))
    y_f = 2.0 * x + rng.normal(0, 5, len(x))
    s = estimate(x, y_c, y_f)
    assert abs(s["cda_mean"] - 2) < 1e-2 and abs(s["fba_mean"] - 2) < 1e-2, s
    assert abs(s["diff_mean"]) < 1e-2 and s["p_value"] > 0.001, s
    y_f2 = 0.5 * x + rng.normal(0, 5, len(x))
    s2 = estimate(x, y_c, y_f2)
    assert abs(s2["diff_mean"] + 1.5) < 1e-2 and s2["p_value"] < 1e-6, s2
    p = np.array([100.0, np.nan, 101.0, np.nan, np.nan, 99.0])
    np.testing.assert_allclose(price_changes(p, 0)[1:], [0, 100, 0, 0, -1e4 * 2 / 101])
    print("self-test passed")


if __name__ == "__main__":
    _self_test()
    import load
    res = compute(load.load_merged())
    for k, v in res["stats"].items():
        print(k, v)
    print(res["binned"])
    print("sum |x| =", res["flow_abs_total"], "seconds with flow =", res["seconds_with_flow"])

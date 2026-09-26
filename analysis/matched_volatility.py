"""Matched-timestamp volatility -- a genuinely paired volatility comparison
between CDA and FBA, built from the two engines' `vwap` columns.

The per-bucket `realized_volatility` column needs >=2 trade-price
observations *inside the same 1-second bucket* to form a return; the FBA
clears at most once per bucket, so it is structurally uncomputable there
(n=1 paired bucket out of 2,678,400). This module instead intersects on
buckets where *both* engines actually traded that second, forms
consecutive-pair log-returns across that shared sparse/irregular grid (the
same matched timestamps for both engines, which is what makes the
comparison fair), and aggregates those returns into a per-day RMS
volatility figure per engine -- comparable via the same Newey-West paired
test (`paired_stats.paired_diff`) used everywhere else in this thesis.

See Thesis/chapters/appendix.tex, "Matched-timestamp volatility" paragraph,
for the formula as stated in the thesis.
"""

import numpy as np
import pandas as pd

import config
import paired_stats


def build_intersection(merged):
    """Rows where both engines actually traded that bucket (vwap present on
    both sides), sorted by time. `merged` is the DataFrame from
    load.load_merged()."""
    mask = merged["vwap_fba"].notna() & merged["vwap_cda"].notna()
    sub = merged.loc[mask, ["interval_start_ns", "vwap_fba", "vwap_cda"]]
    return sub.sort_values("interval_start_ns").reset_index(drop=True)


def compute_matched_returns(intersection):
    """Consecutive-pair log-returns on the matched grid, for both engines
    over the *identical* set of matched timestamps -- that identity is what
    makes the comparison fair despite the irregular gaps between matches.
    Pairs with a non-positive price on either endpoint of either engine are
    skipped (mirrors the Rust per-bucket formula's own defensive skip,
    src/metrics/timeseries.rs:754). Returns a DataFrame indexed by the
    later endpoint's timestamp, columns ret_fba/ret_cda (natural log, not
    yet scaled to bps)."""
    ts = intersection["interval_start_ns"].to_numpy()
    p_fba = intersection["vwap_fba"].to_numpy(dtype=np.float64)
    p_cda = intersection["vwap_cda"].to_numpy(dtype=np.float64)

    p0_fba, p1_fba = p_fba[:-1], p_fba[1:]
    p0_cda, p1_cda = p_cda[:-1], p_cda[1:]
    valid = (p0_fba > 0) & (p1_fba > 0) & (p0_cda > 0) & (p1_cda > 0)

    ret_fba = np.log(p1_fba[valid] / p0_fba[valid])
    ret_cda = np.log(p1_cda[valid] / p0_cda[valid])
    t_end = ts[1:][valid]

    return pd.DataFrame({"interval_start_ns": t_end, "ret_fba": ret_fba,
                          "ret_cda": ret_cda})


def aggregate_window_volatility(returns, window=None, min_count=None):
    """Group returns by the later endpoint's calendar window (day by
    default) and compute the RMS log-return (bps) per engine per window.
    RMS, not raw sum-of-squares: a window's return count varies with how
    active trading was that day, so an unnormalized sum would conflate
    "busier day" with "more volatile day" -- dividing by count gives a
    genuine average-return-magnitude statistic, comparable across windows
    of unequal activity. A window with fewer than `min_count` returns gets
    NaN for both engines symmetrically rather than a near-meaningless
    noisy RMS."""
    window = config.MATCHED_VOL_WINDOW if window is None else window
    min_count = config.MATCHED_VOL_MIN_RETURNS if min_count is None else min_count

    ts = pd.to_datetime(returns["interval_start_ns"], unit="ns")
    grouped = returns.assign(_window=ts.dt.floor(window)).groupby("_window")

    def _rms_bps(s):
        return float(np.sqrt(np.mean(np.square(s)))) * 1e4

    agg = grouped.agg(
        n_returns=("ret_fba", "size"),
        vol_fba_bps=("ret_fba", _rms_bps),
        vol_cda_bps=("ret_cda", _rms_bps),
    )

    below = agg["n_returns"] < min_count
    agg.loc[below, ["vol_fba_bps", "vol_cda_bps"]] = np.nan

    return agg


def intraday_volatility(returns):
    """The un-aggregated companion to aggregate_window_volatility: one
    volatility observation per matched consecutive-pair return, not one per
    day. A single return has no mean to take an RMS around, so it reports
    the absolute log-return directly in bps -- the same fallback the
    per-bucket realized_volatility formula uses when a bucket has exactly
    one return (appendix.tex, "Realized volatility (U)": "a single return
    in the bucket reports |r| directly"). This keeps every one of the
    ~545k matched returns as its own paired observation (CDA and FBA
    computed from the identical matched timestamps, so still positionally
    aligned pair by pair), trading the daily version's noise-averaging for
    full time resolution and a much larger n."""
    return pd.DataFrame({
        "interval_start_ns": returns["interval_start_ns"],
        "vol_fba_bps": returns["ret_fba"].abs() * 1e4,
        "vol_cda_bps": returns["ret_cda"].abs() * 1e4,
    })


def compute(merged, window=None, min_count=None):
    """Orchestrates the full matched-timestamp volatility pipeline. Returns
    a dict with the paired Newey-West stats (daily-aggregated and intraday)
    plus diagnostic counts."""
    window = config.MATCHED_VOL_WINDOW if window is None else window
    min_count = config.MATCHED_VOL_MIN_RETURNS if min_count is None else min_count

    intersection = build_intersection(merged)
    diag = _diagnostic(merged, intersection)

    returns = compute_matched_returns(intersection)
    window_df = aggregate_window_volatility(returns, window=window, min_count=min_count)

    windows_total = len(window_df)
    windows_excluded = int(window_df["vol_fba_bps"].isna().sum())
    print(f"windows ({window}): {windows_total} total, {windows_excluded} excluded "
          f"(< {min_count} matched returns)")

    stats = paired_stats.paired_diff(window_df["vol_fba_bps"], window_df["vol_cda_bps"])
    print(f"matched-timestamp volatility (daily): CDA={stats['cda_mean']:.3f}bps "
          f"FBA={stats['fba_mean']:.3f}bps diff={stats['diff_mean']:.3f}bps "
          f"p={stats['p_value']} n={stats['n_paired']}")

    intraday_df = intraday_volatility(returns)
    intraday_stats = paired_stats.paired_diff(intraday_df["vol_fba_bps"], intraday_df["vol_cda_bps"])
    print(f"matched-timestamp volatility (intraday): CDA={intraday_stats['cda_mean']:.3f}bps "
          f"FBA={intraday_stats['fba_mean']:.3f}bps diff={intraday_stats['diff_mean']:.3f}bps "
          f"p={intraday_stats['p_value']} n={intraday_stats['n_paired']}")

    return dict(
        stats=stats,
        window_df=window_df,
        window=window,
        min_count=min_count,
        returns_n=len(returns),
        windows_total=windows_total,
        windows_excluded=windows_excluded,
        intraday_stats=intraday_stats,
        intraday_df=intraday_df,
        **diag,
    )


def _diagnostic(merged, intersection):
    total = len(merged)
    fba_n = int(merged["vwap_fba"].notna().sum())
    cda_n = int(merged["vwap_cda"].notna().sum())
    inter_n = len(intersection)
    independence_pct = (fba_n / total) * (cda_n / total) * 100
    print(f"total buckets:        {total:,}")
    print(f"FBA nonempty:         {fba_n:,} ({fba_n/total*100:.2f}%)")
    print(f"CDA nonempty:         {cda_n:,} ({cda_n/total*100:.2f}%)")
    print(f"intersection (both):  {inter_n:,} ({inter_n/total*100:.2f}%)")
    print(f"independence baseline: {independence_pct:.2f}% "
          f"(expected higher than this if co-occurrence is flow-driven)")
    return dict(total=total, fba_nonempty_n=fba_n, cda_nonempty_n=cda_n,
                intersection_n=inter_n, intersection_pct=inter_n / total * 100,
                independence_baseline_pct=independence_pct)


if __name__ == "__main__":
    import load
    m = load.load_merged()
    result = compute(m)
    print(result["window_df"].head(10))

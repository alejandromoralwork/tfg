"""Builds the figures for ch5_results.tex -> Thesis/figures/*.png.

Eight figures, picked to each tell one specific story rather than
dumping all ~30 metrics:

  fig_latencycontrol.png -- engine-control finding: clearing/match latency
  fig_kylelambda.png      -- headline finding: Kyle's lambda + the FBA
                             intra-interval-dispersion uniform-pricing check
  fig_matchedvol.png       -- matched-timestamp volatility by calendar day
                             (RQ2.2), plus matched-return coverage per day
  fig_spreaddecomp.png     -- effective spread decomposed into realized
                             spread + price impact, both engines
  fig_pricepaths.png       -- CDA vs FBA price path (VWAP proxy) over the
                             single highest-volatility day in the sample
  fig_pricediff.png        -- FBA-minus-CDA price difference: the same day,
                             plus its distribution over the full month
  fig_depthcompare.png     -- depth at best / within 10/50/100bps, bar
                             comparison
  fig_dailytrend.png       -- daily mean effective spread, both engines,
                             across the month (stability/coherence check)
  fig_residualshare.png    -- FBA unexecuted_residual_share distribution
                             + a short rolling-mean time series
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

import config
import load
import matched_volatility
import signed_fba_spread

CDA_COLOR = "#1b9e77"
FBA_COLOR = "#d95f02"
DIFF_COLOR = "#7570b3"
plt.rcParams.update({
    "figure.dpi": 150,
    "font.size": 10,
    "axes.spines.top": False,
    "axes.spines.right": False,
})


def _save(fig, name):
    path = config.FIGURES_DIR / f"{name}.png"
    fig.savefig(path, bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {path}")


def select_busiest_day(m, ts, day):
    grouped = m.assign(_day=day).groupby("_day")["realized_volatility_cda"]
    vol_by_day = grouped.mean()
    counts_by_day = grouped.size()
    # the first/last calendar day in the sample can be a sliver (the run's
    # anchor falls at 2025-11-30 23:59:59 UTC, one second before December
    # starts) -- a single-row group's mean is not a "busiest day", it's an
    # outlier artifact. Require a near-complete day (>=80% of 86,400 buckets).
    complete_days = counts_by_day[counts_by_day >= 0.8 * 86400].index
    return vol_by_day.loc[complete_days].idxmax()


def fig_latencycontrol(m):
    fig, ax = plt.subplots(figsize=(6, 3.6))
    for col, color, label in [
        ("avg_clearing_latency_micros_cda", CDA_COLOR, "CDA (per match)"),
        ("avg_clearing_latency_micros_fba", FBA_COLOR, "FBA (per batch clear)"),
    ]:
        vals = m[col].dropna()
        vals = vals[vals > 0]
        ax.hist(np.log10(vals), bins=80, alpha=0.6, color=color, label=label, density=True)
    ax.set_xlabel(r"$\log_{10}$(measured compute time, microseconds)")
    ax.set_ylabel("density")
    ax.set_title("Engine control: per-operation compute time")
    ax.legend(frameon=False)
    _save(fig, "fig_latencycontrol")


def fig_kylelambda(m):
    fig, axes = plt.subplots(1, 2, figsize=(9.5, 3.6))

    ax = axes[0]
    lo, hi = m[["kyle_lambda_cda", "kyle_lambda_fba"]].stack().quantile([0.01, 0.99])
    for col, color, label in [("kyle_lambda_cda", CDA_COLOR, "CDA"),
                                ("kyle_lambda_fba", FBA_COLOR, "FBA")]:
        vals = m[col].dropna()
        vals = vals[(vals >= lo) & (vals <= hi)]
        ax.hist(vals, bins=100, alpha=0.6, color=color, label=label, density=True)
    ax.axvline(0, color="black", linewidth=0.8, linestyle="--")
    ax.set_xlabel("Kyle's lambda (bps per SOL)")
    ax.set_ylabel("density")
    ax.set_title("Price impact of order flow, per interval")
    ax.legend(frameon=False)

    ax = axes[1]
    cda_disp = m["intra_interval_price_dispersion_cda"].dropna()
    cda_disp = cda_disp[cda_disp < cda_disp.quantile(0.99)]
    fba_disp = m["intra_interval_price_dispersion_fba"].dropna()
    fba_zero_pct = 100 * (fba_disp == 0).mean()
    ax.hist(cda_disp, bins=80, color=CDA_COLOR, alpha=0.7, label="CDA", log=True)
    ax.hist(fba_disp[fba_disp > 0], bins=80, color=FBA_COLOR, alpha=0.7,
             label=f"FBA, nonzero only ({100-fba_zero_pct:.1f}% of intervals)", log=True)
    ax.axvline(0, color=FBA_COLOR, linewidth=1.2, linestyle="--",
               label=f"FBA exactly 0 ({fba_zero_pct:.1f}% of intervals, off this log scale)")
    ax.set_xlabel("Intra-interval price dispersion (bps)")
    ax.set_ylabel("count (log scale)")
    ax.set_title("Uniform-pricing check")
    ax.legend(frameon=False, fontsize=7)

    fig.tight_layout()
    _save(fig, "fig_kylelambda")


def fig_spreaddecomp(m, signed_df):
    labels = ["Effective", "Realized 1s", "Realized 5s", "Realized 30s",
              "Impact 1s", "Impact 5s", "Impact 30s"]
    cols = ["effective_spread_bps", "realized_spread_bps_1s", "realized_spread_bps_5s",
            "realized_spread_bps_30s", "price_impact_bps_1s", "price_impact_bps_5s",
            "price_impact_bps_30s"]
    cda_vals = [m[f"{c}_cda"].mean() for c in cols]
    fba_vals = [signed_df[f"{c}_fba"].mean() for c in cols]

    x = np.arange(len(labels))
    width = 0.35
    fig, ax = plt.subplots(figsize=(8, 4))
    ax.bar(x - width/2, cda_vals, width, color=CDA_COLOR, label="CDA (signed)")
    ax.bar(x + width/2, fba_vals, width, color=FBA_COLOR, label="FBA (CDA-tick-signed, see caption)")
    ax.axhline(0, color="black", linewidth=0.8)
    ax.set_xticks(x)
    ax.set_xticklabels(labels, rotation=20, ha="right")
    ax.set_ylabel("bps (quantity-weighted mean)")
    ax.set_title("Effective spread decomposed into realized spread and price impact")
    ax.legend(frameon=False)
    fig.tight_layout()
    _save(fig, "fig_spreaddecomp")


def fig_pricepaths(m, ts, day, busiest_day):
    day_mask = day == busiest_day
    day_df = m.loc[day_mask].copy()
    day_df["ts"] = ts.loc[day_mask]

    fig, ax = plt.subplots(figsize=(9, 3.8))
    ax.plot(day_df["ts"], day_df["vwap_fba"], color=FBA_COLOR, label="FBA VWAP",
             linewidth=0.8, alpha=0.6, zorder=1)
    ax.plot(day_df["ts"], day_df["vwap_cda"], color=CDA_COLOR, label="CDA VWAP",
             linewidth=0.9, zorder=2)
    ax.set_ylabel("SOL/USD (trade-volume-weighted price per interval)")
    ax.set_title(f"Price path, both mechanisms -- {busiest_day} "
                 "(highest mean CDA realized volatility day in sample)")
    ax.legend(frameon=False)
    fig.autofmt_xdate()
    fig.tight_layout()
    _save(fig, "fig_pricepaths")


def fig_pricediff(m, ts, day, busiest_day):
    """FBA-minus-CDA price difference, explicitly requested: how far apart
    do the two mechanisms' prices sit, in bps, both on the busiest day and
    across the whole month."""
    both = m["vwap_fba"].notna() & m["vwap_cda"].notna() & (m["vwap_cda"] > 0)
    diff_bps = pd.Series(np.nan, index=m.index)
    diff_bps.loc[both] = (m.loc[both, "vwap_fba"] - m.loc[both, "vwap_cda"]) \
        / m.loc[both, "vwap_cda"] * 1e4

    fig, axes = plt.subplots(1, 2, figsize=(10, 3.8), gridspec_kw={"width_ratios": [2, 1]})

    ax = axes[0]
    day_mask = (day == busiest_day) & both
    ax.plot(ts.loc[day_mask], diff_bps.loc[day_mask], color=DIFF_COLOR, linewidth=0.7)
    ax.axhline(0, color="black", linewidth=0.8)
    ax.set_ylabel("FBA $-$ CDA VWAP (bps)")
    ax.set_title(f"Price difference, {busiest_day}")
    ax.tick_params(axis="x", rotation=30)

    ax = axes[1]
    vals = diff_bps.dropna()
    lo, hi = vals.quantile([0.005, 0.995])
    vals_clipped = vals[(vals >= lo) & (vals <= hi)]
    ax.hist(vals_clipped, bins=80, color=DIFF_COLOR, alpha=0.85)
    ax.axvline(0, color="black", linewidth=0.8)
    ax.axvline(vals.median(), color=DIFF_COLOR, linewidth=1.2, linestyle="--",
               label=f"median {vals.median():.2f} bps")
    ax.set_xlabel("FBA $-$ CDA VWAP (bps)")
    ax.set_ylabel("count")
    ax.set_title("Distribution, full month\n(only intervals both traded)")
    ax.legend(frameon=False, fontsize=8)

    fig.tight_layout()
    _save(fig, "fig_pricediff")
    return dict(median_diff_bps=float(vals.median()), mean_diff_bps=float(vals.mean()),
                std_diff_bps=float(vals.std()), n=int(len(vals)))


def fig_depthcompare(m):
    labels = ["At best", "Within 10bps", "Within 50bps", "Within 100bps"]
    cols = ["depth_at_best", "depth_within_10bps", "depth_within_50bps", "depth_within_100bps"]
    cda_vals = [m[f"{c}_cda"].mean() for c in cols]
    fba_vals = [m[f"{c}_fba"].mean() for c in cols]

    # log scale: "at best" (touch only) is 2-3 orders of magnitude smaller
    # than the cumulative "within x bps" figures -- on a linear axis it's
    # invisible next to them.
    x = np.arange(len(labels))
    width = 0.35
    fig, ax = plt.subplots(figsize=(6.5, 4))
    ax.bar(x - width/2, cda_vals, width, color=CDA_COLOR, label="CDA")
    ax.bar(x + width/2, fba_vals, width, color=FBA_COLOR, label="FBA")
    ax.set_yscale("log")
    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("mean depth (SOL, log scale)")
    ax.set_title("Depth at best and within threshold, both mechanisms")
    ax.legend(frameon=False)
    fig.tight_layout()
    _save(fig, "fig_depthcompare")


def fig_dailytrend(signed_df, complete_days):
    """Daily mean effective spread across the whole month -- a coherence
    check as much as a result: shows whether the headline RQ2.1 gap holds
    throughout the month or is driven by a handful of days. Built from the
    matched, CDA-tick-signed FBA series (analysis/signed_fba_spread.py) so
    both sides are aggregated over the identical set of buckets each day.
    `complete_days` are the calendar days with a near-complete underlying
    record grid (>=80% of 86,400 one-second buckets), computed from the
    full replay so a partial anchor day cannot skew a mean."""
    sday = pd.to_datetime(signed_df["interval_start_ns"], unit="ns").dt.date
    d = signed_df.assign(_day=sday)
    daily_cda = d.groupby("_day")["effective_spread_bps_cda"].mean()
    daily_fba = d.groupby("_day")["effective_spread_bps_fba"].mean()
    complete = daily_cda.index.intersection(complete_days)
    daily_cda, daily_fba = daily_cda.loc[complete], daily_fba.loc[complete]

    fig, ax = plt.subplots(figsize=(9, 3.6))
    ax.plot(daily_cda.index, daily_cda.to_numpy(), color=CDA_COLOR, marker="o",
             markersize=3, linewidth=1, label="CDA")
    ax.plot(daily_fba.index, daily_fba.to_numpy(), color=FBA_COLOR, marker="o",
             markersize=3, linewidth=1, label="FBA")
    ax.set_ylabel("Daily mean effective spread (bps)")
    ax.set_title("Effective spread by calendar day, full month")
    ax.legend(frameon=False)
    fig.autofmt_xdate()
    fig.tight_layout()
    _save(fig, "fig_dailytrend")


def fig_residualshare(m):
    fig, axes = plt.subplots(1, 2, figsize=(9.5, 3.6))

    ax = axes[0]
    vals = m["unexecuted_residual_share_fba"].dropna()
    ax.hist(vals, bins=60, color=FBA_COLOR, alpha=0.8)
    ax.set_xlabel(r"$\Sigma|D(p^*){-}S(p^*)| \,/\, \Sigma\max(D(p^*),S(p^*))$ per interval")
    ax.set_ylabel("count")
    ax.set_title("FBA unexecuted residual share, distribution")

    ax = axes[1]
    ts = pd.to_datetime(m["interval_start_ns"], unit="ns")
    s = pd.Series(m["unexecuted_residual_share_fba"].to_numpy(), index=ts)
    rolling = s.rolling("6H", min_periods=1).mean()
    ax.plot(rolling.index, rolling.to_numpy(), color=FBA_COLOR, linewidth=0.8)
    ax.set_ylabel("6h rolling mean")
    ax.set_title("...over the full month (persistent, not episode-driven)")
    fig.autofmt_xdate()

    fig.tight_layout()
    _save(fig, "fig_residualshare")


def fig_matchedvol(window_df, min_count, intraday_df):
    """Matched-timestamp volatility, both granularities together: daily RMS
    by calendar day (top), the intraday per-return distribution the daily
    RMS is summarising over (middle), and, since the daily figure's
    credibility depends on it, the per-day matched-return count against
    the exclusion threshold (bottom)."""
    fig, (ax1, ax2, ax3) = plt.subplots(
        3, 1, figsize=(9, 7.6),
        gridspec_kw={"height_ratios": [2, 1.6, 1]},
    )

    ax1.plot(window_df.index, window_df["vol_cda_bps"], color=CDA_COLOR,
             marker="o", markersize=3, linewidth=1, label="CDA")
    ax1.plot(window_df.index, window_df["vol_fba_bps"], color=FBA_COLOR,
             marker="o", markersize=3, linewidth=1, label="FBA")
    ax1.set_ylabel("Daily RMS\nlog-return (bps)")
    ax1.set_title("Matched-timestamp volatility by calendar day, both mechanisms")
    ax1.legend(frameon=False)
    ax1.tick_params(axis="x", rotation=30)

    intraday_ts = pd.to_datetime(intraday_df["interval_start_ns"], unit="ns")
    intraday_indexed = intraday_df.set_index(intraday_ts).sort_index()
    rolling_cda = intraday_indexed["vol_cda_bps"].rolling("6H", min_periods=1).mean()
    rolling_fba = intraday_indexed["vol_fba_bps"].rolling("6H", min_periods=1).mean()
    ax2.plot(rolling_cda.index, rolling_cda.to_numpy(), color=CDA_COLOR,
             linewidth=0.8, label="CDA")
    ax2.plot(rolling_fba.index, rolling_fba.to_numpy(), color=FBA_COLOR,
             linewidth=0.8, label="FBA")
    ax2.set_ylabel("6h rolling mean\n$|r|$ (bps)")
    ax2.set_title("Intraday variant: every matched return, 6-hour rolling mean over the month")
    ax2.legend(frameon=False)
    ax2.tick_params(axis="x", rotation=30)

    ax3.bar(window_df.index, window_df["n_returns"], color=DIFF_COLOR, alpha=0.7)
    ax3.axhline(min_count, color="black", linewidth=0.8, linestyle="--",
                label=f"min. {min_count} returns/day")
    ax3.set_ylabel("matched returns\nper day")
    ax3.legend(frameon=False, fontsize=8)
    ax3.tick_params(axis="x", rotation=30)

    fig.tight_layout()
    _save(fig, "fig_matchedvol")


def main():
    print("loading merged timeseries for figures...")
    m = load.load_merged()
    ts = pd.to_datetime(m["interval_start_ns"], unit="ns")
    day = ts.dt.date
    busiest_day = select_busiest_day(m, ts, day)
    counts = m.assign(_day=day).groupby("_day").size()
    complete_days = counts[counts >= 0.8 * 86400].index

    signed = signed_fba_spread.compute(m)

    fig_latencycontrol(m)
    fig_kylelambda(m)
    mv = matched_volatility.compute(m)
    fig_matchedvol(mv["window_df"], mv["min_count"], mv["intraday_df"])
    fig_spreaddecomp(m, signed["signed_df"])
    fig_pricepaths(m, ts, day, busiest_day)
    diffinfo = fig_pricediff(m, ts, day, busiest_day)
    fig_depthcompare(m)
    fig_dailytrend(signed["signed_df"], complete_days)
    fig_residualshare(m)

    return {"pricepaths_day": str(busiest_day), **diffinfo}


if __name__ == "__main__":
    info = main()
    print(info)

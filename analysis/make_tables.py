"""Computes every table cell ch5 needs and writes:

  - Thesis/data/generated_numbers.tex   one \\newcommand per populated cell
  - analysis/output/summary.json         full machine-readable dump (all
                                          metrics, not just the curated ones
                                          that make it into a table)

LaTeX \\newcommand names must be letters only (no digits, no underscores),
so every metric key below maps to a hand-picked PascalCase, digit-free
macro stem (see METRIC_STEM).
"""

import json

import numpy as np
import pandas as pd

import config
import load
import matched_volatility
import paired_stats
import signed_fba_spread

# metric column (base name, without _fba/_cda suffix) -> macro-name stem.
# Digit-free by construction (LaTeX \newcommand names are letters only).
METRIC_STEM = {
    "quoted_spread_bps": "QuotedSpread",
    "depth_at_best": "DepthAtBest",
    "depth_within_10bps": "DepthWithinTenBps",
    "depth_within_50bps": "DepthWithinFiftyBps",
    "depth_within_100bps": "DepthWithinHundredBps",
    "effective_spread_bps": "EffectiveSpread",
    "realized_spread_bps_1s": "RealizedSpreadOneS",
    "realized_spread_bps_5s": "RealizedSpreadFiveS",
    "realized_spread_bps_30s": "RealizedSpreadThirtyS",
    "price_impact_bps_1s": "PriceImpactOneS",
    "price_impact_bps_5s": "PriceImpactFiveS",
    "price_impact_bps_30s": "PriceImpactThirtyS",
    "amihud_illiquidity": "Amihud",
    "kyle_lambda": "KyleLambda",
    "realized_volatility": "RealizedVol",
    "matched_volatility": "MatchedVol",
    "matched_volatility_intraday": "MatchedVolIntraday",
    "intra_interval_price_dispersion": "Dispersion",
    "executed_volume": "ExecVolume",
    "executed_notional": "ExecNotional",
    "vwap": "Vwap",
    "trade_count": "TradeCount",
    "fill_rate": "FillRate",
    "avg_time_to_execution_secs": "TimeToExec",
    "trader_surplus": "TraderSurplus",
    "order_size_inflation": "SizeInflation",
    "order_to_trade_ratio": "OrderToTradeRatio",
    "throughput_orders_per_sec": "Throughput",
    "avg_clearing_latency_micros": "ClearingLatency",
    # one-sided
    "book_imbalance": "BookImbalance",
    "total_book_depth": "TotalBookDepth",
    "boundary_concentration": "BoundaryConcentration",
    "unexecuted_residual_share": "ResidualShare",
}

# metrics formatted with more decimal precision because their natural scale
# is small (ratios/shares near 0-1, or bps quantities where 2dp loses signal)
_FOUR_DP = {"fill_rate", "order_to_trade_ratio", "book_imbalance",
            "unexecuted_residual_share", "boundary_concentration",
            "order_size_inflation"}
# trade_count's raw per-row value is an integer count, but its *mean*
# across buckets (what's reported here) is a fraction (e.g. ~1.3
# trades/bucket) -- falls through to the default 2dp formatting below,
# not integer rounding, or a real small difference would round away to 0.


def _fmt(x, key=None):
    if x is None or (isinstance(x, float) and (np.isnan(x) or np.isinf(x))):
        return "n/a"
    s = f"{x:.4f}" if key in _FOUR_DP else f"{x:,.2f}"
    if set(s.replace(",", "")) <= set("-0."):  # e.g. "-0.00" -> "0.00"
        s = s.lstrip("-")
    return s


def _fmt_p(p):
    if p is None or (isinstance(p, float) and np.isnan(p)):
        return "n/a"
    if p < 0.001:
        return "$<$0.001"
    return f"{p:.3f}"


def macros_for_paired(macros, dump, metric_key, stats):
    stem = METRIC_STEM[metric_key]
    macros[f"Res{stem}Cda"] = _fmt(stats["cda_mean"], metric_key)
    macros[f"Res{stem}Fba"] = _fmt(stats["fba_mean"], metric_key)
    macros[f"Res{stem}Diff"] = _fmt(stats["diff_mean"], metric_key)
    macros[f"Res{stem}P"] = _fmt_p(stats["p_value"])
    macros[f"Res{stem}HacSe"] = _fmt(stats["hac_se"], metric_key)
    macros[f"Res{stem}NPaired"] = f"{stats['n_paired']:,}"
    dump[metric_key] = stats


def macros_for_onesided(macros, dump, metric_key, engine_label, stats):
    stem = METRIC_STEM[metric_key]
    macros[f"Desc{stem}{engine_label}Mean"] = _fmt(stats["mean"], metric_key)
    macros[f"Desc{stem}{engine_label}Std"] = _fmt(stats["std"], metric_key)
    macros[f"Desc{stem}{engine_label}Median"] = _fmt(stats["median"], metric_key)
    macros[f"Desc{stem}{engine_label}N"] = f"{stats['n']:,}"
    dump[f"{metric_key}_{engine_label.lower()}"] = stats


def parse_checkpoint(text):
    kv = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("version") is False and " " not in line:
            continue
        parts = line.split(" ", 1)
        if len(parts) == 2:
            kv[parts[0]] = parts[1]
    return kv


def main(extra_macros=None):
    print("loading merged timeseries...")
    merged = load.load_merged()

    macros = {}
    dump = {"paired": {}, "one_sided": {}, "sample": {}, "validation": {}}

    print("computing paired-difference stats for", len(config.PAIRED_METRICS), "metrics...")
    for metric in config.PAIRED_METRICS:
        stats = paired_stats.paired_diff(merged[f"{metric}_fba"], merged[f"{metric}_cda"])
        macros_for_paired(macros, dump["paired"], metric, stats)

    # replace the unsigned FBA effective/realized spread and price impact
    # (computed above via the generic loop, using the mean-absolute-deviation
    # branch the FBA's missing taker/maker flag forces) with a CDA-tick-signed
    # version -- see analysis/signed_fba_spread.py for why the CDA's own tick,
    # not the FBA's, is what avoids a circular construction. CDA-side values
    # are unchanged throughout.
    print("computing signed FBA spread (CDA tick rule)...")
    signed = signed_fba_spread.compute(merged)
    for metric, stats in signed["stats"].items():
        macros_for_paired(macros, dump["paired"], metric, stats)
    macros["SignedFbaSpreadNMatched"] = f"{signed['n_matched']:,}"
    macros["SignedFbaSpreadNSigned"] = f"{signed['n_signed']:,}"

    print("computing one-sided stats for CDA-only / FBA-only metrics...")
    for metric in config.CDA_ONLY_METRICS:
        stats = paired_stats.one_sided_stats(merged[f"{metric}_cda"])
        macros_for_onesided(macros, dump["one_sided"], metric, "Cda", stats)
    for metric in config.FBA_ONLY_METRICS:
        stats = paired_stats.one_sided_stats(merged[f"{metric}_fba"])
        macros_for_onesided(macros, dump["one_sided"], metric, "Fba", stats)

    # what drives the price-difference spikes in fig_pricediff -- checked
    # empirically rather than asserted: correlate |FBA-CDA VWAP diff| against
    # realized volatility (a fast-move proxy) and against min(trade_count)
    # on both sides (a thin-sample proxy), pooled over the whole month.
    diff_bps = pd.Series(np.nan, index=merged.index)
    diffmask = merged["vwap_fba"].notna() & merged["vwap_cda"].notna() & (merged["vwap_cda"] > 0)
    diff_bps.loc[diffmask] = (merged.loc[diffmask, "vwap_fba"] - merged.loc[diffmask, "vwap_cda"]) \
        / merged.loc[diffmask, "vwap_cda"] * 1e4
    abs_diff = diff_bps.abs()
    vol_corr = abs_diff.corr(merged["realized_volatility_cda"])
    macros["PriceDiffVolCorr"] = f"{vol_corr:.2f}"
    dump["price_diff_vs_volatility_corr"] = float(vol_corr)

    # FBA intra-interval dispersion is not *exactly* zero in every interval
    # -- METRICS.md itself hedges with "~0", not "0" -- record the real share.
    disp_mask = merged["intra_interval_price_dispersion_fba"].notna() & \
                merged["intra_interval_price_dispersion_cda"].notna()
    disp_fba = merged.loc[disp_mask, "intra_interval_price_dispersion_fba"]
    exact_zero_pct = float((disp_fba == 0).mean()) * 100
    macros["DispersionFbaExactZeroPct"] = f"{exact_zero_pct:.2f}"
    macros["DispersionFbaNonzeroPct"] = f"{100-exact_zero_pct:.2f}"
    dump["dispersion_fba_exact_zero_pct"] = exact_zero_pct

    # share of empty FBA batches (trade_count == 0) -- real, computable
    # without a sweep; replaces the sweep-only "empty batch" diagnostic.
    empty_share = float((merged["trade_count_fba"] == 0).mean())
    macros["FbaEmptyBatchShare"] = f"{empty_share*100:.2f}"
    dump["fba_empty_batch_share"] = empty_share

    # matched-timestamp volatility -- the paired RQ2.2 volatility comparison
    # that realized_volatility can't supply for the FBA (see
    # analysis/matched_volatility.py and appendix.tex's formula entry).
    print("computing matched-timestamp volatility...")
    mv = matched_volatility.compute(merged)
    macros_for_paired(macros, dump["paired"], "matched_volatility", mv["stats"])
    macros["ResMatchedVolIntersectionN"] = f"{mv['intersection_n']:,}"
    macros["ResMatchedVolIntersectionPct"] = f"{mv['intersection_pct']:.2f}"
    macros["ResMatchedVolIndependenceBaselinePct"] = f"{mv['independence_baseline_pct']:.2f}"
    macros["ResMatchedVolReturnsN"] = f"{mv['returns_n']:,}"
    macros["ResMatchedVolWindowsExcluded"] = f"{mv['windows_excluded']:,}"
    dump["matched_volatility"] = {k: v for k, v in mv.items()
                                   if k not in ("stats", "window_df",
                                                "intraday_stats", "intraday_df")}

    # same matched-timestamp construction, but un-aggregated: one paired
    # observation per matched consecutive-pair return (n in the hundreds
    # of thousands) instead of one per calendar day (n=31). Keeps both
    # granularities in the thesis rather than replacing one with the other.
    macros_for_paired(macros, dump["paired"], "matched_volatility_intraday",
                       mv["intraday_stats"])

    # p99 of per-bucket average clearing/match latency -- a genuinely
    # computable distributional fact (99th pct across the 2.68M per-bucket
    # means), distinct from (and not a substitute for) a 99th percentile
    # over individual order latencies, which the aggregated CSV can't
    # support -- stated as such wherever this number is used.
    for eng, eng_stem in (("cda", "Cda"), ("fba", "Fba")):
        vals = merged[f"avg_clearing_latency_micros_{eng}"].dropna()
        p99 = float(np.percentile(vals, 99)) if len(vals) else np.nan
        macros[f"Latency{eng_stem}PNinetyNine"] = _fmt(p99)
        dump[f"latency_{eng}_p99_micros"] = p99

    # --- sample table (tab:sample) ---
    checkpoint = parse_checkpoint(config.CHECKPOINT_TXT.read_text())
    scan_stats = json.loads((config.OUTPUT_DIR / "scan_stats.json").read_text())
    dataset_stats_path = config.OUTPUT_DIR / "dataset_stats.json"

    records_seen = int(checkpoint["records_seen"])
    records_skipped = int(checkpoint["records_skipped"])
    macros["SampleFilesTotal"] = checkpoint["files_total"]
    macros["SampleTotalMessages"] = f"{records_seen:,}"
    macros["SampleNewOrders"] = f"{scan_stats['new_live_orders']:,}"
    macros["SampleCancellations"] = f"{scan_stats['cancellations']:,}"
    macros["SampleOtherEvents"] = f"{scan_stats['other_events']:,}"
    macros["SampleSkipped"] = f"{records_skipped:,}"
    macros["SampleSkippedPct"] = f"{100*records_skipped/records_seen:.2f}"
    macros["SampleFbaTrades"] = f"{int(merged['trade_count_fba'].sum()):,}"
    macros["SampleCdaTrades"] = f"{int(merged['trade_count_cda'].sum()):,}"
    macros["SampleFbaNotional"] = f"{merged['executed_notional_fba'].sum():,.2f}"
    macros["SampleCdaNotional"] = f"{merged['executed_notional_cda'].sum():,.2f}"
    dump["sample"]["checkpoint"] = checkpoint
    dump["sample"]["scan_stats"] = scan_stats

    if dataset_stats_path.exists():
        ds = json.loads(dataset_stats_path.read_text())
        macros["SampleDistinctParticipants"] = f"{ds['distinct_participants']:,}"
        macros["SampleMedianInterArrival"] = f"{ds['median_inter_arrival_ms']:.3f}"
        macros["SampleMeanInterArrival"] = f"{ds['mean_inter_arrival_ms']:.3f}"
        macros["SampleZeroGapPct"] = f"{ds['zero_gap_fraction']*100:.1f}"
        p99_ns = ds["inter_arrival_percentiles_ns"].get("99")
        macros["SamplePNinetyNineInterArrival"] = f"{p99_ns/1e6:.1f}" if p99_ns is not None else "n/a"
        dump["sample"]["dataset_stats"] = ds
    else:
        for name in ("SampleDistinctParticipants", "SampleMedianInterArrival",
                      "SampleMeanInterArrival", "SampleZeroGapPct", "SamplePNinetyNineInterArrival"):
            macros[name] = "n/a"
        print("WARNING: dataset_stats.json not found -- run dataset_stats.py first; "
              "participant count / inter-arrival stats left as n/a")

    # --- validation table (tab:validation) ---
    # cargo test / test engine all results confirmed by direct run (see
    # implementation log / TESTING.md); hardcoded here since they come from
    # a separate toolchain invocation, not this Python pipeline.
    macros["ValCargoTestsPassed"] = "83"
    macros["ValCargoTestsTotal"] = "83"
    macros["ValChecklistPassed"] = "37"
    macros["ValChecklistTotal"] = "37"
    macros["ValInvariantViolations"] = "0"
    macros["ValRunComplete"] = "true" if checkpoint.get("complete") == "true" else "false"
    dump["validation"] = dict(cargo_tests="80/80", runtime_checklist="37/37",
                               run_complete=checkpoint.get("complete"))

    if extra_macros:
        macros.update(extra_macros)
        dump["extra"] = extra_macros

    # --- write outputs ---
    tex_path = config.DATA_DIR / "generated_numbers.tex"
    lines = [
        "% AUTO-GENERATED by analysis/make_tables.py -- do not hand-edit.",
        "% Re-run `python analysis/run_all.py` to regenerate after the",
        "% underlying results/sol/*.csv change.",
        "",
    ]
    for name in sorted(macros):
        lines.append(f"\\newcommand{{\\{name}}}{{{macros[name]}}}")
    tex_path.write_text("\n".join(lines) + "\n")
    print(f"wrote {tex_path} ({len(macros)} macros)")

    json_path = config.OUTPUT_DIR / "summary.json"
    json_path.write_text(json.dumps(dump, indent=2, default=str))
    print(f"wrote {json_path}")

    return merged  # handed to make_figures if run from run_all in-process


if __name__ == "__main__":
    main()

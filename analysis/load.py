"""Load and merge the FBA/CDA timeseries CSVs.

2,678,400 rows x 34 columns each, ~600MB per file. Loaded whole (no
chunking needed -- fits comfortably in memory with the dtypes below), with
explicit dtypes to keep peak memory down and to avoid float32 precision
loss on the few columns that hold large summed fixed-point values.
"""

import pandas as pd

import config

# float64 for columns that sum large fixed-point integers (executed_notional
# can reach ~1e16 raw units; float32's ~7 significant digits would lose
# precision there). float32 for everything else (bps/ratio/count-like
# columns, where 7 significant digits is ample).
_FLOAT64_COLS = {"executed_notional", "vwap", "trader_surplus"}

_ALL_METRIC_COLS = [
    "quoted_spread_bps", "depth_at_best", "depth_within_10bps",
    "depth_within_50bps", "depth_within_100bps", "book_imbalance",
    "total_book_depth", "effective_spread_bps", "realized_spread_bps_1s",
    "realized_spread_bps_5s", "realized_spread_bps_30s",
    "price_impact_bps_1s", "price_impact_bps_5s", "price_impact_bps_30s",
    "amihud_illiquidity", "kyle_lambda", "realized_volatility",
    "intra_interval_price_dispersion", "pricing_error_bps",
    "executed_volume", "executed_notional", "vwap", "fill_rate",
    "avg_time_to_execution_secs", "trader_surplus", "order_size_inflation",
    "order_to_trade_ratio", "boundary_concentration",
    "throughput_orders_per_sec", "avg_clearing_latency_micros",
    "unexecuted_residual_share",
]

_DTYPES = {"engine": "category", "interval_start_ns": "int64",
           "interval_width_ns": "int64", "trade_count": "int64"}
for _c in _ALL_METRIC_COLS:
    _DTYPES[_c] = "float64" if _c in _FLOAT64_COLS else "float32"


def _read_one(path):
    df = pd.read_csv(path, dtype=_DTYPES)
    for col in config.PRICE_SCALED_COLUMNS:
        df[col] = df[col] / config.PRICE_SCALE
    return df


def load_engine_frames():
    """Returns (fba_df, cda_df), each with price-scaled columns already
    converted to USD."""
    fba = _read_one(config.FBA_CSV)
    cda = _read_one(config.CDA_CSV)
    return fba, cda


def load_merged():
    """Returns one DataFrame, one row per interval_start_ns, columns
    suffixed _fba / _cda. Safe because both recorders share one anchor/grid
    (METRICS.md sec 1) and both CSVs have the same 2,678,400 rows."""
    fba, cda = load_engine_frames()
    assert len(fba) == len(cda), (
        f"row count mismatch: fba={len(fba)} cda={len(cda)} -- "
        "the two recorders should share one grid; investigate before trusting a merge"
    )
    merged = pd.merge(
        fba.drop(columns=["engine"]),
        cda.drop(columns=["engine"]),
        on=["interval_start_ns", "interval_width_ns"],
        suffixes=("_fba", "_cda"),
        how="inner",
        validate="one_to_one",
    )
    assert len(merged) == len(fba), "merge dropped or duplicated rows -- grid mismatch"
    return merged


if __name__ == "__main__":
    m = load_merged()
    print(f"merged shape: {m.shape}")
    print(m[["interval_start_ns", "quoted_spread_bps_fba", "quoted_spread_bps_cda"]].head())

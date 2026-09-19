"""Paths and constants shared by every analysis script.

All paths are resolved relative to this file so the pipeline runs the same
whether invoked as `python analysis/run_all.py` from the repo root or as
`python run_all.py` from inside analysis/.
"""

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

RESULTS_DIR = REPO_ROOT / "results" / "sol"
FBA_CSV = RESULTS_DIR / "fba_timeseries.csv"
CDA_CSV = RESULTS_DIR / "cda_timeseries.csv"
CHECKPOINT_TXT = RESULTS_DIR / "checkpoint.txt"
SUMMARY_TXT = RESULTS_DIR / "summary.txt"

RAW_DATA_DIR = REPO_ROOT / "src" / "data" / "order_statuses" / "sol"

ANALYSIS_DIR = REPO_ROOT / "analysis"
OUTPUT_DIR = ANALYSIS_DIR / "output"

THESIS_DIR = REPO_ROOT / "Thesis"
FIGURES_DIR = THESIS_DIR / "figures"
DATA_DIR = THESIS_DIR / "data"

# Fixed-point scale used throughout the Rust crate (src/types.rs: PRICE_SCALE).
# Divide raw price/notional-denominated columns by this to get USD.
# kyle_lambda is explicitly NOT scaled this way (it's bps per SOL) -- see METRICS.md.
PRICE_SCALE = 1_000_000

# Columns that are fixed-point price/notional and need /PRICE_SCALE.
PRICE_SCALED_COLUMNS = ["executed_notional", "vwap", "trader_surplus"]

# Metrics reported on both engines -- eligible for paired-difference treatment.
PAIRED_METRICS = [
    "quoted_spread_bps",
    "depth_at_best",
    "depth_within_10bps",
    "depth_within_50bps",
    "depth_within_100bps",
    "effective_spread_bps",
    "realized_spread_bps_1s",
    "realized_spread_bps_5s",
    "realized_spread_bps_30s",
    "price_impact_bps_1s",
    "price_impact_bps_5s",
    "price_impact_bps_30s",
    "amihud_illiquidity",
    "kyle_lambda",
    "realized_volatility",
    "intra_interval_price_dispersion",
    "executed_volume",
    "executed_notional",
    "vwap",
    "trade_count",
    "fill_rate",
    "avg_time_to_execution_secs",
    "trader_surplus",
    "order_size_inflation",
    "order_to_trade_ratio",
    "throughput_orders_per_sec",
    "avg_clearing_latency_micros",
]

# CDA-only (always None/absent on FBA rows) -- report as one-sided descriptive stats.
CDA_ONLY_METRICS = ["book_imbalance", "total_book_depth"]

# FBA-only (always None/absent on CDA rows) -- report as one-sided descriptive stats.
FBA_ONLY_METRICS = ["boundary_concentration", "unexecuted_residual_share"]

# Always None on both engines with this dataset -- no external oracle/mark price.
# Excluded from every table; documented once in Appendix D.
STRUCTURALLY_EMPTY_METRICS = ["pricing_error_bps"]

for d in (OUTPUT_DIR, FIGURES_DIR, DATA_DIR):
    d.mkdir(parents=True, exist_ok=True)

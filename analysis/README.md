# `analysis/`

Python pipeline that turns `results/sol/{fba,cda}_timeseries.csv` (and, for
dataset-level descriptive stats, the raw `src/data/order_statuses/sol/*.data.gz`
input files) into the numbers and figures used in `Thesis/chapters/ch5_results.tex`.

This is analysis-only: it does not touch the Rust simulator (`src/`) and does
not re-run the simulation. It reads what `simulate sol 1` already produced.

## Setup

```
pip install -r analysis/requirements.txt
```

## Running

```
python analysis/run_all.py
```

This runs, in order: `load.py` (read + merge the two timeseries CSVs),
`dataset_stats.py` (parse the raw order-status archive for dataset-level
counts — this step reads ~6GB of gzipped binary and is the slow part, a few
minutes), `descriptive.py` + `paired_stats.py` (compute every number ch5
needs), `make_tables.py` (write `Thesis/data/generated_numbers.tex` — one
`\newcommand` per populated table cell — and `analysis/output/summary.json`,
a full machine-readable dump for reference/debugging), and `make_figures.py`
(write `Thesis/figures/*.pdf`).

Re-run any time the underlying CSVs change; it overwrites its outputs
in place. `Thesis/chapters/ch5_results.tex` reads the generated macros via
`\input{data/generated_numbers}` — nothing about the table markup needs to
change on a re-run, only the numbers.

## Files

- `config.py` — paths and constants (`PRICE_SCALE`, directories).
- `load.py` — loads and merges the two timeseries CSVs with explicit dtypes.
- `dataset_stats.py` — parses the raw 54-byte binary order-status records
  (schema: `data/SCHEMA.md`) for message-count/participant/inter-arrival
  stats that aren't in the per-second timeseries CSVs.
- `paired_stats.py` — CDA/FBA paired-difference statistics with Newey-West
  (HAC) standard errors, for metrics both engines report.
- `descriptive.py` — one-sided descriptive stats for CDA-only/FBA-only
  metrics, plus sample/validation-table numbers.
- `make_tables.py` — turns the above into `Thesis/data/generated_numbers.tex`.
- `make_figures.py` — the ~6 figures described in the thesis plan, saved to
  `Thesis/figures/*.pdf`.
- `run_all.py` — single entrypoint.


## Verification scripts (testing appendix)

Run in this order; each writes `analysis/output/verification_*.json`, and `make_verification_tables.py`
turns them into `Thesis/data/generated_verification.tex` (macros `\Ver...` used by the appendix and chapter 5).

- `verify_raw_data.py` - second decoder of the whole archive (format, order, gaps, order life-cycle, venue fills by second). ~10 min.
- `verify_duplicates.py` - exact repeated records per file. ~10 min.
- `verify_reference_engines.py <root> label=<hour_dir> ...` - independent Python CDA/FBA compared with the Rust output of
  `market_sim simulate <hour_dir> 1` (run from the directory that contains `output/`).
- `verify_replay_hours.py [n]` - volume against the venue's taker fills on n random hours (reference CDA).
- `verify_replay_fidelity.py` - whole-month simulated volume against the venue (needs the two scripts above).
- `verify_price_vs_yahoo.py` - hourly simulated price against Yahoo Finance SOL-USD; writes `Thesis/figures/fig_pricevsyahoo.png`.
- `verify_outputs.py`, `verify_resets.py` - invariants on the result files; signs of engine restarts in the month run.
- `run_property_tests.py`, `run_mutation_tests.py` - Rust test totals; planted-bug (mutation) testing on a copy of `src/`.

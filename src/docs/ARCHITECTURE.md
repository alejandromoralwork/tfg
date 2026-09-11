# `market_sim` — Architecture Guide

This is the whole-project map: what every source file does, how an order
travels from a raw dataset record to a matched trade in each engine, and how
the `simulate` command turns a multi-day replay into a per-interval metric
time series.

For depth on specific pieces, see the sibling docs and don't duplicate them
here:

| Doc | Covers |
|---|---|
| [`ENGINE_DESIGN.md`](ENGINE_DESIGN.md) | The FBA uniform-clearing-price algorithm (candidate prices, three-tier selection, rationing) with a worked example; the module-interaction walkthrough from `add`/`load` to settlement. |
| [`METRICS.md`](METRICS.md) | Authoritative formula-by-formula reference for all 35 `simulate` time-series CSV columns — exact accumulators, `None`-vs-`0.0` edge cases, and CDA-vs-FBA differences. |
| [`TESTING.md`](TESTING.md) | How every test is run and what it proves: the full `cargo test` catalog (80 tests, 11 files) and the runtime `test engine` checklist (37 cases) — both interactively and as terminal/argv commands. |
| [`REFERENCE.md`](REFERENCE.md) | Exhaustive, file-by-file API reference — every public struct/enum/function in the crate with its exact signature and algorithm, one section per source file. |
| [`SCHEMA.md`](SCHEMA.md) | Field-by-field schema of the 54-byte binary order-status record and the CSV PREVIEW format, plus the `mapdir` lookup tables. |
| [`Cancellations.md`](Cancellations.md) | Why the dataset's `accepted` vs `_rejected` file split is *not* "live vs rejected", how cancellations are replayed (and fills deliberately are not), and the unenforced-TIF limitation. |
| [`init.md`](init.md) | Toolchain prerequisites and how to build/run. |
| [`DEPLOY.md`](DEPLOY.md) | Running `simulate` non-interactively — the Docker image and a Google Cloud Batch job spec (with checkpoint-backed retry). |
| [`DATASET.md`](DATASET.md) | The upstream Hyperliquid dataset itself — archive sizes, the reference Python reader, citation, license (not `market_sim`'s own code). |
| [`README.md`](README.md) | Index of this documentation set. |

The research framing (RQ1/RQ2.1/2.2/2.3) lives in the repo-root
`docs/expose.tex` (one level *above* `src/`).

---

## 1. What this is

`market_sim` is the quantitative-simulation half of a thesis comparing two
market-clearing mechanisms fed the **same** real order flow:

- **CDA** — a Continuous Double Auction: every order is matched or rested the
  instant it arrives against a live resting book.
- **FBA** — a Frequent Batch Auction: orders accumulate for a fixed interval
  `τ`, then all clear together at one uniform price chosen to maximize
  matched volume.

The input is Hyperliquid Level-4 order-status data (every order lifecycle
event, including rejected orders) for BTC/ETH/SOL perpetuals, December 2025.
The `simulate` command replays it through fresh, isolated instances of both
engines and writes a per-`τ`-interval catalogue of ~35 microstructure
metrics per engine — the empirical basis for RQ2.

It is a single Rust binary crate (`market_sim`), rooted at `src/` (the
`Cargo.toml` is `src/Cargo.toml`, entry point `src/main.rs`). Only two
third-party crates: `flate2` (pure-Rust gzip for `*.data.gz`) and `colored`
(terminal colour). `curl` / `tar` / `xz` / `cargo` are shelled out to as
external processes rather than added as dependencies.

---

## 2. Module map

```
main.rs                     fn main() { inputs::cli::run(); }
├── mod types               types.rs
├── mod engines             engines/mod.rs
│     ├── mod fba           engines/fba.rs
│     └── mod cda           engines/cda.rs
├── mod inputs              inputs/mod.rs
│     ├── mod cli               inputs/cli.rs
│     ├── mod simulator         inputs/simulator.rs
│     ├── mod simulate_cmd      inputs/simulate_cmd.rs
│     ├── mod scan_cmd          inputs/scan_cmd.rs
│     ├── mod binary_format     inputs/binary_format.rs
│     ├── mod progress          inputs/progress.rs
│     ├── mod replay_checkpoint inputs/replay_checkpoint.rs
│     ├── mod download_cmd      inputs/download_cmd.rs
│     ├── mod update_cmd        inputs/update_cmd.rs
│     └── mod test_suite        inputs/test_suite.rs
└── mod metrics             metrics/mod.rs
      ├── mod stats             metrics/stats.rs
      └── mod timeseries        metrics/timeseries.rs
```

**Architectural boundary:** `engines/*` depend *only* on `types.rs`. They
never import `metrics` or `inputs` and don't know a CLI exists. `inputs/cli.rs`
is the only module that touches both an engine and `metrics::stats`.

The sections below give one paragraph per file — enough to navigate the crate. For the
exhaustive, function-by-function reference (every signature, every struct field, every
algorithm step) see [`REFERENCE.md`](REFERENCE.md).

### 2.1 `types.rs`

The shared vocabulary, depends on nothing else in the crate.

- `PRICE_SCALE = 1_000_000` — every price/notional in the crate is a
  `u128` fixed-point value scaled by 1e6. `Amount` / `Price` are `u128`
  aliases.
- `Side { Buy, Sell }`, `OrderKind { Limit { price }, Market }`,
  `EngineKind { Fba, Cda }` (`.label()` → `"FBA"` / `"CDA"`).
- `Order` — a raw L4 record plus one engine-only field `remaining` (unfilled
  quantity, mutated live by the engines; independent of `sz`, which is the
  record's own historical size snapshot). Key methods:
  - `side()` — from the raw `is_ask` bool.
  - `kind()` — `order_type_id` in `{1,2,3,6}` → `Market`, else `Limit { limit_px }`.
  - `is_new_live_order()` — `status_id == 1 && !is_trigger`, or `status_id == 9`
    (a conditional order that just triggered). Everything else — rejections,
    fills, un-triggered conditionals — is *not* a new live order.
  - `is_cancellation()` — one of the 8 cancel-type `status_id`s. **Excludes**
    `filled` (5): a fill is Hyperliquid's own engine's outcome, not something
    our independently-computed engines replay.
  - `reduce(fill)` — `remaining = remaining.saturating_sub(fill)`.
- `Trade` — `{ trade_id, price, quantity, buyer_id, seller_id, buy_order_id,
  sell_order_id, engine_type, ts, trade_tx_hash, chain_id }`. `ts` is the
  **aggressor**'s timestamp for a CDA trade, the **max order ts in the batch**
  for an FBA trade (see §3).

### 2.2 `engines/cda.rs` — `CdaOrderBook`

The resting book is two ascending `BTreeMap<Price, VecDeque<Order>>` (`bids`,
`asks`); each price level is a FIFO queue ordered by `(ts, oid)`. "Best" is
just which end you read — lowest ask, highest bid. An `oid_index:
HashMap<u64, (Side, Price)>` lets `cancel` find a resting order in `O(log L)`.
`bid_depth` / `ask_depth` / `bid_order_count` / `ask_order_count` are `O(1)`
running counters kept in sync at the three mutation points (rest-insert,
full-fill removal, cancel).

`submit(order) -> Vec<Trade>`:
1. `is_cancellation()` → `cancel(oid)`, return `[]`.
2. not `is_new_live_order()` or `remaining == 0` → return `[]`.
3. Walk the opposite side best-first while `check_price_match` keeps crossing
   (a market order always crosses; a limit crosses at `taker_px ≥ maker_px`
   for a buy). Each fill produces a `Trade` **at the resting maker's own
   limit price** (`get_price(&maker.kind())`) — so every fill in one `submit`
   can print at a different price; the taker's price only gates eligibility.
4. Leftover of a *limit* taker rests at its own limit; leftover of a *market*
   taker is discarded.

Public getters (used by `metrics::stats` and by `simulate_cmd` per record):
`best_bid` / `best_ask` (price keys), `best_bid_order` / `best_ask_order`
(front order at the touch), `bid_depth` / `ask_depth` (whole-side resting
qty), `depth_at_best` (`bid_depth + ask_depth`), `book_imbalance`
(`(bid_depth − ask_depth)/(bid_depth + ask_depth)`), `quoted_spread_bps`
(`(ask−bid)/mid·1e4`), `bids_iter` / `asks_iter` (best-first),
`trade_count` / `executed_volume` / `executed_notional`, `fill_rate`
(`total_filled_qty / total_submitted_qty`, each fill counting `qty·2`).

### 2.3 `engines/fba.rs` — `FbaOrderBook`

`pending_orders: Vec<Order>` is the batch buffer. `submit(order)` merely
queues a new live order (or `cancel`s via `pending_orders.retain`); **nothing
executes on submit**.

`clear() -> Option<ClearingResult>` runs the uniform-price auction (full
algorithm in `ENGINE_DESIGN.md` §1):
- `candidate_prices` = every submitted limit price (market orders contribute
  none; an all-market batch yields an empty set → `clear()` returns `None`
  and rolls the whole batch over).
- `select_price` picks the winner lexicographically: **(1)** maximize matched
  volume `min(demand(P), supply(P))`; **(2)** minimize `|demand − supply|`;
  **(3)** minimize `|P − last_clearing_price|`; **(4)** lowest price.
- Sequential price-time-priority matching walk → `Vec<Trade>`, **every trade
  at the one `clearing_price`**. `Trade.ts` = max `ts` across all orders in
  the batch.
- Residual (partially filled + never-eligible) orders roll back into
  `pending_orders`. `last_clearing_price` updates only when `traded_quantity > 0`.

`ClearingResult { clearing_price, traded_quantity, demand_at_price,
supply_at_price, trades }`. Getters: `best_unfilled_buy` / `best_unfilled_sell`
(pending buffer extremes), `quoted_spread_bps` (implied spread between them,
referenced to `last_clearing_price`), `depth_at_best`, `trade_count` /
`executed_volume` / `executed_notional`, `fill_rate`,
`unexecuted_residual_share` (`|demand − supply| / max(demand, supply)` from
the last clear).

### 2.4 `inputs/mod.rs`

Module aggregator only — declares the ten `inputs::*` children.

### 2.5 `inputs/cli.rs`

The interactive REPL (`pub fn run()`), the only runtime entry point. Parses
one whitespace-split line into a `CliCommand` and dispatches. Owns exactly
one live `FbaOrderBook` + `CdaOrderBook` for hand-testing (`add` / `load` /
`clear` / `metrics` / `orderbook` / `log`). The heavy commands (`simulate`,
`scan`, `download`, `extract`, `update`, `test engine`) delegate to their
modules and never touch the live session books. Strips a leading UTF-8 BOM so
a PowerShell-piped first line still parses.

Commands: `engine <continuous|batch>`, `add <buy|sell> <price|market> <qty>
<user>`, `batch`, `clear`, `load <path>…`, `simulate <path|btc|eth|sol|all>
[interval_secs]`, `scan <path|coin|all>`, `download <coin|all>`, `extract
<coin|all>`, `update [branch]`, `log`, `metrics`/`stats`, `orderbook`/`ob`,
`test engine <continuous|batch>`, `help`, `exit`/`quit`.

### 2.6 `inputs/binary_format.rs`

Decoder for the real 54-byte packed little-endian order-status record.
Unpacks the bit-packed fixed-point encoding (bits 31–29 = decimal-place
count, 28–0 = integer value; signed variant for `triggerCondition`), scales
into `PRICE_SCALE`, and returns a `types::Order`. `RECORD_SIZE = 54`,
`parse_record(&[u8; 54]) -> Option<Order>` (`None` only when size rounds to
0), `looks_like_order_status_record` (a cheap plausibility check that lets
the streaming loader drop a wrong-format file up front). Called by
`inputs/simulator.rs`'s binary path.

### 2.7 `inputs/simulator.rs`

Two intake paths into `types::Order`:
- `load_order_status_csv(path) -> Vec<Order>` — materializes a small
  pre-decoded CSV PREVIEW file (the `load` command). Pure integer parsing:
  `parse_fixed_point` (`"126.67"` → `126_670_000`), `round_to_unit`,
  `parse_dataset_ts` (`"YYYY-MM-DD HH:MM:SS.fffffffff"` → ns via Howard
  Hinnant's `days_from_civil`).
- The streaming path for the real archive (the `simulate` / `scan`
  commands):
  - `collect_input_files(root)` — recursively gathers `*.csv` / `*.gz` under
    a dir and `sort()`s them. Lexicographic sort == chronological
    date/hour, and (because `.` < `_`) puts `sol_00.data.gz` *before*
    `sol_00_rejected.data.gz` — accepted before rejected within an hour.
  - `stream_file(path, bytes_read, on_record)` (`pub(crate)`) — opens one
    file, gzip-decompresses through `flate2::MultiGzDecoder` if `.gz`,
    dispatches to `stream_csv` / `stream_binary`, and calls `on_record` for
    each decoded `Order` — never materializing a file's worth at once. A
    `CountingReader` sits *below* the decoder so `bytes_read` tracks
    physical (compressed) bytes for the progress bar.
  - `stream_records` — the same over a whole file list (kept for the
    `#[ignore]`d integration test; `simulate` now drives the file loop
    itself, see §4).
  - `stream_records_parallel` — round-robins files across worker threads,
    used **only** by `scan` (order-independent tallies).
  - `RunStats { files_processed, files_skipped, records_seen, records_skipped }`.

### 2.8 `inputs/simulate_cmd.rs`

The `simulate` command — the streaming replay driver. Fully described in §4.
Key pieces: `run() -> i32`, the per-record closure, `clear_fba_batch` (fires an
FBA `clear()` on each event-time interval boundary and records a
`BatchClearedEvent`), `flush_hi` (how far to flush after a file — see §4),
`append_rows`, `write_summary`, `MARKOUT_GUARD_SECS`, and the resume logic.

### 2.9 `inputs/scan_cmd.rs`

The `scan` command: streams the same archive as `simulate` but with a cheap
tallying closure instead of the engines — counts records, new-live-orders,
cancellations, other events, using the exact `Order::is_new_live_order` /
`is_cancellation` predicates the engines branch on, so the numbers describe
what a `simulate` run *would* do. No book computation, no metrics, no
`output/` files.

### 2.10 `inputs/progress.rs`

Shared live progress bar: `run_with_progress(total, measure, fmt, extra,
work)` runs `work` on the calling thread while a scoped background thread
redraws a single in-place line (`\r`) ~1×/s from `measure()`. Plus
`human_bytes`, `format_duration`, `eta_suffix`.

### 2.11 `inputs/replay_checkpoint.rs`

Crash-safe progress state for `simulate` (§4.4). `Checkpoint` struct +
hand-rolled `key value` `render`/`parse` + atomic `save_atomic` (`.tmp` then
rename). `SummaryAccumulator` — running `(n, sum, min, max)` per metric so
`summary.txt` is exact without keeping every row; its `metrics()` list is the
single source of truth for which fields the summary reports. `slugify`,
`truncate_data_rows`, `output_dir`. `VERSION` is bumped whenever the CSV
schema changes so a stale checkpoint is refused rather than producing a
column-misaligned file.

### 2.12 `inputs/download_cmd.rs`

`download` / `extract` — fetches a coin's order-status archive from Zenodo
(`curl -L -C -`) and unpacks it (`xz -dk` then `tar -xf`, two steps to dodge
a Windows bsdtar pipe deadlock on multi-GB archives) into
`data/order_statuses/<coin>/`. Probes for a bundled Git-for-Windows `xz.exe`.

### 2.13 `inputs/update_cmd.rs`

`update [branch]` — self-update without `git`: downloads the repo as a branch
`.tar.gz` from `codeload.github.com`, `cargo build --release` in a temp dir,
then re-execs the fresh binary. Never touches the running checkout or its own
locked `.exe`.

### 2.14 `inputs/test_suite.rs`

Deterministic checklists runnable at runtime via
`test engine <continuous|cda|batch|fba|metrics|all>` (interactively or as an
argv command, so they work without a Rust toolchain — `all` exits `0`/`1`).
Every case builds fresh isolated state and hand-designed events (explicit
ids/timestamps, never wall-clock) and asserts against an independently
hand-computed expectation:
- `run_cda_tests` / `run_fba_tests` — the engine getters: crossing / resting /
  partial fill / price-time priority, market orders, non-live filtering,
  cancellation replay (and the deliberate non-replay of `filled`), plus a
  hand-computed metrics scenario per engine.
- `run_timeseries_metric_tests` — the *streaming* pipeline: drives
  `metrics::timeseries::MetricsRecorder` with synthetic
  `OrderMessage`/`TradeEvent`/`BatchClearedEvent`/`BookSnapshot`s and checks
  every one of the 35 CSV columns against a worked value (a different code
  path from the engine getters above). Full case-by-case catalogue:
  [`TESTING.md`](TESTING.md).

### 2.15 `metrics/mod.rs` / `metrics/stats.rs`

`stats.rs` is a print-only orchestrator for the interactive `metrics` /
`orderbook` commands — no calculation of its own, it just calls the engines'
own public getters and prints them side by side. This is the "pull" metrics
path: state-now snapshots, no event log.

### 2.16 `metrics/timeseries.rs`

The "push" metrics path — the only place that keeps an event log. Described
in §4.3 and §5. `MetricsRecorder` (the streaming aggregator), the four event
types (`OrderMessage`, `TradeEvent`, `BatchClearedEvent`, `BookSnapshot`),
`IntervalMetrics` (one CSV row), `Carry` (cross-flush state), `bucket_of`,
`csv_header` / `csv_row` / `csv_rows`.

---

<a id="order-lifecycle"></a>
## 3. The order lifecycle — record to matched trade

```
                 data/order_statuses/<date>/<coin>_<HH>.data.gz  (54-byte binary)
                 data/.../order_statuses_*_PREVIEW.csv            (pre-decoded CSV)
                                     │
        binary_format::parse_record  │  simulator::parse_row
                                     ▼
                              types::Order
                     (ts, oid, user_id, side, kind, limit_px,
                      orig_sz, remaining, status_id, is_trigger, …)
                                     │
                 ┌───────────────────┴───────────────────┐
                 │  is_new_live_order()?  is_cancellation()?  (else: dropped — a
                 │                                             rejection or a fill)
                 ▼                                             ▼
        ┌─────────────────┐                          ┌──────────────────┐
        │   CDA path      │                          │    FBA path      │
        │ cda.submit(o)   │                          │  fba.submit(o)   │
        └────────┬────────┘                          └────────┬─────────┘
                 │ new live order:                            │ new live order:
                 │  walk opposite side best-first,            │  push to pending_orders
                 │  cross while price matches,                │  (nothing executes yet)
                 │  Trade @ each resting MAKER's price,       │
                 │  Trade.ts = aggressor's ts,               │ cancellation:
                 │  rest the limit leftover                   │  pending_orders.retain(≠ oid)
                 │ cancellation: remove from bids/asks        │
                 ▼                                            ▼
          Vec<Trade>  (0..n, each maybe a           on each event-time τ boundary:
           different price)                          fba.clear()
                                                       candidate prices → volume-max →
                                                       min-imbalance → nearest-to-last →
                                                       lowest; price-time rationing walk
                                                     ClearingResult
                                                       { clearing_price, traded_quantity,
                                                         demand_at_price, supply_at_price,
                                                         trades: Vec<Trade> }
                                                     every Trade @ the one clearing_price,
                                                     Trade.ts = max order ts in the batch,
                                                     residual rolls into pending_orders
```

The two engines are fed the identical `Order` stream, in strict sequence, and
never see each other. The CDA prices each fill at the maker it crossed; the
FBA prices the whole batch at one volume-maximizing price. That difference is
the object of study.

---

<a id="simulate-pipeline"></a>
## 4. The `simulate` time-series pipeline, end to end

`simulate <path|btc|eth|sol|all> [interval_secs]` → `inputs::simulate_cmd::run`.
Default `interval_secs = 1`. `all` runs `btc`, `eth`, `sol` back to back as
independent runs. Output goes to `output/<slug>/` where `<slug>` is the
source path's last component (`sol`, `20251201`, …).

### 4.1 File streaming

`collect_input_files` gives a deterministically sorted file list.
`simulate_cmd` iterates it **one file at a time**, calling
`simulator::stream_file` per file with a per-record closure. A live progress
bar (`progress::run_with_progress`) tracks compressed bytes read against the
total on-disk size.

### 4.2 The per-record closure

For every decoded `Order`:

1. `set_anchor(order.ts)` on both recorders on the very first record, so the
   FBA and CDA time grids share an origin.
2. Push an `OrderMessage { ts, oid, user_id, side, limit_price, quantity,
   accepted }` — recorded **unconditionally**, before any accept/reject
   gating, so `order_to_trade_ratio` etc. see the whole stream.
3. **FBA interval boundaries.** If `order.ts` has crossed the next
   `τ`-width boundary in event-time, call `clear_fba_batch` for each crossed
   boundary: snapshot the pending buffer (for the depth schedule + the
   `net_order_flow` regressor), run `fba.clear()`, and record a
   `BatchClearedEvent` (with `clearing_price: None` if nothing crossed).
   FBA trades from the clear are recorded as `TradeEvent`s with
   `aggressor_side: None`.
4. **The engines.** If the record `is_new_live_order() || is_cancellation()`
   (otherwise both engines would no-op): `fba.submit(order.clone())`;
   capture the pre-trade midpoint (`reference_price`); `cda.submit(order)`.
   CDA trades are recorded as `TradeEvent`s with the pre-trade
   `reference_price` and `aggressor_side: Some(order.side())`.
5. Record a `BookSnapshot { ts, best_bid, best_ask, best_bid_qty,
   best_ask_qty, bid_depth, ask_depth, depth_schedule, compute_time }` — the
   `depth_schedule` (bps-banded resting volume) is an `O(book)` scan, so it's
   only recomputed on an actionable record and cached otherwise.

### 4.3 `MetricsRecorder` — streaming aggregation

The recorder holds a bounded window of `trades` / `batches` / `books` and a
`Carry` of cross-flush scalars. `bucket_of(ts, anchor, τ)` maps a timestamp
to its interval start.

- `emit(&messages, hi) -> Vec<IntervalMetrics>` runs `compute_range` over
  every bucket in `[emitted_upto, hi)`: it groups the retained events into
  per-bucket accumulators and produces one `IntervalMetrics` row per bucket
  (empty buckets included, so the grid is contiguous). Most metrics are pure
  running sums; a few (`realized_volatility`, `intra_interval_price_dispersion`)
  are a two-pass stddev over that one bucket's retained values. Two metrics
  span bucket boundaries via the `Carry`:
  - `amihud_illiquidity` — `|Δclose| / volume`, using `Carry.prev_close`
    (last reference-price close of the previous non-empty bucket).
  - FBA `kyle_lambda` — regresses this bucket's clearing-price moves on the
    batch's net order flow; `Carry.prev_clearing` is the previous priced
    batch's clearing price.
- After a flush, `prune()` drops events whose bucket is now emitted, and the
  caller trims its `messages` slice the same way — so memory stays bounded to
  roughly one input file's span of activity, not the whole run.
- `finish(&messages, max_seen_ts)` flushes the tail at end of run.

**Flush cadence — once per input file.** After finishing file `N` (not the
last), `simulate` flushes every bucket that ends more than
`MARKOUT_GUARD_SECS` (35 s — the 30 s realized-spread markout plus the 5 s
`kyle_lambda` markout, with margin) before **both** `last_ts(N)` and
`first_ts(N+1)` (peeked cheaply with `simulator::peek_first_ts`). The first
bound means the bucket's own forward-looking mids already exist; the second
means no later file — `collect_input_files` sorts them — can still add records
to it. So the CSV grows once per input file (first rows after file #2 on the
hourly dataset — file #1's frontier is still below the anchor). A record that
somehow arrives for an already-flushed bucket (a non-ts-sorted custom input
dir) is counted as `late_events_dropped`.

```
record ──► OrderMessage / TradeEvent / BatchClearedEvent / BookSnapshot
              │
              ▼   (retained, bounded window)
        MetricsRecorder.{trades, batches, books}
              │
   after each input file N (not last):
              │   hi = min(peek_first_ts(N+1), last_ts(N)) − MARKOUT_GUARD_SECS   (grid-aligned)
              ▼
        emit(&messages, hi)  ──►  Vec<IntervalMetrics>  (one row per bucket up to hi)
              │                          │
              │                          ├─► append to output/<slug>/{fba,cda}_timeseries.csv
              │                          └─► fold into SummaryAccumulator
              ▼
        prune()  +  messages.retain(…)   (drop emitted events → bounded memory)
              │
              ▼
        checkpoint.txt  (atomic rewrite: files done, cursor, carries, summary accs)
```

### 4.4 Output layout, summary, resume

`output/<slug>/` holds:
- `fba_timeseries.csv` / `cda_timeseries.csv` — header written once, rows
  appended once per input file.
- `summary.txt` — written once at completion from the `SummaryAccumulator`s
  (avg/min/max per metric across intervals + per-engine totals).
- `checkpoint.txt` — rewritten atomically after every file.

**Resume:** re-running `simulate` on the same source loads `checkpoint.txt`,
skips finished files, and appends (after trimming each CSV back to the
checkpoint's recorded row count, in case a crash left it one file ahead).
Resume is *approximate* — engine books and the in-flight event window are not
persisted, so a resumed run leaves a short gap of empty interval rows (~one
file's span) at the seam, unless nothing had been flushed yet, in which case
it restarts from the top and is exact. A checkpoint whose `source` / `interval`
/ file-count
/ `VERSION` doesn't match is refused with a "delete that directory to start
over" message.

---

## 5. The metric catalogue

One row per `τ`-interval per engine; 35 CSV columns. Scope: **U**niversal /
**C**DA-only / **F**BA-only / **X** (needs external data, always empty).
"bps" = basis points; prices are `PRICE_SCALE` (1e6) fixed-point unless
noted. RQ column maps to `docs/expose.tex`.

**Full formula, accumulator, and edge-case detail for every column below (including the
`kyle_lambda` CDA-vs-FBA construction that used to be spelled out here) now lives in
[`METRICS.md`](METRICS.md) — the authoritative reference. Treat the table below as an index,
not a source of truth.**

| Column | Scope | RQ | Short description |
|---|---|---|---|
| `engine`, `interval_start_ns`, `interval_width_ns` | U | — | Row key: `"FBA"`/`"CDA"`, bucket start (ns since epoch), `τ` in ns. |
| `quoted_spread_bps` | U | 2.1 | Mean bid-ask (CDA) / unfilled-extremes (FBA) spread, bps. |
| `depth_at_best` | U | 2.1 | Mean top-of-book volume (touch-only for CDA; demand+supply at clearing price for FBA). |
| `depth_within_{10,50,100}bps` | U | 2.1 | Mean cumulative resting volume within x bps of mid / clearing price. |
| `book_imbalance` | C | 2.1 | Mean `(bid_qty − ask_qty)/(bid_qty + ask_qty)`. |
| `total_book_depth` | C | 2.1 | Mean whole-book resting volume, all levels both sides. |
| `effective_spread_bps` | U | 2.1 | Quantity-weighted deviation of trade price from the pre-trade reference price. |
| `realized_spread_bps_{1,5,30}s` | U | 2.1 | Same, vs the reference price `Δ` seconds *after* the trade. |
| `price_impact_bps_{1,5,30}s` | U | 2.1 | `effective_spread_bps − realized_spread_bps_Δs`. |
| `amihud_illiquidity` | U | 2.1 | Canonical Amihud (2002) illiquidity ratio, carried across intervals. |
| `kyle_lambda` | U | 2.1 | Price-impact slope, bps per SOL — built differently per engine; see `METRICS.md`. |
| `realized_volatility` | U | 2.2 | Root-sum-of-squares of consecutive reference-price returns. |
| `intra_interval_price_dispersion` | U | 2.2 | Relative stddev of trade prices, in bps. |
| `pricing_error_bps` | X | 2.2 | Always empty — needs an external oracle/mark-price feed the dataset lacks. |
| `executed_volume`, `executed_notional` | U | 2.3 | Σ trade quantity, Σ `quantity·price` (raw PRICE_SCALE). |
| `vwap` | U | 2.3 | `executed_notional / executed_volume`. |
| `trade_count` | U | 2.3 | Trades in the interval. |
| `fill_rate` | U | 2.3 | Σ filled / Σ original quantity, bucketed by submission interval. |
| `avg_time_to_execution_secs` | U | 2.3 | Mean first-fill latency, bucketed by submission interval. |
| `trader_surplus` | U | 2.3 | Realized price improvement vs each side's own limit. |
| `order_size_inflation` | U | 2.3 | Mean per-user `orig/filled` over users with some fill. |
| `order_to_trade_ratio` | U | 2.3 | Messages / trades in the interval. |
| `boundary_concentration` | F | 2.3 | Share of order arrivals in the final 10% of each batch's window. |
| `throughput_orders_per_sec` | U | 2.3 | Wall-clock, non-deterministic — see `METRICS.md` for the numerator/denominator caveat. |
| `avg_clearing_latency_micros` | U | 2.3 | Wall-clock, non-deterministic — per-`submit` for CDA, per-`clear` for FBA. |
| `unexecuted_residual_share` | F | 2.3 | `|demand − supply| / max(demand, supply)` from the batch clear. |

Every formula above is checked against a hand-computed value by the runtime
`test engine metrics` checklist — see [`TESTING.md`](TESTING.md) for the full case catalogue.

---

## 6. Running & testing

**For the full testing reference — every one of the 80 `cargo test` functions, every one of
the 37 runtime `test engine` checklist cases, and exactly how to run each in the terminal —
see [`TESTING.md`](TESTING.md). This section is just the quick-reference command list.**

```
cd src
cargo build                 # or --release
cargo run                   # no args → launches the REPL; `help` lists commands
cargo test                  # unit + integration tests
cargo test -- --ignored     # the tests that need real data / external tools
```

Inside the REPL:

```
simulate data/sample/order_statuses/20251201 1     # replay the bundled 1-hour sample
scan sol                                            # count records without running the engines
download sol                                        # fetch + extract the SOL archive from Zenodo
test engine batch                                   # FBA matching-behaviour checklist
test engine metrics                                 # time-series metric catalogue checklist
test engine all                                     # every checklist
```

`market_sim` also takes the same commands as **arguments** for scripting /
containers — `market_sim simulate sol 1` runs that one command and exits with
its status (`0` ok, `1` run-time failure, `2` bad request). No arguments →
the REPL. `market_sim test engine <continuous|cda|batch|fba|metrics|all>` works
this way too, exiting `0` if every checklist case passed and `1` otherwise — so
the checklists can be run inside the Docker image with no Rust toolchain. See
[`DEPLOY.md`](DEPLOY.md) for the image and the Google Cloud Batch job spec.

`test_suite.rs`'s cases run via `test engine …` (interactively or as an argv
command), not `cargo test`. `run_cda_tests` / `run_fba_tests` check the engine
getters; `run_timeseries_metric_tests` drives `metrics::timeseries::MetricsRecorder`
directly and checks every CSV column against a hand-computed value. The
`#[ignore]`d `cargo test` cases (`streams_the_real_sample_gz_file_correctly`,
`scan_reproduces_known_totals_for_the_real_sample_data`,
`extract_archive_produces_the_expected_files`) need `data/sample/` or network
access.

---

## 7. Known approximations & limitations

- **`pricing_error_bps`** is always empty — no oracle/mark-price feed in the
  dataset. Kept as a documented column, not silently omitted.
- **TIF is parsed but not enforced** (no ALO-reject-if-crossing, no
  IOC-no-rest). See `Cancellations.md`.
- **Fills are not replayed, only cancellations** — a `filled` status is
  Hyperliquid's own engine's outcome, irrelevant to what our independently
  computed engines decide.
- **Approximate resume** — a resumed run leaves a short gap of empty interval
  rows (~one input file's span) at the seam (engine/window state is not
  persisted).
- **`throughput_orders_per_sec` / `avg_clearing_latency_micros`** measure
  wall-clock engine cost and are non-deterministic between runs and machines.
  `throughput`'s numerator counts every message (incl. rejected) but its
  denominator only sums the measured `submit`/`clear` time, so the two
  populations don't line up; `avg_clearing_latency_micros` is per-`submit` for
  CDA but per-`clear` for FBA under one column name.
- **`price_impact_bps_Δ = effective_spread_bps − realized_spread_bps_Δ`** is
  formed from two separately quantity-weighted trade populations; they can
  differ for trades near the end of the retained window. Harmless in a real run
  — the 35 s `MARKOUT_GUARD_SECS` flush guarantees every flushed bucket's
  ≤30 s markouts already exist.
- **`kyle_lambda`** — the CDA 5s horizon and sweep grouping are choices (a
  multi-level sweep is one regression observation, though `trade_count` counts
  it as several); the FBA construction (contemporaneous on pre-selection net
  order flow) differs from the CDA one, and the near-zero FBA result is
  inherent to the auction's price-selection rule, not noise.
- The `metrics::timeseries` catalogue is checked column-by-column against
  hand-computed values by `test engine metrics` (`inputs/test_suite.rs`), which
  also runs as an argv command in the container.
- **Not built:** the "implementation shortfall of standardized probe orders"
  and the "simulated latency arbitrageur" metrics from RQ2.1 / RQ2.3.

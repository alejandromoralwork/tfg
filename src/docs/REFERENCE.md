# API Reference — every file, one by one

> See [`README.md`](README.md) for the documentation index and
> [`ARCHITECTURE.md`](ARCHITECTURE.md) for the narrative whole-project map (module
> boundaries, the order lifecycle, the `simulate` pipeline). This file is the exhaustive
> counterpart: every public struct/enum and every public (and load-bearing private) function
> in the crate, file by file, with exact signatures and precise algorithm descriptions —
> ARCHITECTURE.md explicitly defers here rather than duplicating this level of detail.

Files are covered in dependency order: `types.rs` first (depends on nothing), then
`engines/*` (depends only on `types.rs`), then `metrics/*` (depends on `engines/*` +
`types.rs`), then `inputs/*` (the CLI/pipeline layer that ties everything together), then
`main.rs`.

---

<a id="typesrs"></a>
## `types.rs`

The shared vocabulary. Depends on nothing else in the crate. No inline tests.

**Constants / aliases**: `PRICE_SCALE: u128 = 1_000_000` — every price/notional in the crate
is fixed-point, scaled by 1e6. `Amount = u128`, `Price = u128`.

**`enum Side { Buy, Sell }`**, **`enum OrderKind { Limit { price: Price }, Market }`**,
**`enum EngineKind { Fba, Cda }`** — `label(&self) -> &'static str` → `"FBA"`/`"CDA"`.

**`struct Order`** (`Clone, Debug`) — the raw L4 record plus one engine-only field:
`ts: u64, user_id: String, is_builder: bool, status_id: u8, is_ask: bool, limit_px: Price,
sz: Amount` (the record's own historical size snapshot — *not* live remaining),
`oid: u64, timestamp_diff: u32, trigger_condition: i128, triggered: bool, is_trigger: bool,
has_children: bool, is_position_tpsl: bool, reduce_only: bool, order_type_id: u8, tif_id: u8,
trigger_px: Price, orig_sz: Amount, closed_ts: Amount`, optional resolved label strings
`status/order_type/tif: Option<String>`, and `remaining: Amount` (mutated live by the
engines, independent of `sz`).

- `Order::limit(id, participant_id, side, price, quantity, timestamp) -> Self` — synthetic
  constructor for the CLI's `add` command: hardcodes `status_id=1` (open), `order_type_id=0`
  (Limit), `tif_id=1` (Gtc), `remaining = orig_sz = quantity`.
- `Order::market(...) -> Self` — same shape, `limit_px=0`, `order_type_id=1` (Market),
  `tif_id=2` (Ioc).
- `side(&self) -> Side` — `is_ask` → `Sell`, else `Buy`.
- `kind(&self) -> OrderKind` — `order_type_id` `1|2|3|6` (Market, Stop Market, Take-Profit
  Market, Vault Close) → `Market`; everything else (`0|4|5` — Limit, TP-Limit, Stop-Limit —
  and any unrecognized id) → `Limit { limit_px }`.
- `limit_price(&self) -> Option<Price>` — `Some(price)` iff `kind()` is `Limit`.
- `is_new_live_order(&self) -> bool` — `status_id==1 && !is_trigger`, or `status_id==9`
  (a conditional order that just triggered). Everything else (rejections, fills, an
  un-triggered conditional) is not a new live order.
- `is_cancellation(&self) -> bool` — `matches!(status_id, 2|7|10|11|12|13|14|16)` — the 8
  cancel-type codes. Deliberately **excludes `filled` (5)**: a fill is Hyperliquid's own
  matching engine's outcome, not something this crate's independently-computed engines
  replay.
- `reduce(&mut self, fill: Amount)` — `remaining = remaining.saturating_sub(fill)`.

**`struct Trade`** (`Clone, Debug`) — `trade_id: u64, price: Price, quantity: Amount,
buyer_id: String, seller_id: String, buy_order_id: u64, sell_order_id: u64,
engine_type: EngineKind, ts: u64, trade_tx_hash: Option<String>, chain_id: Option<u64>`.
`ts` is the **aggressor's** timestamp for a CDA trade, the **max order ts in the batch** for
an FBA trade.

---

<a id="engines-modrs"></a>
## `engines/mod.rs`

Two lines: `pub mod fba; pub mod cda;`. Nothing else.

---

<a id="cdars"></a>
## `engines/cda.rs`

`CdaOrderBook` — the continuous double-auction engine. 6 inline tests (see
[`TESTING.md`](TESTING.md#cargo-cda)).

**`struct CdaOrderBook`** — `bids`/`asks: BTreeMap<Price, VecDeque<Order>>` (private; each
price level a FIFO queue by arrival, ascending `BTreeMap` on both sides so "best" is
`last_key_value` for bids, `first_key_value` for asks; invariant — no price key ever maps to
an empty `VecDeque`); `oid_index: HashMap<u64, (Side, Price)>` (O(log L) cancel lookup, kept
in sync at 3 mutation points: resting-insert, full-fill removal, explicit cancel);
`pub executed_trades: Vec<Trade>`, `next_trade_id: u64`; `bid_order_count`/`ask_order_count:
usize` (O(1) running counters); `total_submitted_qty`/`total_filled_qty: Amount` (feed
`fill_rate()` — every trade of quantity `q` adds `q*2` to `total_filled_qty`, once per side,
so a fully-matched book reaches `fill_rate = 1.0` rather than capping at 0.5); `bid_depth`/
`ask_depth: Amount` (O(1) running totals of resting quantity per side).

Free functions: `get_price(kind: &OrderKind) -> Price` (`Limit{price}` → `price`; `Market` →
`PRICE_SCALE`, unreachable in practice since a market order is never rested);
`check_price_match(taker_kind, maker_kind, taker_side) -> bool` (`true` if either side is
`Market`; else `Limit` vs `Limit`: `Buy` needs `taker_px >= maker_px`, `Sell` needs
`taker_px <= maker_px`); `arrival_key(o) -> (u64, u64)` = `(ts, oid)` (intra-level FIFO
tie-break); `insert_sorted(level, order)` — O(1) `push_back` fast path when arrival order
holds, else an O(level depth) `partition_point` + `insert` fallback for out-of-order arrival
at the same price.

- `new() -> Self` — zeroed, `next_trade_id = 1`.
- `submit(&mut self, order: Order) -> Vec<Trade>`:
  1. `is_cancellation()` → delegate to `cancel(oid)`, return `[]`.
  2. `!is_new_live_order() || remaining == 0` → return `[]`.
  3. `total_submitted_qty += remaining`.
  4. Walk the opposite side's best level while `check_price_match` holds and
     `fill = min(order.remaining, best.remaining) > 0`: reduce both, push a `Trade` priced
     at `get_price(&maker.kind())` — **the resting maker's own price**, so every fill in one
     `submit` call can print at a different price. Full maker fill → `pop_front()`, remove
     from `oid_index`, decrement counters, drop the price key if the level is now empty.
  5. Leftover of a limit taker rests (`insert_sorted` into its own side); leftover of a
     market taker is discarded.
  6. Returns exactly the trades this call appended.
- `cancel(&mut self, oid: u64) -> bool` — O(log L) lookup via `oid_index`, removes from the
  level's `VecDeque`, drops the price key if now empty, decrements counters. `false` (not a
  panic) for an unknown oid.
- `best_bid()`/`best_ask() -> Option<Price>`; `best_bid_order()`/`best_ask_order() ->
  Option<&Order>` (front order at the best level); `bid_count()`/`ask_count() -> usize`;
  `bids_is_empty()`/`asks_is_empty()`; `bids_iter()`/`asks_iter() -> impl Iterator<Item =
  &Order>` (best-first, then FIFO); `trade_count()`; `executed_volume()` (`Σ qty`);
  `executed_notional()` (`Σ (qty·price)/PRICE_SCALE`, divided per-trade then summed);
  `quoted_spread_bps() -> Option<f64>` (`None` if either side empty or `mid==0`, else
  `(ask-bid)/mid·1e4`); `bid_depth()`/`ask_depth()`; `depth_at_best() -> Amount` =
  `bid_depth()+ask_depth()` (**whole-book both sides**, despite the name — the time-series
  module's own same-named metric is touch-only, see [`METRICS.md`](METRICS.md)); `
  book_imbalance() -> Option<f64>` (`None` if total depth is 0, else `(bid-ask)/total`);
  `fill_rate() -> Option<f64>` (`None` if nothing submitted, else `total_filled_qty as f64 /
  total_submitted_qty as f64`).

---

<a id="fbars"></a>
## `engines/fba.rs`

`FbaOrderBook` — the frequent-batch-auction engine. 3 inline tests (see
[`TESTING.md`](TESTING.md#cargo-fba)). Full clearing-price algorithm and a
worked example: [`ENGINE_DESIGN.md`](ENGINE_DESIGN.md).

**`struct ClearingResult`** (`Clone, Debug`) — `clearing_price: Price, traded_quantity:
Amount, demand_at_price: Amount, supply_at_price: Amount, trades: Vec<Trade>`.

**`struct FbaOrderBook`** — `pub pending_orders: Vec<Order>` (the batch buffer),
`pub executed_trades: Vec<Trade>`, `pub last_clearing_price: Option<Price>`,
`next_trade_id: u64`, `total_submitted_qty: Amount`, `last_demand_at_price`/
`last_supply_at_price`/`last_unexecuted_quantity: Amount` (snapshot of the most recent
`clear()`, feeds `unexecuted_residual_share`).

- `new() -> Self`.
- `submit(&mut self, order: Order)` — `is_new_live_order()` → `total_submitted_qty +=
  remaining; pending_orders.push(order)`; else `is_cancellation()` → `cancel(oid)`; else
  (rejection / untriggered conditional) silently dropped.
- `cancel(&mut self, oid: u64) -> bool` — `pending_orders.retain(|o| o.oid != oid)`.
- `clear(&mut self) -> Option<ClearingResult>`:
  1. `orders = mem::take(&mut self.pending_orders)`; empty → `None` immediately.
  2. `candidate_prices(&orders)` then `select_price(&orders, candidates)`. No candidates at
     all (an all-market batch) → **restore** `self.pending_orders = orders` and return
     `None` — the batch rolls to the next round rather than clearing at a guessed price.
  3. `eligible_orders(&orders, Buy, price)` / `..Sell..` — sorted index lists.
  4. `batch_ts = max(o.ts)` across the batch (fallback 0) — every `Trade.ts` in the batch.
  5. Sequential two-pointer walk over the sorted buy/sell indices: each fill mutates both
     orders' `remaining` in place and pushes a `Trade` at `price: clearing_price` (**uniform
     for the whole batch** — the defining FBA property).
  6. `traded_quantity = Σ trade.quantity`; `last_clearing_price` updates **only if
     `traded_quantity > 0`**.
  7. `residual_orders = orders.into_iter().filter(|o| o.remaining > 0)` → new
     `pending_orders`.
  8. `last_unexecuted_quantity = demand_at_price.abs_diff(supply_at_price)` — deliberately
     `|demand−supply|`, **not** a sum over `residual_orders`' remaining (which would include
     never-eligible orders and could push the share above 1).
- `candidate_prices(&self, orders) -> BTreeSet<Price>` — every submitted limit price; market
  orders contribute none.
- `select_price(&self, orders, candidates) -> Option<(Price, demand, supply)>` — for each
  candidate: `volume = demand.min(supply)`, `imbalance = demand.abs_diff(supply)`. A
  candidate replaces `best` iff, lexicographically: (1) `volume > best.volume`; else tied,
  (2) `imbalance < best.imbalance`; else tied, (3) closer to `last_clearing_price` (or, with
  no history, straight to) (4) the lower price. `None` only if `candidates` is empty.
- `demand_supply_evaluators(&self, orders) -> (impl Fn(Price)->Amount, impl Fn(Price)->
  Amount)` — precomputed **once per batch** (replacing a since-removed function called
  `aggregate_volume`, which rescanned the whole batch per candidate — an O(batch²) cost the
  source's own comments describe fixing): splits into buy/sell market totals + sorted limit
  `(price, qty)` vectors with a cumulative-quantity prefix sum; `demand_at(p)` = market qty +
  every buy-limit priced `>= p` (via `partition_point`); `supply_at(p)` = market qty + every
  sell-limit priced `<= p`.
- `eligible_orders(&self, orders, side, price) -> Vec<usize>` — indices qualifying at
  `price`, sorted by `(order_priority, ts, oid)`.
- `order_priority(&self, order) -> (u8, Price)` — `(0,0)` for any market order (most
  aggressive); for a limit, `(1, u128::MAX - price)` on the buy side (higher price sorts
  earlier) and `(1, price)` on the sell side (lower price sorts earlier).
- `trade_count()`/`executed_volume()`/`executed_notional()` — same formulas as CDA's
  namesakes.
- `best_unfilled_buy()`/`best_unfilled_sell() -> Option<Price>` — max/min limit price among
  pending buys/sells (market orders excluded).
- `quoted_spread_bps() -> Option<f64>` — needs both unfilled extremes;
  `reference = last_clearing_price.unwrap_or((buy+sell)/2)`; `None` if `reference==0`; else
  `(sell-buy)/reference·1e4`.
- `depth_at_best() -> Amount` = `Σ pending_orders[..].remaining` — for FBA this genuinely is
  "everything currently unmatched" (unlike CDA's whole-book `depth_at_best`).
- `fill_rate() -> Option<f64>` — `None` if nothing submitted; else `filled =
  total_submitted_qty.saturating_sub(Σ pending remaining)`, `filled/total_submitted_qty`
  (derived by subtraction — safe here because nothing in FBA vanishes without either
  matching or staying queued, unlike a CDA market order that can vanish against empty
  liquidity, which is why CDA tracks `total_filled_qty` directly instead).
- `unexecuted_residual_share() -> Option<f64>` — `None` if
  `max(last_demand_at_price, last_supply_at_price)==0`; else
  `last_unexecuted_quantity / that_max`.
- Free fn `cumulative_quantities(sorted: &[(Price,Amount)]) -> Vec<Amount>` — prefix-sum
  helper, `cum[0]=0`, `cum[i] = cum[i-1] + sorted[i-1].1`.

---

<a id="metrics-modrs"></a>
## `metrics/mod.rs`

Two lines: `pub mod stats; pub mod timeseries;`.

---

<a id="statsrs"></a>
## `metrics/stats.rs`

The **pull** metrics path for the interactive `metrics`/`orderbook` commands. No calculation
of its own — every number comes from a `pub fn` already on `FbaOrderBook`/`CdaOrderBook`. No
inline tests.

- `fmt_opt(v: Option<f64>) -> String` — `Some(x)` → `"{x:.4}"`, `None` → `"n/a"`.
- `print_summary(fba, cda)` — banner + `print_fba` + `print_cda`.
- `print_fba(fba: &FbaOrderBook)` — prints, in order: `quoted_spread_bps`, `depth_at_best`,
  `trade_count`, `executed_volume`, `executed_notional`, `fill_rate`,
  `unexecuted_residual_share`.
- `print_cda(cda: &CdaOrderBook)` — prints: `quoted_spread_bps`, `depth_at_best`,
  `trade_count`, `executed_volume`, `executed_notional`, `fill_rate`, `book_imbalance`.

---

<a id="timeseriesrs"></a>
## `metrics/timeseries.rs`

The **push** metrics path — the only place in the crate that keeps an event log, fed
exclusively by `simulate`. 17 inline tests. **Every one of the 35 CSV columns' exact
formula, accumulator, and edge-case behavior is documented in full in
[`METRICS.md`](METRICS.md)** — this section covers the surrounding machinery only, to avoid
duplicating that content.

**Constants**: `DEPTH_BPS_THRESHOLDS: [u32;3] = [10,50,100]`, `NS_PER_SEC: u64 =
1_000_000_000`, `REALIZED_SPREAD_HORIZONS_SECS: [u64;3] = [1,5,30]`,
`KYLE_LAMBDA_HORIZON_SECS: u64 = 5`.

**Event structs**: `OrderMessage { ts, oid, user_id, side, limit_price: Option<u128>,
quantity, accepted }` (recorded before any accept/reject gating); `TradeEvent { trade,
reference_price: Option<u128>, aggressor_side: Option<Side> }` (`aggressor_side` always
`None` for FBA); `BatchClearedEvent { ts, batch_open_ts, clearing_price: Option<u128>,
demand_at_price, supply_at_price, net_order_flow: f64, traded_quantity,
unexecuted_quantity, best_unfilled_buy, best_unfilled_sell, depth_schedule: [(u128,u128);3],
compute_time: Duration }`; `BookSnapshot { ts, best_bid, best_ask, best_bid_qty,
best_ask_qty, bid_depth, ask_depth, depth_schedule, compute_time }` (qty fields are
touch-only; depth fields are whole-book).

`depth_schedule(reference, levels) -> [(u128,u128);3]` — free function; for each `(price,
side, qty)`, computes `bps_away` and accumulates `qty` into every threshold it falls within
(cumulative — a 10bps-away level also counts toward 50/100bps).

**`struct IntervalMetrics`** — one CSV row; `empty(engine, interval_start, interval_width)`
constructs a zeroed/`None`d row (this is what pre-populates every grid slot before
accumulation, guaranteeing the contiguous-grid-with-gaps behavior).

**`struct OrderState`** (private) — per-oid fill-tracking rebuilt fresh at each
`compute_range` call: `user_id, limit_price, orig_qty, filled_qty, first_seen_ts,
first_fill_ts`.

`bucket_of(ts, anchor, interval_width) -> u64` — `anchor + ((ts-anchor)/width)*width`
(saturating for `ts < anchor`).

**`struct Carry`** (`Clone, Copy, Default, Debug`) — `prev_close: Option<f64>`,
`prev_clearing: Option<f64>` — the two cross-flush-boundary values, persisted in
`checkpoint.txt` so a resume doesn't lose them.

**`struct MetricsRecorder`** — `engine: EngineKind, interval_width: u64, anchor:
Option<u64>, emitted_upto: u64, carry: Carry, trades: Vec<TradeEvent>, batches:
Vec<BatchClearedEvent>, books: Vec<BookSnapshot>, late_events_dropped: u64`.

- `new(engine, interval_width_ns)` / `resume(engine, interval_width_ns, anchor,
  emitted_upto, carry, late_events_dropped)` (rebuilds from a checkpoint — engine books and
  the in-flight event window are **not** restored, only the grid/cursor/carries).
- `set_anchor(&mut self, ts)` — public, no-op once already set; lets `simulate` pin both the
  FBA and CDA recorder to the identical grid origin.
- `record_trade`/`record_batch`/`record_book_snapshot` — each pins the anchor if unset, then
  either drops-and-counts (`is_late`, i.e. its bucket is already emitted) or retains the
  event.
- `emit(&mut self, messages, hi) -> Vec<IntervalMetrics>` — the core flush: computes
  `compute_range(messages, emitted_upto, hi, anchor, carry)`, updates state, returns rows.
- `finish(&mut self, messages, max_seen_ts) -> Vec<IntervalMetrics>` = `emit(messages,
  max_seen_ts + 1)`.
- `prune(&mut self)` — drops every retained trade/batch/book whose bucket is now emitted.
- `compute_range(...)` (private) — the entire per-bucket computation; see
  [`METRICS.md`](METRICS.md) for what it computes, column by column.

**CSV rendering**: `fmt_opt(v) -> String` (`Some(x)` → `"{x:.6}"`, `None` → `""` — an empty
CSV field, note the different convention from `stats.rs`'s `"n/a"`); `csv_header()`;
`csv_row`/`csv_rows` (append-friendly, newline-terminated).

**Supporting mechanics**: `price_at_or_after(target_ts) -> Option<f64>` — binary search over
a combined, ts-sorted series of CDA midpoints and FBA clearing prices (book snapshots win
ties against a batch at the identical ts); `mean(values) -> f64`; `stddev(values) -> f64`
(population, `/n`, `0.0` for `len < 2`).

---

<a id="inputs-modrs"></a>
## `inputs/mod.rs`

Ten lines, one per child module: `binary_format, cli, download_cmd, progress,
replay_checkpoint, scan_cmd, simulate_cmd, simulator, test_suite, update_cmd`.

---

<a id="clirs"></a>
## `inputs/cli.rs`

The interactive REPL (`pub fn run()`) and the non-interactive argv entry point
(`run_once`). 15 inline tests.

**`enum CliCommand`** (`Debug, PartialEq`) — `Add { side, price: Option<u128>, qty, user }`,
`Engine(EngineMode)` (`Continuous`/`Batch`), `Batch`, `Clear`, `Log`, `Metrics`, `Orderbook`,
`TestEngine(TestTarget)` (`Cda`/`Fba`/`Metrics`/`All`), `Load { paths: Vec<String> }`,
`Simulate { path, interval_secs: Option<u64> }`, `Scan { path }`, `Download(DownloadTarget)`,
`Extract(DownloadTarget)`, `Update { branch: Option<String> }`, `Help`, `Exit`.

**`CliCommand::parse(input: &str) -> Option<Self>`** — (1) strips a leading UTF-8 BOM
(`\u{feff}`, since `str::trim()` alone doesn't; needed for a PowerShell-piped first line);
(2) `input.trim().split_whitespace().collect()`; empty → `None`; (3) dispatches on
`parts[0].to_lowercase()`. Full per-command parsing rules, usage-error messages, and every
alias (`stats`≡`metrics`, `ob`≡`orderbook`, `cda`≡`continuous`, `fba`≡`batch`,
`quit`≡`exit`) are exercised by this file's own 15 tests — see
[`TESTING.md`](TESTING.md#cargo-cli) for the exhaustive case list.
`parse_download_target(arg)` (shared helper for `download`/`extract`) maps
`"btc"/"eth"/"sol"/"all"` case-insensitively, else prints an error and returns `None`.

**`run()`** — owns one `FbaOrderBook` + one `CdaOrderBook` for the whole session (state:
`order_id_counter: u64`, `current_mode: EngineMode`, starts `Batch`). Loop: read a line
(`Ok(0)` EOF or `Err` → `break`, preventing an infinite fast spin on a closed stdin), parse
it, dispatch:
- `Engine(mode)` — switches `current_mode`.
- `Add{...}` — builds an `Order::limit`/`Order::market` with `ts = now_ns()` (wall clock)
  and the next `order_id_counter`; `Batch` mode → `fba.submit`; `Continuous` mode →
  `cda.submit`, prints each resulting trade.
- `Batch` (the no-arg command) — `Continuous` mode renders the book; `Batch` mode renders
  the pending buffer.
- `Clear` — warns if in `Continuous` mode (clear only affects the FBA pipeline), then always
  clears FBA.
- `Log` — merges both engines' `executed_trades` sorted by `ts`.
- `Metrics`/`Orderbook` — delegate to `metrics::stats`.
- `TestEngine(target)` — `run_test_target(target)`, return code discarded (REPL keeps
  running).
- `Load{paths}` — `simulator::load_order_status_csv` per path, submits every order into the
  active engine, reports live/total counts; a per-path error is printed and loading
  continues with the next path.
- `Simulate`/`Scan`/`Download`/`Extract`/`Update` — delegate to their own modules.
- `Help`/`Exit` — print help / break the loop.
- Unparseable line → red `[ERROR] Command sequence unrecognized...`.

**`fn run_test_target(target: TestTarget) -> i32`** — runs `test_suite::run_cda_tests`/
`run_fba_tests`/`run_timeseries_metric_tests` (whichever the target selects; `All` runs all
three) through `print_checklist`, ANDs their pass/fail, returns `0`/`1`. See
[`TESTING.md`](TESTING.md#runtime-checklist).

**`run_once(args: &[String]) -> i32`** — the non-interactive path. Dispatches on
`args[0].to_lowercase()`:
- `simulate` — needs a path/coin; optional numeric interval; delegates to
  `simulate_cmd::run`'s own return code.
- `scan` — needs a path/coin; `scan_cmd::run`; returns `0`.
- `download`/`extract` — needs a valid coin target; returns `0` on dispatch.
- `update` — always `0`.
- `help`/`--help`/`-h` — `0`.
- `test` — needs `engine <target>`; valid target → `run_test_target`'s `0`/`1`.
- Any REPL-only stateful command (`add`, `engine`, `batch`, `clear`, `log`, `metrics`,
  `stats`, `orderbook`, `ob`, `load`) → rejected, **exit 2** ("interactive-only — run with no
  arguments").
- Anything else unrecognized → **exit 2**.

**Exit-code convention**: **0** = success; **1** = reserved for `test engine <target>` when
any checklist case failed (the only place this function itself produces a `1`); **2** =
usage/argument error (missing/invalid argument, unknown coin/target, an interactive-only
command invoked via argv, or an unrecognized top-level command). `simulate` is the one
exception that returns whatever `simulate_cmd::run` itself decided (0/1/2), not a value fixed
in `run_once`.

`run()` (interactive) never returns an exit code and owns live cross-command order-book
state; `run_once(args)` is stateless between invocations and only wires up the
"batch-shaped" commands that don't need that persistent state.

---

<a id="simulatorrs"></a>
## `inputs/simulator.rs`

Two intake paths into `types::Order`. 10 inline tests (2 ignored).

**`pub const MIN_COLUMNS: usize = 22`** — column count of the known PREVIEW CSV header.

- `load_order_status_csv(path) -> io::Result<Vec<Order>>` — reads the whole file
  (`fs::read_to_string`), checks the header via `looks_like_order_status_header`; a
  structurally-wrong header prints one `[WARN]` and returns `Ok(vec![])` (not an error).
  Otherwise `parse_row` per line: a malformed row prints one `[WARN] Skipping malformed row
  N` and continues; a zero-size row is silently dropped. Materializes everything — fine at
  PREVIEW scale, not for the real archive.
- `parse_row(line) -> Result<Option<Order>, ()>` — splits on `,`, requires `>= MIN_COLUMNS`
  fields, reads fixed column indices (`ts`@0, `user_id`@1, `status_id`@3, `is_ask`@4,
  `limit_px`@5, `oid`@7, `is_trigger`@11, `order_type_id`@15, `tif_id`@16, `orig_sz`@18,
  status/order_type/tif labels @19-21). `orig_sz == 0` → `Ok(None)`. On this path `sz` and
  `orig_sz` collapse to the same rounded value (unlike the binary path below, which decodes
  them independently).
- `parse_fixed_point(s) -> Option<u128>` (`pub(crate)`, also used by `cli.rs`'s `add`) —
  decimal string → `PRICE_SCALE`-scaled `u128`; fractional part padded/truncated to exactly
  6 digits. `"126.67"` → `126_670_000`.
- `round_to_unit(s) -> Option<u128>` (`pub(crate)`, also used by `add`) — decimal string →
  nearest whole unit, round-half-up on the first fractional digit only (exact, since `>=0.5`
  always has a first digit `>=5`). `"39.35"`→39, `"39.5"`→40.
- `parse_dataset_ts(s) -> Option<u64>` — parses `"YYYY-MM-DD HH:MM:SS.fffffffff"` into ns
  since epoch via pure integer math, delegating day-count to `days_from_civil` (Howard
  Hinnant's public-domain proleptic-Gregorian algorithm, reproduced verbatim).

**Streaming section** (the real binary/gzip archive path):
- `struct RunStats { files_processed, files_skipped, records_seen, records_skipped }`.
- `collect_input_files(root: &Path) -> io::Result<Vec<PathBuf>>` — recursively gathers
  `.csv`/`.gz` files, then `sort()`s them: lexicographic order gives chronological date/hour
  order, and (because `.` sorts before `_`) puts `sol_00.data.gz` before
  `sol_00_rejected.data.gz` within an hour. Accepts a single file path too.
- `struct CountingReader<R>` — wraps a `Read`, atomically adds every successful read's byte
  count to a shared counter; sits *below* the gzip decoder so it tracks physical
  (compressed) bytes for the progress bar.
- `open_reader(path, bytes_read) -> io::Result<Box<dyn Read>>` — `File` → `BufReader` →
  `CountingReader`, wrapped in `flate2::read::MultiGzDecoder` if `.gz` (the `Multi` variant
  handles concatenated gzip members).
- `stream_records(files, bytes_read, on_record) -> io::Result<RunStats>` (`#[allow(dead_code)]`)
  — sequential whole-file-list streamer; **no longer used by `simulate`** (which now drives
  its own per-file loop to interleave flush/checkpoint) — kept only for one `#[ignore]`d
  integration test.
- `stream_records_parallel(files, bytes_read, on_record: impl Fn+Sync) -> io::Result<RunStats>`
  — used **only** by `scan` (order-independent tallies). `n_threads =
  available_parallelism().min(files.len())`; each worker takes every `n`th file
  round-robin (static balance, not work-stealing); a `Mutex<()>` guards only the per-file
  announcement print; the first file-level error across all workers is returned after every
  worker finishes its own share (no early-abort).
- `stream_file(path, bytes_read, on_record) -> io::Result<(records_seen, records_skipped,
  file_dropped)>` (`pub(crate)`) — dispatches to `stream_csv`/`stream_binary` by extension.
- `peek_first_ts(path) -> io::Result<Option<u64>>` (`pub(crate)`) — decodes just the first
  record's `ts` without materializing the file (CSV: scans up to 256 lines; binary: reads up
  to 256 raw 54-byte records, bailing early if the very first fails
  `looks_like_order_status_record`). This is what `simulate` calls on the *next* file to know
  how far it's safe to flush.
- `stream_csv`/`stream_binary` (private) — the per-file record loops; `stream_binary` wraps
  its reader in a 256KiB `BufReader` specifically so each 54-byte read doesn't trigger its
  own gzip-decompression call. Both run a first-record/first-line structural sanity check
  and drop the whole file with one `[WARN]` if it fails, rather than one warning per row.
- `read_one_record(reader, buf: &mut [u8; 54]) -> io::Result<bool>` — fills exactly 54 bytes
  via a manual read-loop (handles short reads from a decompressor); `Ok(false)` = clean EOF;
  a truncated trailing record → `Err(UnexpectedEof)`.

---

<a id="binary_formatrs"></a>
## `inputs/binary_format.rs`

The 54-byte packed order-status record decoder. 11 inline tests. Full byte-layout spec:
[`SCHEMA.md`](SCHEMA.md).

- `RECORD_SIZE: usize = 54` (`pub(crate)`).
- `scale_to_price_units(value, decimals) -> u128` — exact multiply for `decimals <= 6`;
  round-half-up divide for `decimals == 7` (the one case with more precision than
  `PRICE_SCALE` holds).
- `decode_price(encoded: u32) -> Price` — bits 31-29 = decimal-place count, bits 28-0 =
  integer value. `$96,543.21` → `96_543_210_000` (the `SCHEMA.md` worked example).
- `decode_signed_price(encoded: u32) -> i128` — used for `triggerCondition`: bits 31-29
  decimals, **bit 28 sign** (1=negative — a custom bit-packed sign, not two's-complement),
  bits 27-0 value.
- `decode_qty_to_whole_units(encoded: u32) -> Amount` — decodes via `decode_price`'s bit
  scheme, then rounds to the nearest whole unit (same round-half-up convention as
  `simulator::round_to_unit`).
- `parse_record(bytes: &[u8; 54]) -> Option<Order>` — exact byte offsets: `ts`@0..8 (u64 LE),
  `user_id`@8..12 (u32), `is_builder`@12, `status_id`@13, `is_ask`@14, `limit_px`@15..19,
  `sz`@19..23, `oid`@23..31 (u64 LE), `timestamp_diff`@31..35, `trigger_condition`@35..39,
  `triggered`@39, `is_trigger`@40, `has_children`@41, `is_position_tpsl`@42,
  `reduce_only`@43, `order_type_id`@44, `tif_id`@45, `trigger_px`@46..50, `orig_sz`@50..54.
  `orig_sz == 0` → `None`. `status`/`order_type`/`tif` labels are deliberately left `None` —
  resolving them costs an allocation per record, not worth it at tens-of-millions-of-records
  scale since the engines only read the numeric ids. Note: unlike the CSV path, `sz` and
  `orig_sz` are decoded independently here from separate byte ranges and need not be equal.
- `looks_like_order_status_record(bytes: &[u8; 54]) -> bool` (`pub(crate)`) — a plausibility
  heuristic, not a real signature check: `ts` in `[2020-01-01, 2035-01-01)` (ns), and
  `status_id <= 17`, `order_type_id <= 6`, `tif_id <= 5` (the documented max ids from
  `mapdir/*.csv`). Lets the streaming loader drop a wrong-format file with one warning
  rather than one per record; not a defense against adversarial input.

---

<a id="simulate_cmdrs"></a>
## `inputs/simulate_cmd.rs`

The `simulate` command — the streaming replay driver, and the most involved file in the
crate. 3 inline tests. The full per-record algorithm and flush cadence are also narrated in
[`ARCHITECTURE.md` §4](ARCHITECTURE.md#simulate-pipeline); this
section is the exact function-level breakdown.

**Constants**: `DEFAULT_INTERVAL_SECS: u64 = 1`; `NS_PER_SEC: u64 = 1_000_000_000`;
`MARKOUT_GUARD_SECS: u64 = 35` — how far event-time must move past a bucket's end before
it's flushed: just enough that its own forward-looking markout mids (30s realized-spread +
5s `kyle_lambda`, with margin) already exist. **Not** the flush cadence — a file boundary is.

**`pub fn run(path_str: &str, interval_secs: Option<u64>) -> i32`** — exit codes: `0` ok,
`1` a run-time failure (streaming/IO — safe to retry/resume), `2` a bad request (no data
found, incompatible checkpoint — retrying won't help). Algorithm:

1. `"all"` (case-insensitive) → recursively runs `btc`/`eth`/`sol` in sequence, each fully
   independent (own files/engines/progress bar/output dir); returns the worst exit code.
2. Resolves `interval_secs` (default 1) and a coin shorthand (`"btc"/"eth"/"sol"` →
   `data/order_statuses/<coin>`) or a literal path.
3. `collect_input_files` — empty result or IO error → `[ERROR]` + return `2`.
4. **Resume-or-fresh decision**: loads `checkpoint.txt` if present. A mismatch on `source`/
   `interval`/`files_total` → `[ERROR]` + `2` ("delete that directory to start over"). Already
   `complete` → `[OK]` + `0`. Otherwise resumes; no checkpoint → fresh
   `Checkpoint::fresh(...)`.
5. **Restart-from-scratch detection**: a checkpoint exists but nothing was ever flushed (a
   crash before the first per-file flush) → rebuild fresh but keep cumulative
   `elapsed_secs`.
6. CSV setup: resuming → `truncate_data_rows` both CSVs back to the checkpoint's row counts
   (a crash between append and checkpoint-save can leave a CSV one file ahead); fresh →
   write headers.
7. Builds **fresh** `FbaOrderBook`/`CdaOrderBook` (books are never persisted, even on
   resume) and `MetricsRecorder`s — via `::resume(...)` (carrying anchor/cursor/carries) if
   `ckpt.anchor.is_some()`, else `::new(...)`.

**The per-record closure**, in exact order, for every decoded `Order`:
1. `records_seen` += 1; remember `last_seen_ts`.
2. **First record only**: pin `anchor` and call `set_anchor` on **both** recorders — same
   grid origin for FBA and CDA from the very first record, not waiting for FBA's first batch
   clear.
3. Push an `OrderMessage` unconditionally (before any accept/reject gating).
4. **FBA boundary detection**: a `while order.ts >= next_fba_boundary` loop (not `if` — a
   quiet gap can cross several boundaries at once) calls `clear_fba_batch` per boundary
   crossed.
5. **Actionability gate**: `is_actionable = is_new_live_order() || is_cancellation()` —
   anything else is a guaranteed no-op in both engines' `submit`, so the clone+call is
   skipped.
6. If actionable: `fba.submit(order.clone())`.
7. `reference_price = midpoint(cda.best_bid(), cda.best_ask())` captured **before** the CDA
   submit.
8. Timed `cda.submit(order)` (only if actionable) → `Vec<Trade>` + `compute_time`.
9. Each CDA trade → `TradeEvent { reference_price, aggressor_side: Some(order.side()) }`.
10. Reads best bid/ask/qty and O(1) whole-book depths.
11. **Depth-schedule caching**: an O(book-size) scan via `timeseries::depth_schedule`,
    recomputed **only if actionable**, else the previous cached value is reused.
12. Records a `BookSnapshot`.

**After each file**: updates checkpoint counters; on the **last** file, flushes any
still-open FBA batch first. Flush: last file → `finish(&messages, max_seen_ts)` (everything);
else → `emit(&messages, flush_hi(...))`. Appends rows to both CSVs, folds them into the
`SummaryAccumulator`s, `prune()`s both recorders and the shared `messages` log, persists
`Checkpoint::save_atomic`.

- `fn flush_hi(anchor, next_first_ts, file_last_ts, guard_ns, width) -> u64` — `None` anchor
  → `0`. Else `cap = min(next_first_ts.unwrap_or(file_last_ts), file_last_ts)`, `safe =
  cap.saturating_sub(guard_ns)`; if `safe > anchor`, floor-align to the bucket grid
  (`anchor + ((safe-anchor)/width)*width`); else return `anchor` (a no-op for `emit`).
- `fn clear_fba_batch(fba, recorder, batch_open_ts, batch_close_ts)` — no-op on an empty
  buffer. Snapshots `(limit_price, side, remaining)` **before** `clear()` consumes it (for
  the batch's depth schedule); computes `net_order_flow` (signed sum, `kyle_lambda`'s FBA
  regressor) from that same pre-clear snapshot; times `fba.clear()`. A `None` clear result
  still records a `BatchClearedEvent` with `clearing_price: None` — "a batch that finds no
  crossing price is still worth knowing about." Every trade recorded with
  `aggressor_side: None` (no taker/maker in a uniform-price batch).
- `fn append_rows(path, rows) -> io::Result<()>` — no-op if empty; else opens
  append-mode, writes, flushes to the OS.
- `fn write_summary(out_dir, source, ckpt) -> io::Result<()>` — formats `summary.txt`:
  file/record counts, cumulative wall-clock, the 35s markout-guard note, late-dropped counts,
  per-engine totals, then each `SummaryAccumulator::render_section`.
- `fn run_timestamp() -> String` / `fn civil_from_days(z: i64) -> (i64,u32,u32)` —
  `YYYYMMDD_HHMMSS` via Howard Hinnant's inverse civil-calendar algorithm (no chrono
  dependency).
- `fn midpoint(bid, ask) -> Option<u128>` — both present → average; one-sided → that side;
  neither → `None`.

---

<a id="scan_cmdrs"></a>
## `inputs/scan_cmd.rs`

The `scan` command: the same archive `simulate` would read, through a cheap tallying closure
instead of the engines. 2 inline tests (1 ignored). Uses
`simulator::stream_records_parallel` (tallies are order-independent, unlike `simulate`'s
strict sequential replay).

- `enum Category { NewLiveOrder, Cancellation, Other }` — mirrors exactly what the engines'
  own `submit()` branches on.
- `fn classify(order: &Order) -> Category` — `is_new_live_order()` → `NewLiveOrder`; else
  `is_cancellation()` → `Cancellation`; else `Other`.
- `pub fn run(path_str: &str)` — `"all"` loops the three coins independently (no combined
  total); resolves the same coin-shorthand as `simulate_cmd::run` (deliberately duplicated,
  not shared, since each caller's error text differs); `collect_input_files` with the same
  three-way error pattern; runs `stream_records_parallel` under `progress::run_with_progress`
  with three plain `AtomicU64` counters (`new_live_orders`, `cancellations`,
  `other_events`); prints a final report (total, each category with its plain-English
  meaning, and skipped-record count).

---

<a id="progressrs"></a>
## `inputs/progress.rs`

Shared live progress-bar machinery for `simulate`/`scan`/`download`/`extract`. 3 inline
tests.

- `human_bytes(n: u64) -> String` — divides by 1024 through `["B","KiB","MiB","GiB","TiB"]`
  while `value >= 1024.0`, formats to 2 decimals.
- `print_bar(current, total: Option<u64>, elapsed, fmt, extra)` — with a known total: a
  30-char `[####----]` bar + percentage + `fmt(current)/fmt(total)` + an ETA suffix; without
  one: `"... {fmt(current)} so far (elapsed Ns)"`, no percentage possible. On a real terminal:
  `\r`-redraws in place; piped/redirected (containers, Cloud Logging): one `println!` per
  call instead, so container logs read as a scrolling series of snapshots rather than one
  garbled line.
- `fn eta_suffix(current, total, elapsed) -> String` — withheld (`""`) if nothing done,
  already done, or `elapsed < 2s` (too little signal to trust a rate); else
  `rate = current/elapsed_secs`, ETA = `(total-current)/rate` formatted via
  `format_duration`.
- `format_duration(secs: u64) -> String` — coarsest two units: `"{d}d {h}h"` /
  `"{h}h {m:02}m"` / `"{m}m {s:02}s"` / `"{s}s"`.
- `run_with_progress<T>(total, measure, fmt, extra, work: impl FnOnce()->T) -> T` — runs
  `work` on the calling thread while a scoped background thread polls `measure()` ~10×/sec
  and redraws via `print_bar` roughly once/second, until a `done` flag (set after `work`
  returns) ends the loop. **The final post-scope redraw call passes `Duration::ZERO` as
  elapsed** (not the real total) — deliberate, so the bar visibly reaches its end state
  rather than stopping short with a stale ETA.

---

<a id="replay_checkpointrs"></a>
## `inputs/replay_checkpoint.rs`

Crash-safe progress state for `simulate` — a hand-rolled `key value` text format, no
serialization dependency, written atomically. 4 inline tests. Explicitly **approximate on
resume**: only the bucket grid, emit cursor, cross-flush carries, cumulative counters, and
summary accumulators are persisted — never the engine books or the in-flight event window.

**Constants**: `CHECKPOINT_FILE = "checkpoint.txt"`, `FBA_CSV`/`CDA_CSV`, `SUMMARY_FILE =
"summary.txt"`. `VERSION: u32 = 3` — **v2** added the `kyle_lambda` column and the
`{fba,cda}_prev_clearing` carry; **v3** changed the flush cadence from a blanket window to
the per-input-file look-ahead described above (renaming `settle_ns` → `markout_guard_ns`).
Each bump rejects an older checkpoint outright rather than trying to interpret it.

- `slugify(source: &str) -> String` — last path component, non-`[A-Za-z0-9._-]` chars → `_`;
  empty result → `"run"`. `"sol"` → `"sol"`.
- `struct Checkpoint` (all fields `pub`) — `version, source, interval_ns,
  markout_guard_ns` (informational, not validated on resume), `files_total, files_done,
  last_file, anchor: Option<u64>, emitted_upto: u64, records_seen, records_skipped,
  files_processed, files_skipped, fba_rows_written, cda_rows_written, fba_prev_close:
  Option<f64>, cda_prev_close, fba_prev_clearing: Option<f64>` (FBA `kyle_lambda` carry),
  `cda_prev_clearing` (**always `None`** — kept for symmetry only), `fba_late_dropped,
  cda_late_dropped: u64, elapsed_secs: f64` (cumulative across every run in this output
  dir), `fba_summary, cda_summary: SummaryAccumulator, complete: bool`.
- `Checkpoint::fresh(source, interval_ns, markout_guard_ns, files_total) -> Self`.
- `Checkpoint::load(dir) -> io::Result<Option<Checkpoint>>` — `NotFound` → `Ok(None)`; other
  IO error → `Err`; parse failure → `Err(InvalidData)` with the path in the message.
- `Checkpoint::save_atomic(&self, dir) -> io::Result<()>` — writes to a `.tmp` sibling, then
  (no atomic-replace on Windows) removes the existing final file if present, then renames —
  "the price of no-deps atomicity on Windows; a crash in that window just means the next run
  starts from the previous checkpoint (or fresh)."
- `render`/`parse` (private) — line-oriented `key value`; `Option<f64>` uses a `"NONE"`
  sentinel; unknown keys ignored (forward-compatible); any `version != VERSION` is an
  immediate, explicit error.

**`SummaryAccumulator`** — running per-metric aggregation without keeping the full series
(so a multi-day run's summary doesn't require holding every row in memory).
- `enum Scope { Universal, FbaOnly, CdaOnly, NeedsExternalData(&'static str) }`.
- `fn metrics() -> Vec<MetricDesc>` — the single source-of-truth catalogue of all 30
  summarizable metric rows (5 of the 35 CSV columns — the 3 key columns plus `trade_count`/
  `executed_volume`/`executed_notional`, which get their own dedicated running sums instead
  — aren't in this per-metric list), in print order, each tagged with its `Scope`.
- `push`/`fold(&mut self, m: &IntervalMetrics)` — for each metric descriptor, if the row has
  a value, updates that metric's running `(n, sum, min, max)`.
- `serialize`/`deserialize` — a `;`-joined blob, `name=n:sum:min:max` chunks keyed by name
  (order-independent, so the metric list can change without breaking old checkpoints).
- `render_section(&self, label) -> String` — `"(no intervals)"` if nothing folded yet; else
  one formatted avg/min/max row per metric, with `FbaOnly`/`CdaOnly`/`NeedsExternalData` rows
  rendered as an explicit `"n/a — <reason>"` on the other engine.

- `truncate_data_rows(path, keep: u64) -> io::Result<()>` — rewrites a CSV to its header plus
  only the first `keep` data lines; `NotFound` → `Ok(())`; already the right length → no-op.
- `output_dir(source: &str) -> PathBuf` = `Path::new("output").join(slugify(source))`.

---

<a id="download_cmdrs"></a>
## `inputs/download_cmd.rs`

`download`/`extract` — shells out to `curl`/`tar` rather than adding an HTTP client or LZMA
decoder dependency (both ship natively on Windows 10 1803+ and virtually every Linux/macOS
install). 6 inline tests (1 ignored).

- `enum Coin { Btc, Eth, Sol }` — `label()` → `"btc"/"eth"/"sol"`; `all() -> [Coin;3]`;
  `url()` → the exact Zenodo download URL for that coin's `orders_202512.tar.xz`.
- `enum DownloadTarget { Coin(Coin), All }`.
- `pub fn run(target)` / `pub fn run_extract(target)` — loop every coin for `All`; dispatch
  to `download_one`/`extract_one` per coin.
- `fn download_one(coin)` — skips if `dest_already_populated` (a nested `.gz`/`.data` file
  already exists — "delete it first to re-fetch"). Runs `curl -L -C - --fail --retry 3
  --retry-all-errors -o <archive> <url>`: `-C -` resumes a partial download; `--fail` avoids
  writing an HTTP error page as if it were the archive; **`--retry-all-errors`** matters
  specifically because plain `--retry` only covers curl's "transient" error class — it
  doesn't cover a mid-transfer TLS drop (observed on these multi-GB Zenodo downloads on
  Windows/Schannel), which `--retry-all-errors` does cover. On success, calls
  `extract_archive`.
- `fn extract_one(coin)` — same populated/dest checks, requires the archive already exists
  locally (`"Run 'download <coin>' first"` otherwise), then `extract_archive` directly (no
  curl step) — for retrying just extraction.
- `fn extract_archive(label, archive, dest, url)` — **deliberately two separate steps, not
  one `tar -xf archive.tar.xz`**: Windows' bundled `tar.exe` (bsdtar) has no native LZMA
  support and shells out to an external `xz` internally, a combination observed to deadlock
  on multi-GB archives on this platform. So: (1) spawn `xz -dk <archive>` (keep the source
  `.xz`, remove it manually only on full success), progress measured by polling the growing
  `.tar` file's size; (2) spawn `tar -xf <tar> -C <dest>`, progress measured by
  `dir_size(dest)`. Any failure at either step leaves the archive in place for retry and
  prints a manual-extraction fallback (e.g. via 7-Zip) pointing at the URL. Full success
  removes the `.tar` then the `.tar.xz`.
- `fn dir_size(dir) -> u64` — recursive sum of every regular file's byte length.
- `fn xz_uncompressed_size(archive) -> Option<u64>` — runs `xz -l --robot`, parses the
  tab-separated ROBOT MODE output for the uncompressed byte count; `None` on any failure
  (progress degrades to bytes-only, not fatal).
- `fn xz_on_path()` / `xz_search_dirs()` / `path_with_bundled_xz()` — probes whether `xz` is
  already runnable; if not, searches the common Git-for-Windows install locations
  (`Program Files*/Git/{mingw64,usr}/bin`) for a bundled `xz.exe` and, if found, hands the
  child process (only the child — never this process's own `PATH`) an augmented `PATH`.
- `fn dest_already_populated(dest) -> bool` — recursive check for any nested `.gz`/`.data`
  file.

---

<a id="update_cmdrs"></a>
## `inputs/update_cmd.rs`

`update [branch]` — self-update without requiring `git`. **The one file in `inputs/` with no
`#[cfg(test)]` block at all.**

**Constants**: `REPO_OWNER`, `REPO_NAME`, `BIN_NAME = "market_sim"`.

- `pub fn run(branch: Option<&str>)` — `branch` defaults to `"main"`. Builds a unique
  per-process, per-second scratch dir under the OS temp folder
  (`{REPO_NAME}-update-{pid}-{unix_secs}`) — never touches the running checkout.
  `download_and_extract` failure → removes the scratch dir entirely and returns (nothing
  salvageable). `build` failure → **leaves the source in place** for inspection and returns
  (unlike the download-failure case). Success → `relaunch`.
- `fn download_and_extract(work_dir, branch) -> Option<PathBuf>` — downloads
  `https://codeload.github.com/<owner>/<repo>/tar.gz/refs/heads/<branch>` (GitHub's plain
  tarball snapshot endpoint, no git protocol/auth needed) via `curl -fSL`, extracts via
  `tar -xzf` (gzip is bsdtar's built-in codec — no LZMA two-step needed here, unlike
  `download_cmd`'s `.tar.xz`), and returns the first directory found directly under
  `work_dir` — **deliberately not** hardcoding GitHub's `{repo}-{branch}` naming convention,
  so an upstream naming change can't silently break this.
- `fn build(extracted_root) -> Option<PathBuf>` — verifies `<root>/src/Cargo.toml` exists
  (this project's own `Cargo.toml`-in-`src/` layout); runs `cargo build --release` with
  **inherited stdio** (not captured) so a long rebuild streams live compiler output instead
  of looking hung; verifies the resulting binary exists at
  `src/target/release/market_sim(.exe)`.
- `fn relaunch(new_exe)` — Unix: `exec()`s the new binary, **replacing the current process
  image entirely** (no parent left behind); only returns on failure. Windows (no `exec`
  syscall exists): spawns the new binary as a child inheriting this console, then
  `process::exit(0)` — "the practical equivalent, just with a parent that briefly outlives
  the handoff instead of none." Sidesteps the classic self-update problem of overwriting a
  locked, currently-running executable: the build happens entirely in a fresh temp
  directory, and only the final relaunch depends on the running process at all.

---

<a id="test_suiters"></a>
## `inputs/test_suite.rs`

The runtime `test engine <target>` checklist — production code, **not** `#[cfg(test)]`, so
it runs inside the compiled binary with no Rust toolchain. Full case-by-case catalogue with
scenario numbers and expected values: [`TESTING.md` §3](TESTING.md#runtime-checklist).

- `struct TestCase { name: &'static str, passed: bool, detail: String }`, built by
  `check(name, passed, detail)`.
- `approx_eq(a,b)` — absolute tolerance `0.01`; `approx_rel(a,b)` — relative tolerance
  `1e-9 * b.abs().max(1.0)` (used for `PRICE_SCALE`-magnitude sums).
- Deterministic scenario builders: `limit`, `market`, `non_live` (a status_id=2 row with no
  prior live order sharing its oid — proves a cancel-status row never *enters* the book as
  live), `cancel_event`, `filled_event`, plus a `tmsg`/`ttrade`/`tcda_trade`/`tfba_trade`/
  `tbook`/`tbatch` family for the metrics checklist's synthetic `MetricsRecorder` events.
- `print_checklist(engine_label, cases) -> bool` — prints the boxed PASS/FAIL report (see
  [`TESTING.md` §3.2](TESTING.md#output-format)); returns whether every case passed.
- `run_cda_tests() -> Vec<TestCase>` (15 cases), `run_fba_tests() -> Vec<TestCase>` (14
  cases), `run_timeseries_metric_tests() -> Vec<TestCase>` (8 cases) — each builds fresh,
  isolated state per case and asserts against an independently hand-computed expectation.

---

<a id="mainrs"></a>
## `main.rs`

```rust
mod types;
mod engines;
mod inputs;
mod metrics;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        inputs::cli::run();
    } else {
        std::process::exit(inputs::cli::run_once(&args));
    }
}
```

No arguments → the interactive REPL. With arguments → run that one command via `run_once`
and exit with its status code (see the `cli.rs` section above for the full 0/1/2
convention) — the non-interactive path used by shell scripts, Docker, and GCP Batch (e.g.
`market_sim simulate sol 1`).

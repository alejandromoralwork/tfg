# Running the Simulator

Practical guide to building and driving `simulation`, the interactive CLI
that sits on top of the two matching engines (`engines::cda::ContinuousEngine`,
`engines::fba::BatchAuctionEngine`) and the `metrics` crate. For *why* the
engines are built the way they are, see [ENGINE_DESIGN.md](ENGINE_DESIGN.md).

All commands below assume your shell's working directory is
`thesis-market-models/` (this repo's root, next to `Cargo.toml`).

---

## 1. Build and run

```
cargo build --workspace
cargo run -p simulation
```

You'll land in an interactive prompt:

```
sim [FBA]> 
```

The bracketed label tracks which matching engine is currently active — see
§2. Type `help` any time to reprint the command list.

Prefer non-interactive runs? Pipe a text file of commands into the same
binary — see `simulation/src/testing_commands.txt` for a worked example:

```
cargo run -p simulation < simulation/src/testing_commands.txt
```

---

## 2. Switching engines: CDA vs FBA

```
sim [FBA]> engine continuous     # or: engine cda
sim [CDA]> engine batch          # or: engine fba
```

This flips `current_mode` in `simulation/src/main.rs`, which changes what
every subsequent `add`/`load` call actually does:

| Mode | Engine | What happens to a new order |
|---|---|---|
| `batch` (`fba`) | `BatchAuctionEngine` | Queued into `pending_orders`; sits there until you run `clear`, which computes one uniform clearing price for the whole batch and matches by price-time priority. |
| `continuous` (`cda`) | `ContinuousEngine` | Matched **immediately** against the resting book; any unfilled limit remainder rests on the book for future orders to hit. |

Both engines only ever trade the single default pair, SOL/USD
(`AssetPair::default()`) — there's no asset argument anywhere in the CLI.

Both wrappers (`FbaSimulator`, `CdaSimulator`) feed the same
`MetricsCollector` shape, so `metrics` (§5) can compare the two runs on the
same time grid regardless of which mode you were in when you fed them.

---

## 3. Sending orders from the command prompt

```
sim [FBA]> add buy 127 5 Alice
✅ Queued order successfully in FBA discrete window buffer [ID: 1]
sim [FBA]> add sell 127 5 Bob
✅ Queued order successfully in FBA discrete window buffer [ID: 2]
sim [FBA]> clear
```

`add <buy|sell> <price> <qty> <user>` — price and quantity are typed as
plain whole numbers (the CLI doesn't parse decimals; internally the price
gets multiplied by `PRICE_SCALE` = 1e6 to become the engine's fixed-point
representation). `user` is any string, used as the participant id.

Other commands, same in both modes:

| Command | Effect |
|---|---|
| `batch` | Show the FBA pending-order buffer, or the CDA book state. |
| `clear` | Force the FBA engine to clear its current batch. (No-op warning in CDA mode — it already matches instantly.) |
| `log` | Print the full historical order/trade ledger. |
| `metrics` | Print the RQ2 metric time series computed so far, for **both** engines side by side. |
| `load <path> [path...]` | Replay real order-status data — see §4. |
| `help` / `exit` | Self-explanatory. |

---

## 4. Loading the real sample dataset

The `load` command (new — see §6) reads
`data/sample/order_statuses_accepted_PREVIEW.csv` /
`order_statuses_rejected_PREVIEW.csv` — one real hour of Hyperliquid SOL
order-flow, already decoded to plain CSV (see `../data/SCHEMA.md`, one
level above this repo). Both files share the same column layout; you can
load one or both:

```
sim [FBA]> load ../data/sample/order_statuses_accepted_PREVIEW.csv ../data/sample/order_statuses_rejected_PREVIEW.csv
📥 Loaded 600 order-status record(s) (149 live) into the FBA engine.
sim [FBA]> clear
sim [FBA]> metrics
```

What happens to each row: the loader builds a real `Order` preserving its
*actual* `status_id` (open / canceled / filled / rejected / ...) and
original timestamp/order id from the dataset — it does **not** assume
everything is a fresh `open` order the way `add` does. Every row is
recorded as an `OrderMessage` (so rejection-aware metrics like
order-to-trade ratio see the full picture), but only rows that pass
`Order::is_new_live_order()` (status `open`, non-trigger) actually enter
the batch/book. This mirrors exactly how a live replay would gate
messages — see `docs/ENGINE_DESIGN.md` §2.

To compare engines on the same data, reload the same file(s) after
switching mode:

```
sim [FBA]> engine continuous
sim [CDA]> load ../data/sample/order_statuses_accepted_PREVIEW.csv
sim [CDA]> metrics
```

### Known limitations

- **Quantities are rounded to the nearest whole SOL unit on load.** The
  engine's `Amount`/quantity type has no fixed-point convention the way
  price does (`PRICE_SCALE`) — only price is scaled internally. Real order
  sizes like `39.35` SOL become `39`. Extending quantity to fixed-point
  would need matching changes in `display.rs`/`report.rs`/`collector.rs`
  formatting — a reasonable follow-up, out of scope for getting sample
  data flowing.
- **Only the pre-decoded PREVIEW CSVs are supported**, not the raw
  `.gz`/binary hourly files described in `../data/SCHEMA.md`. To replay a
  different hour/day, regenerate a similar CSV first using the dataset's
  own `../data/read_data.py` (Python, requires NumPy/pandas).
- **No order-cancel/fill lifecycle replay.** The engines only ever accept
  brand-new order submissions — a `canceled`/`filled` row in the dataset is
  recorded for metrics purposes but there's no mechanism to "cancel" or
  otherwise mutate an order already sitting in the batch/book, since the
  engines don't support that operation at all today (not something this
  change added or removed).

---

## 5. Reading the metrics

`metrics` prints a compact summary table per engine (spread, trade count,
volume, surplus, fill rate, unexecuted %). The full ~25-column catalogue
(effective/realized spread, price impact, depth-within-bps,
order-to-trade ratio, clearing latency, ...) is available programmatically
via `metrics::report::to_csv(&series)` — not wired to a CLI command today,
but straightforward to call from your own code if you need a CSV export
for external analysis. See `metrics/src/interval.rs` for what every field
means and which research question (RQ2.1/2.2/2.3) it belongs to.

---

## 6. What changed recently

- Removed `engines/src/fba/lp_encoder.rs` and `engines/src/fba/solver.rs`:
  dead code — this engine trades a single pair, so the uniform clearing
  price has a closed-form solution (`BatchAuctionEngine::select_price`);
  no LP solver was ever called at runtime. See `ENGINE_DESIGN.md` §1.4.
- Removed the unused second matching implementation inside
  `engines/src/common/order_book.rs` (`submit`/`match_buy`/`match_sell`/
  etc.) — `ContinuousEngine` already reimplements matching inline and
  never called it. `OrderBookState` is now just the thin
  `{ pair, bids, asks }` holder it was actually used as.
- Added the `load` command and `simulation/src/loader.rs` (§4), plus a
  shared `ingest(order)` method on both `FbaSimulator` and `CdaSimulator`
  so hand-typed `add` orders and loaded historical orders go through the
  exact same metrics/matching path.

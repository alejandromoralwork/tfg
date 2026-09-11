# Metrics Reference — how every number in `output/<slug>/{fba,cda}_timeseries.csv` is calculated

> For the whole-project map see [`ARCHITECTURE.md`](ARCHITECTURE.md); for how records reach
> this pipeline in the first place see [`ARCHITECTURE.md` §4](ARCHITECTURE.md#simulate-pipeline)
> and [`REFERENCE.md`](REFERENCE.md#simulate_cmdrs). See [`README.md`](README.md) for the full
> documentation index. This file is the single source of truth for the *formula* behind every
> column — don't duplicate these formulas elsewhere; link here instead.

All of this lives in `src/metrics/timeseries.rs`. It is the **"push"** metrics path: the only
place in the crate that keeps an event log (`OrderMessage` / `TradeEvent` / `BatchClearedEvent`
/ `BookSnapshot`), fed exclusively by the `simulate` command. It is entirely separate from
`metrics/stats.rs`'s **"pull"** path (the interactive `metrics`/`orderbook` commands, which
just print an engine's own getters on demand — see `REFERENCE.md`'s `stats.rs` section).

Every metric here is checked against a hand-computed value by the runtime `test engine
metrics` checklist — see [`TESTING.md`](TESTING.md) for the full case-by-case catalogue.

---

## 1. How rows are built — the machinery every column depends on

- **One row per `(engine, interval)` bucket.** `bucket_of(ts, anchor, τ) = anchor +
  ((ts - anchor) / τ) * τ` — `anchor` is the timestamp of the very first record `simulate`
  sees (pinned once, shared identically by both the FBA and CDA recorder so their grids
  align). Buckets are **contiguous and empty-row-inclusive**: `compute_range` pre-populates one
  `IntervalMetrics::empty(...)` per grid slot in `[lo, hi)` before any accumulation, so a quiet
  period still produces rows with `None`/`0` fields rather than a gap.
- **Streamed, not one-shot.** `MetricsRecorder::emit(&messages, hi)` flushes every bucket whose
  end is `&lt; hi`; `finish` is `emit(.., max_seen_ts + 1)` (flush everything, end of run). A
  streaming run split into N incremental `emit` calls produces byte-identical CSV rows to one
  giant `finish` call at the end — enforced by `streaming_emit_in_pieces_matches_one_shot_finish`
  / `streaming_fba_batches_emit_in_pieces_matches_one_shot`.
- **Two metrics span bucket boundaries** via a `Carry` struct threaded in and out of every
  `emit`/`finish` call (and persisted in `checkpoint.txt` so a resumed run keeps them):
  `prev_close` (previous non-empty bucket's reference-price close, for `amihud_illiquidity`)
  and `prev_clearing` (previous *priced* FBA batch's clearing price, for FBA `kyle_lambda`;
  always `None` on a CDA recorder — CDA has no batch clearing price).
- **`None` vs `0.0` is meaningful and inconsistent by design** — read each column's own rule
  below. In general: "no qualifying observation this bucket" → `None` (rendered as an empty CSV
  field); "there were observations and the number really is zero" → `0.0`/`0`. One deliberate
  exception is noted at `intra_interval_price_dispersion`.
- **Scope tags used below**: **U**niversal (both engines) / **C**DA-only / **F**BA-only /
  **X** (always empty — needs data this dataset doesn't have).

---

## 2. Column-by-column

### `engine`, `interval_start_ns`, `interval_width_ns` — U (row key)
`engine = EngineKind::label()` ("FBA"/"CDA"); `interval_start` = the bucket's start (ns since
epoch); `interval_width` = `τ` in ns, the `simulate <path> [interval_secs]` argument (default
1s) converted to ns.

### `quoted_spread_bps` — U
- **CDA**: for every `BookSnapshot` in the bucket with `best_bid`/`best_ask` both `Some` and
  `bid+ask &gt; 0`: `mid = (bid+ask)/2.0`, `spread_bps = (ask-bid)/mid * 1e4`. Field = mean over
  qualifying snapshots.
- **FBA**: for every `BatchClearedEvent` with `best_unfilled_buy`, `best_unfilled_sell`, and
  `clearing_price &gt; 0` all present: `(sell-buy)/clearing_price * 1e4`. Field = mean over
  qualifying batches.
- A snapshot/batch that fails its own gate (e.g. one-sided book, zero clearing price) is
  **excluded** from the mean, not counted as 0. `None` if nothing in the bucket qualified.

### `depth_at_best` — U
- **CDA**: mean over snapshots of `(best_bid_qty + best_ask_qty) / 2.0` — **touch only**: the
  single best order's remaining size on each side. Deliberately not the whole-book
  `bid_depth`/`ask_depth` sum (that's `total_book_depth` below) — an earlier version of this
  code used the whole-book figures here, which made `depth_at_best` come out ~10× *larger*
  than `depth_within_10bps`, backwards from what the name promises. Fixed; documented here so
  it isn't re-broken.
- **FBA**: mean over batches of `(demand_at_price + supply_at_price) / 2.0`.

### `depth_within_10bps` / `_50bps` / `_100bps` — U
Per threshold index (10/50/100 bps): mean over snapshots (CDA) or batches (FBA) of
`(schedule[i].bid + schedule[i].ask) / 2.0`, where `schedule` comes from
`depth_schedule(reference_price, levels)`: for every resting `(price, side, qty)`,
`bps_away = |price - reference| * 1e4 / reference` (0 if `reference == 0`), and `qty`
accumulates into every threshold `bps_away &lt;= threshold` — **cumulative**, so a level within
10bps also counts toward the 50bps and 100bps buckets. `None` if the bucket has no
snapshots/batches at all.

### `book_imbalance` — C
For each snapshot with `best_bid_qty + best_ask_qty &gt; 0`: `(bid_qty - ask_qty) / total`.
Field = mean over qualifying snapshots; zero-touch-depth snapshots excluded. Always `None` on
an FBA row — no code path ever sets it for that engine.

### `total_book_depth` — C
Mean over snapshots of `(bid_depth + ask_depth) / 2.0` — the **whole book**, every price level
both sides (contrast with `depth_at_best`'s touch-only figure).

### `effective_spread_bps` — U
For every trade with `reference_price = Some(m)`, `m &gt; 0` (CDA: pre-trade midpoint; FBA:
`last_clearing_price` at submission):

```
deviation_bps(price, m, aggressor_side) =
    Buy  =&gt;  2·(price - m)/m · 1e4
    Sell =&gt; -2·(price - m)/m · 1e4
    None =&gt;  2·|price - m|/m · 1e4      (FBA — no taker/maker in a uniform-price batch)
```

Accumulated quantity-weighted per bucket: `Σ(dev_bps·qty) / Σ(qty)`. `None` if no trade in the
bucket had a usable reference price.

### `realized_spread_bps_1s` / `_5s` / `_30s` — U
Same trades, same gate, but compared to the midpoint/clearing price *after* the trade instead
of before. For each horizon `Δ ∈ {1,5,30}` seconds, look up `future_price =
price_at_or_after(trade.ts + Δ·1e9)` (binary search over a combined, ts-sorted series of CDA
book midpoints and FBA clearing prices — book snapshots win ties against a batch at the exact
same ts):

```
realized_bps =
    Buy  =&gt;  2·(price - future)/m · 1e4
    Sell =&gt; -2·(price - future)/m · 1e4
    None =&gt;  2·|price - future|/m · 1e4
```

Quantity-weighted mean per horizon per bucket; `None` for a horizon if no trade in the bucket
found a future price within that lookahead (e.g. it falls past the retained window, or the run
ends before it). This is why `simulate` waits `MARKOUT_GUARD_SECS = 35s` (30s + 5s + margin)
past a bucket's own end and the next file's first timestamp before flushing it — every trade's
≤30s markout should already be observable data by then.

### `price_impact_bps_1s` / `_5s` / `_30s` — U
`effective_spread_bps(bucket) - realized_spread_bps_Δ(bucket)` — the adverse-selection
component (the part the effective spread over-counts if the price kept moving your way).
Assigned only if both terms are `Some` for that bucket/horizon.

### `amihud_illiquidity` — U, cross-flush via `Carry.prev_close`
Reference-price series = CDA book midpoints + FBA batch clearing prices, sorted by ts.
`close[bucket]` = the last reference price observed in that bucket. Walking buckets in
ascending order, carrying `prev_close` forward across **empty** buckets (they don't reset it):
if the current bucket has a close and `prev_close = Some(prev)` with `prev &gt; 0`:

```
ret       = (close - prev) / prev
dollar_vol = executed_notional / PRICE_SCALE          -- back out of fixed-point
amihud     = |ret| / dollar_vol * 1e6      (if dollar_vol &gt; 0, else None)
```

This is the canonical Amihud (2002) illiquidity ratio, ×1e6 per the standard convention.
`None` if there's no prior close yet (first priced bucket of the run), or if the bucket traded
zero volume — note a bucket with a real price move but **no trades** reports `None`, not
`Infinity`.

### `kyle_lambda` — U, but built completely differently per engine (bps per SOL, **not**
### PRICE_SCALE-denominated)
Per interval, the OLS slope through the origin, `λ = Σ(x·y) / Σ(x·x)` — two running sums,
`O(1)` per observation, nothing retained.

- **CDA** — one observation per **sweep**: same-`ts` trades (which share side and pre-trade
  mid) are grouped via a `BTreeMap&lt;ts, (net_signed_qty, pre_mid)&gt;` (must be ordered by ts —
  verified by the streaming-vs-one-shot differential test). Only trades with a usable
  `reference_price` and a known `aggressor_side` count (so FBA-shaped trades, which never set
  `aggressor_side`, are skipped here). `x = net signed executed qty` (+buy/−sell across the
  sweep); `y = (post_mid − pre_mid)/pre_mid · 1e4` where `post_mid = price_at_or_after(ts +
  5s)` — a fixed 5s horizon, independent of the three realized-spread horizons above. A
  net-zero sweep (`x=0`) is inert. On real data this comes out **positive-skewed** (buy sweeps
  lift the mid).
- **FBA** — one observation per **priced batch**, walked in strict ascending order (a
  `debug_assert!`ed invariant). `x = net_order_flow` = Σ(submitted buy qty) − Σ(submitted sell
  qty) for the whole batch, captured **before** price selection
  (`BatchClearedEvent.net_order_flow`); `y = (cp_k − cp_{k-1})/cp_{k-1} · 1e4}` using
  `Carry.prev_clearing` as `cp_{k-1}` (carried across flush boundaries, so the first in-range
  batch of a resumed/incremental flush still has a predecessor). A batch with `clearing_price:
  None` is skipped and does **not** advance the carry. On real 1-second data this comes out
  **near zero** — a genuine finding (the volume-max + anchor-to-last-clear rule makes FBA's
  clearing price nearly impervious to flow imbalance), not a defect.
- `None` if `Σ(x·x) == 0` for that bucket (no usable observations).

### `realized_volatility` — U
From the same reference-price series, grouped per bucket. With `≥2` points in a bucket:
consecutive-pair returns `r = (p[i+1]-p[i])/p[i]` (skipping a pair whose base is `≤0`); field =
`sqrt(Σ r²)` — the **root sum of squares** (the Andersen/Bollerslev realized-variance
estimator), explicitly **not** a stddev about the mean. A single return in the bucket yields
`|r|` directly. `None` if the bucket has `&lt;2` points, or all pairs were skipped.

### `intra_interval_price_dispersion` — U
From bucketed trade prices: `m = mean(prices)`; `disp = stddev(prices)/m * 1e4` (population
stddev, `/n` not `/(n-1)`). **The one column that defaults to `0.0` rather than `None`** when
`m ≤ 0` but the bucket has trades at all — it is `Some(0.0)` unconditionally whenever there was
at least one trade. `None` only if the bucket had zero trades. ~0 for FBA by construction (one
clearing price per batch).

### `pricing_error_bps` — X
Always `None`. No code path ever assigns it — needs an external oracle/mark-price feed this
dataset doesn't provide. Kept as a visible, documented gap rather than silently dropped.

### `executed_volume`, `executed_notional` — U
Plain running sums over the main per-trade loop: `executed_volume += qty`,
`executed_notional += qty * price` (raw `PRICE_SCALE` units, summed before any division).

### `vwap` — U
Final pass per bucket: `executed_notional / executed_volume` if `executed_volume &gt; 0`, else
`None`.

### `trade_count` — U
Plain `u64` count (not `Option`), incremented once per trade.

### `fill_rate` — U
Bucketed by **each order's own submission bucket** (`bucket_of(first_seen_ts)`), not the
trade's bucket. Per order-state (built fresh per `compute_range` call from `messages` + the
retained trades): `filled_qty` accumulates from every trade that names its `oid` as
`buy_order_id`/`sell_order_id`. Per bucket: `Σ filled_qty / Σ orig_qty` over orders first seen
in that bucket; `None` if `Σ orig_qty == 0`.

### `avg_time_to_execution_secs` — U
For every order with a `first_fill_ts`, bucketed by `first_seen_ts`:
`(first_fill_ts - first_seen_ts) / 1e9`. Field = mean per bucket; `None` if no order in the
bucket ever got a first fill.

### `trader_surplus` — U (plain `f64`, not `Option`)
Accumulated per trade, per side independently: buy side (if its limit price is known)
`+= (limit - price).max(0.0) * qty`; sell side `+= (price - limit).max(0.0) * qty`. Clipped at
0 so an adverse fill contributes nothing (never goes negative) — realized price-improvement vs.
each side's own limit.

### `order_size_inflation` — U
Bucketed by `first_seen_ts`, aggregated per `(bucket, user_id)` as `(Σorig, Σfilled)`. A user
contributes `orig/filled` to the bucket's mean **only if both `orig &gt; 0` and `filled &gt; 0`** —
an order that never filled at all is deliberately excluded (including it via a `max(1.0)`
denominator used to blow this metric up into the hundreds, dominated by never-traded orders
rather than genuinely oversized ones). `None` if no user in the bucket had a nonzero ratio.

### `order_to_trade_ratio` — U
`bucket_msg_count / trade_count` for the bucket, `None` if `trade_count == 0`. The numerator
counts **every** message (accepted and rejected — `OrderMessage` is recorded before any
engine gating), so this ratio's numerator and `fill_rate`'s populations don't line up 1:1.

### `boundary_concentration` — F
Share of order arrivals landing in the final 10% of each FBA batch's own window. Per
`BatchClearedEvent`: `window = ts - batch_open_ts`; a zero-width window is skipped entirely
(contributes nothing, not a 0). `boundary_start = ts - window/10` (integer division, so the
"final 10%" is `[boundary_start, ts]` inclusive both ends). Using a pre-sorted copy of every
message timestamp (sorted once — the raw stream isn't assumed monotonic, since accepted/
rejected file interleaving can break strict ts order): `total_msgs` = count in `[batch_open_ts,
ts]`, `boundary_msgs` = count in `[boundary_start, ts]`, both inclusive. Numerator and
denominator **accumulate across every batch closing in the bucket** before dividing:
`Σboundary_msgs / Σtotal_msgs`. `None` if `Σtotal_msgs == 0`.

### `throughput_orders_per_sec` — U, **wall-clock, non-deterministic**
`bucket_msg_count / Σ(compute_time)` where `compute_time` sums every `BookSnapshot`'s (CDA) or
`BatchClearedEvent`'s (FBA) measured engine time in the bucket. `None` if total time is 0 or
the bucket has no message-count entry. **Caveat, load-bearing for interpretation**: the
numerator counts *every* message including rejections; the denominator only sums measured
`submit`/`clear` compute time — the two populations don't match, so this is a rough gauge, not
a precise rate, and it varies run-to-run/machine-to-machine.

### `avg_clearing_latency_micros` — U, **wall-clock, non-deterministic**
Same accumulators: `Σ(compute_time).as_micros() / count`. **Per-`submit` for CDA** (one
`BookSnapshot`, hence one timing, per actionable record) but **per-`clear` for FBA** (one
`BatchClearedEvent` per batch) — the same column name measures two different granularities of
operation depending on engine.

### `unexecuted_residual_share` — F
Per bucket, summed across every batch in it: `Σ max(demand_at_price, supply_at_price)` as the
denominator, `Σ unexecuted_quantity` (`|demand−supply|` from that batch's clear, **not** a sum
over residual orders — see `REFERENCE.md`'s `fba.rs` section for why that distinction matters)
as the numerator. `None` if the denominator is 0.

---

## 3. Known, documented gaps (not bugs)

- **`pricing_error_bps`** is always empty — no oracle/mark-price feed in the dataset.
- **`throughput_orders_per_sec` / `avg_clearing_latency_micros`** are wall-clock and
  non-deterministic between runs/machines, and each has a numerator/denominator or
  per-engine-granularity mismatch as described above.
- **`price_impact_bps_Δ = effective_spread_bps − realized_spread_bps_Δ`** combines two
  separately quantity-weighted trade populations; they can differ for trades near the end of
  the retained window. Harmless in a real run — the 35s `MARKOUT_GUARD_SECS` flush guarantees
  every flushed bucket's ≤30s markouts already exist by the time it's computed.
- **`kyle_lambda`**'s CDA 5s horizon and sweep-grouping, and its differing CDA/FBA
  constructions, are deliberate methodological choices, not defects — see the "worth knowing"
  notes above. The near-zero FBA result is itself a research finding (the auction's
  price-selection rule suppresses flow-driven price impact), not noise.
- **Not built**: "implementation shortfall of standardized probe orders" and "simulated
  latency arbitrageur" metrics mentioned in the exposé's RQ2.1/RQ2.3 have no column here.

Every formula above is checked against a hand-computed value by `test engine metrics` — see
[`TESTING.md`](TESTING.md#checklist-cases) for the exact scenarios
and expected numbers.

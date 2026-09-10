//! Time-series metric catalogue for the `simulate` command — the full
//! RQ2.1/2.2/2.3 + engine-performance list from `docs/expose.tex`,
//! computed per time bucket over the duration of a replay.
//!
//! This is deliberately a SEPARATE, parallel pipeline from the on-demand
//! metric methods on `FbaOrderBook`/`CdaOrderBook` (`quoted_spread_bps()`
//! etc., used by the interactive `metrics`/`orderbook` commands) — those
//! stay exactly as they are, answering "what does the book look like right
//! now". This module answers "what happened over time", which needs an
//! actual event log (`MetricsRecorder` records `OrderMessage`/`TradeEvent`/
//! `BatchClearedEvent`/`BookSnapshot` as `simulate` streams through a
//! replay) rather than a single current-state snapshot.
//!
//! Ported from the earlier `thesis-market-models` workspace's
//! `metrics::collector`/`events`/`interval`/`report` — same algorithm,
//! adapted to this crate's `Order`/`Trade`/`EngineKind` (no `pair` field)
//! and folded into one file rather than four, matching how this crate
//! already consolidates related concerns (e.g. `inputs/cli.rs`).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use crate::types::{EngineKind, Side, Trade, PRICE_SCALE};

/// Basis-point offsets the depth-within-x-bps metric is reported at.
pub const DEPTH_BPS_THRESHOLDS: [u32; 3] = [10, 50, 100];

const NS_PER_SEC: u64 = 1_000_000_000;
const REALIZED_SPREAD_HORIZONS_SECS: [u64; 3] = [1, 5, 30];

/// Forward markout horizon for the CDA `kyle_lambda` regression's post-trade
/// mid. Independent of `REALIZED_SPREAD_HORIZONS_SECS` — changing it changes
/// the metric. Must stay well under `simulate`'s settle window so a bucket
/// never flushes before its post-trade mids have been observed.
const KYLE_LAMBDA_HORIZON_SECS: u64 = 5;

// ============================================================================
// Events — plain data `simulate` constructs from an engine's public output
// ============================================================================

/// An order-flow message observed BEFORE any engine-level accept/reject
/// gating (`Order::is_new_live_order`) — recorded unconditionally so
/// metrics like order-to-trade ratio see the full picture, not just what
/// an engine chose to accept.
#[derive(Debug, Clone)]
pub struct OrderMessage {
    pub ts: u64,
    pub oid: u64,
    pub user_id: String,
    pub side: Side,
    pub limit_price: Option<u128>,
    pub quantity: u128,
    pub accepted: bool,
}

/// A trade, plus the two pieces of context needed for effective/realized
/// spread that aren't part of `Trade` itself (deliberately — they're
/// metrics-analysis concepts, not matching-engine ones):
/// - `reference_price`: pre-trade midpoint (CDA) or `last_clearing_price`
///   at submission time (FBA) — what the trade is measured against.
/// - `aggressor_side`: the taker's side (CDA); `None` for FBA, which has
///   no taker/maker distinction in a uniform-price batch.
#[derive(Debug, Clone)]
pub struct TradeEvent {
    pub trade: Trade,
    pub reference_price: Option<u128>,
    pub aggressor_side: Option<Side>,
}

/// Emitted once per FBA batch clearing attempt, whether or not it matched
/// any trades.
#[derive(Debug, Clone)]
pub struct BatchClearedEvent {
    pub ts: u64,
    pub batch_open_ts: u64,
    pub clearing_price: Option<u128>,
    pub demand_at_price: u128,
    pub supply_at_price: u128,
    /// Net signed order flow submitted into this batch, BEFORE price
    /// selection: `Σ(buy remaining) − Σ(sell remaining)` over every order in
    /// the batch (market orders included, rolled-over residual included),
    /// signed SOL. This is the auction-round analogue of Kyle's order-flow
    /// regressor `x` — a strong directional signal, unlike the
    /// `demand_at_price − supply_at_price` residual which `select_price`
    /// deliberately minimizes.
    pub net_order_flow: f64,
    pub traded_quantity: u128,
    pub unexecuted_quantity: u128,
    pub best_unfilled_buy: Option<u128>,
    pub best_unfilled_sell: Option<u128>,
    pub depth_schedule: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    pub compute_time: Duration,
}

/// A snapshot of the CDA book, taken after processing a single order.
#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub ts: u64,
    pub best_bid: Option<u128>,
    pub best_ask: Option<u128>,
    /// Remaining size of the single best-priced resting bid/ask (0 if none
    /// resting on that side) — genuinely "at the touch", used for
    /// `depth_at_best`/`book_imbalance`. Deliberately a separate field from
    /// `bid_depth`/`ask_depth` below rather than reusing them: those sum
    /// the WHOLE book, and conflating the two was a real bug (see
    /// `total_book_depth`'s doc comment on `IntervalMetrics`).
    pub best_bid_qty: u128,
    pub best_ask_qty: u128,
    /// Total remaining size across ALL resting bids/asks, every price
    /// level — used only for `total_book_depth`, not `depth_at_best`.
    pub bid_depth: u128,
    pub ask_depth: u128,
    pub depth_schedule: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    pub compute_time: Duration,
}

/// Cumulative resting/schedule volume within each of `DEPTH_BPS_THRESHOLDS`
/// basis points of `reference`, split by side. `levels` is any iterator of
/// (price, side, quantity) — a CDA book's bids/asks, or an FBA batch's raw
/// order list.
pub fn depth_schedule(reference: u128, levels: impl Iterator<Item = (u128, Side, u128)>) -> [(u128, u128); DEPTH_BPS_THRESHOLDS.len()] {
    let mut schedule = [(0u128, 0u128); DEPTH_BPS_THRESHOLDS.len()];
    if reference == 0 {
        return schedule;
    }
    for (price, side, qty) in levels {
        let diff = if price > reference { price - reference } else { reference - price };
        let bps = diff.saturating_mul(10_000) / reference;
        for (i, threshold) in DEPTH_BPS_THRESHOLDS.iter().enumerate() {
            if bps <= *threshold as u128 {
                match side {
                    Side::Buy => schedule[i].0 += qty,
                    Side::Sell => schedule[i].1 += qty,
                }
            }
        }
    }
    schedule
}

// ============================================================================
// IntervalMetrics — one row per time bucket
// ============================================================================

/// One time-bucketed row of the metric catalogue. Fields are `Option<f64>`
/// wherever the metric can genuinely be undefined for a bucket (no trades
/// occurred, or an external input isn't available) — `None` always means
/// "not computable from what was recorded," never a silent zero.
#[derive(Debug, Clone)]
pub struct IntervalMetrics {
    pub engine: &'static str,
    pub interval_start: u64,
    pub interval_width: u64,

    // ---- Liquidity and transaction cost ----
    pub quoted_spread_bps: Option<f64>,
    /// Volume "at the touch" specifically: for CDA, the single best bid's
    /// remaining size plus the single best ask's remaining size (averaged
    /// across snapshots in the bucket) — NOT total resting book depth (see
    /// `total_book_depth` for that). For FBA, volume at the clearing price
    /// (`demand_at_price`/`supply_at_price`), which was already narrow —
    /// no change there.
    pub depth_at_best: Option<f64>,
    pub depth_within_bps: [Option<f64>; DEPTH_BPS_THRESHOLDS.len()],
    /// CDA-only — no resting book to measure imbalance on for FBA. Same
    /// top-of-book basis as `depth_at_best`, not the whole book.
    pub book_imbalance: Option<f64>,
    /// CDA-only: mean total resting depth (all price levels, both sides)
    /// across snapshots in the bucket. This is what `depth_at_best` used
    /// to (incorrectly) compute — kept under its own honest name rather
    /// than discarded, since "how much liquidity is resting in total" is
    /// still a useful number, just a different one from "depth at best".
    pub total_book_depth: Option<f64>,
    pub effective_spread_bps: Option<f64>,
    pub realized_spread_bps_1s: Option<f64>,
    pub realized_spread_bps_5s: Option<f64>,
    pub realized_spread_bps_30s: Option<f64>,
    pub price_impact_bps_1s: Option<f64>,
    pub price_impact_bps_5s: Option<f64>,
    pub price_impact_bps_30s: Option<f64>,
    pub amihud_illiquidity: Option<f64>,
    /// Kyle's lambda: OLS slope through the origin of relative mid-price
    /// change (basis points) on signed order flow (whole SOL) within the
    /// bucket — `Σ(x·y) / Σ(x·x)`. A price-impact coefficient in
    /// **bps per SOL**, NOT a PRICE_SCALE (1e6) fixed-point quantity — do
    /// not divide it by 1e6 when reading the CSV. `None` when the bucket has
    /// no usable observation (`Σ(x·x) == 0`). Built differently per engine
    /// (CDA: one observation per taker sweep, mid measured
    /// `KYLE_LAMBDA_HORIZON_SECS` later; FBA: one per batch, signed excess
    /// demand vs the one-batch clearing-price move), so the two are NOT
    /// directly comparable across engines — same as `quoted_spread_bps`.
    pub kyle_lambda: Option<f64>,

    // ---- Price discovery and market quality ----
    pub realized_volatility: Option<f64>,
    pub intra_interval_price_dispersion: Option<f64>,
    /// Always `None`: needs an external oracle/mark-price feed this
    /// dataset doesn't include. Kept as a real column (not omitted) so
    /// it's a visible, documented gap rather than a silently missing one.
    pub pricing_error_bps: Option<f64>,

    // ---- Execution, allocation, and engine performance ----
    pub executed_volume: f64,
    pub executed_notional: f64,
    /// Volume-weighted average execution price for the interval
    /// (`executed_notional / executed_volume`) — same PRICE_SCALE
    /// convention as every other price-denominated field.
    pub vwap: Option<f64>,
    pub trade_count: u64,
    pub fill_rate: Option<f64>,
    pub avg_time_to_execution_secs: Option<f64>,
    pub trader_surplus: f64,
    pub order_size_inflation: Option<f64>,
    pub order_to_trade_ratio: Option<f64>,
    /// FBA-only — no batch window for a CDA to speak of.
    pub boundary_concentration: Option<f64>,
    pub throughput_orders_per_sec: Option<f64>,
    pub avg_clearing_latency_micros: Option<f64>,
    /// FBA-only.
    pub unexecuted_residual_share: Option<f64>,
}

impl IntervalMetrics {
    // `pub(crate)`, not private: `inputs::simulate_cmd`'s tests use this as
    // a convenient way to build a mostly-empty IntervalMetrics with just
    // the one or two fields a given test cares about set.
    pub(crate) fn empty(engine: &'static str, interval_start: u64, interval_width: u64) -> Self {
        Self {
            engine,
            interval_start,
            interval_width,
            quoted_spread_bps: None,
            depth_at_best: None,
            depth_within_bps: [None; DEPTH_BPS_THRESHOLDS.len()],
            book_imbalance: None,
            total_book_depth: None,
            effective_spread_bps: None,
            realized_spread_bps_1s: None,
            realized_spread_bps_5s: None,
            realized_spread_bps_30s: None,
            price_impact_bps_1s: None,
            price_impact_bps_5s: None,
            price_impact_bps_30s: None,
            amihud_illiquidity: None,
            kyle_lambda: None,
            realized_volatility: None,
            intra_interval_price_dispersion: None,
            pricing_error_bps: None,
            executed_volume: 0.0,
            executed_notional: 0.0,
            vwap: None,
            trade_count: 0,
            fill_rate: None,
            avg_time_to_execution_secs: None,
            trader_surplus: 0.0,
            order_size_inflation: None,
            order_to_trade_ratio: None,
            boundary_concentration: None,
            throughput_orders_per_sec: None,
            avg_clearing_latency_micros: None,
            unexecuted_residual_share: None,
        }
    }
}

// ============================================================================
// MetricsRecorder — records events during a replay, then buckets them
// ============================================================================

/// Per-order fill-tracking state, built once from the full message+trade
/// history at `finalize()` time (not incrementally — fill rate etc. need
/// an order's *eventual* outcome, only known once the whole run is in).
struct OrderState {
    user_id: String,
    limit_price: Option<u128>,
    orig_qty: u128,
    filled_qty: u128,
    first_seen_ts: u64,
    first_fill_ts: Option<u64>,
}

/// Where `bucket_of` places `ts` on the metric grid: the start of the
/// `interval_width`-wide bucket it falls in, all anchored at `anchor` (the
/// first timestamp a recorder ever saw). A free function rather than a
/// closure so `MetricsRecorder`'s streaming methods — and `simulate_cmd`,
/// which prunes its shared `messages` slice on the same grid — can share it.
pub(crate) fn bucket_of(ts: u64, anchor: u64, interval_width: u64) -> u64 {
    let offset = ts.saturating_sub(anchor);
    anchor + (offset / interval_width) * interval_width
}

/// Cross-flush "prior value + delta" state threaded through `compute_range`:
/// metrics whose per-bucket value depends on an earlier, already-emitted
/// bucket. `pub(crate)` so `inputs::simulate_cmd` can build one from the
/// checkpoint and read it back after a flush.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Carry {
    /// `amihud_illiquidity`: the last non-empty in-range bucket's
    /// reference-price close.
    pub prev_close: Option<f64>,
    /// FBA `kyle_lambda`: the previous priced batch's clearing price
    /// (PRICE_SCALE fixed-point, as `f64`) — the `p_{k-1}` the current
    /// batch's price move is measured against. Always `None` for a CDA
    /// recorder.
    pub prev_clearing: Option<f64>,
}

pub struct MetricsRecorder {
    engine: EngineKind,
    interval_width: u64,
    /// Bucket-grid origin: the first timestamp this recorder ever saw. Every
    /// bucket boundary is `anchor + k * interval_width`. Set once and then
    /// frozen (and restored verbatim on a resumed run) so the grid is
    /// stable for the whole replay. `None` until the first event/message.
    anchor: Option<u64>,
    /// Exclusive upper bucket boundary already emitted to CSV — the next
    /// `emit`/`finish` picks up here. Equals `anchor` until the first flush.
    emitted_upto: u64,
    /// Cross-flush carries (`amihud_illiquidity`, FBA `kyle_lambda`) so those
    /// "prior value + delta" metrics span flush boundaries exactly as they
    /// did in the old one-shot `finalize`.
    carry: Carry,
    trades: Vec<TradeEvent>,
    batches: Vec<BatchClearedEvent>,
    books: Vec<BookSnapshot>,
    /// Events whose bucket had already been emitted when they arrived —
    /// only possible if event-time jumps backwards by more than `simulate`'s
    /// settle window. Counted (and surfaced in the summary) rather than
    /// stored; normally 0.
    late_events_dropped: u64,
}

impl MetricsRecorder {
    /// `interval_width_ns` is the bucket width in nanoseconds — use the
    /// same value for the FBA and CDA recorders in one `simulate` run so
    /// the two resulting time series sit on the same time grid.
    pub fn new(engine: EngineKind, interval_width_ns: u64) -> Self {
        Self {
            engine,
            interval_width: interval_width_ns,
            anchor: None,
            emitted_upto: 0,
            carry: Carry::default(),
            trades: Vec::new(),
            batches: Vec::new(),
            books: Vec::new(),
            late_events_dropped: 0,
        }
    }

    /// Rebuild a recorder mid-replay from a checkpoint. Engine books and the
    /// in-flight event window are deliberately NOT restored (see the
    /// `simulate` resume notes in `inputs::simulate_cmd`) — only the bucket
    /// grid, the emit cursor, and the cross-flush `amihud` and Kyle's-lambda
    /// carries, so rows appended after resume line up with the ones already
    /// on disk.
    pub fn resume(engine: EngineKind, interval_width_ns: u64, anchor: u64, emitted_upto: u64, carry: Carry, late_events_dropped: u64) -> Self {
        Self {
            engine,
            interval_width: interval_width_ns,
            anchor: Some(anchor),
            emitted_upto,
            carry,
            trades: Vec::new(),
            batches: Vec::new(),
            books: Vec::new(),
            late_events_dropped,
        }
    }

    pub fn anchor(&self) -> Option<u64> {
        self.anchor
    }
    pub fn emitted_upto(&self) -> u64 {
        self.emitted_upto
    }
    /// The cross-flush carry state, to persist in the checkpoint after a flush.
    pub(crate) fn carry(&self) -> Carry {
        self.carry
    }
    pub fn late_events_dropped(&self) -> u64 {
        self.late_events_dropped
    }

    fn observe_ts(&mut self, ts: u64) {
        if self.anchor.is_none() {
            self.anchor = Some(ts);
            self.emitted_upto = ts;
        }
    }

    /// Pin the bucket-grid origin explicitly, before any event is recorded.
    /// `simulate` calls this with the first record's timestamp on BOTH the
    /// FBA and CDA recorders so their series land on the exact same grid —
    /// otherwise each would anchor on the first event it happens to see
    /// (a book snapshot for CDA on record 1, but only the first batch
    /// *clear* for FBA, ~one interval later). No-op once set.
    pub fn set_anchor(&mut self, ts: u64) {
        self.observe_ts(ts);
    }

    /// True once `ts`'s bucket has already been flushed — such an event can
    /// no longer change any emitted row, so it's counted, not kept.
    fn is_late(&self, ts: u64) -> bool {
        match self.anchor {
            Some(a) => bucket_of(ts, a, self.interval_width) < self.emitted_upto,
            None => false,
        }
    }

    pub fn record_trade(&mut self, trade: TradeEvent) {
        self.observe_ts(trade.trade.ts);
        if self.is_late(trade.trade.ts) {
            self.late_events_dropped += 1;
            return;
        }
        self.trades.push(trade);
    }

    pub fn record_batch(&mut self, batch: BatchClearedEvent) {
        self.observe_ts(batch.ts);
        if self.is_late(batch.ts) {
            self.late_events_dropped += 1;
            return;
        }
        self.batches.push(batch);
    }

    pub fn record_book_snapshot(&mut self, snapshot: BookSnapshot) {
        self.observe_ts(snapshot.ts);
        if self.is_late(snapshot.ts) {
            self.late_events_dropped += 1;
            return;
        }
        self.books.push(snapshot);
    }

    /// Emit every bucket-grid row with start in `[emitted_upto, hi)`, in
    /// ascending order, from the events and `messages` currently retained;
    /// buckets with no activity are emitted as empty rows so the grid stays
    /// contiguous. Advances `emitted_upto` and the `amihud` carry. Forward
    /// look-ups (realized-spread markouts) read past `hi` into still-retained
    /// events — the caller guarantees data out to `hi + markout horizon`
    /// before calling with a given `hi`.
    pub fn emit(&mut self, messages: &[OrderMessage], hi: u64) -> Vec<IntervalMetrics> {
        if self.anchor.is_none() {
            // No event was recorded directly (some unit-test paths, and any
            // recorder that only ever saw messages) — anchor off the
            // earliest timestamp visible, matching the old `finalize`'s
            // global `min_ts`.
            let mut min_ts = u64::MAX;
            for m in messages {
                min_ts = min_ts.min(m.ts);
            }
            for t in &self.trades {
                min_ts = min_ts.min(t.trade.ts);
            }
            for b in &self.batches {
                min_ts = min_ts.min(b.ts);
            }
            for s in &self.books {
                min_ts = min_ts.min(s.ts);
            }
            if min_ts == u64::MAX {
                return Vec::new();
            }
            self.anchor = Some(min_ts);
            self.emitted_upto = min_ts;
        }
        let anchor = self.anchor.expect("set above");
        if self.interval_width == 0 || hi <= self.emitted_upto {
            return Vec::new();
        }
        let lo = self.emitted_upto;
        let (rows, carry_out, next_upto) = self.compute_range(messages, lo, hi, anchor, self.carry);
        self.carry = carry_out;
        self.emitted_upto = next_upto;
        rows
    }

    /// Flush everything still buffered: the tail buckets up to and including
    /// the one holding `max_seen_ts`. Call once at end of replay.
    pub fn finish(&mut self, messages: &[OrderMessage], max_seen_ts: u64) -> Vec<IntervalMetrics> {
        self.emit(messages, max_seen_ts.saturating_add(1))
    }

    /// Convenience one-shot flush over the whole retained set — computes the
    /// end timestamp itself. Test-only: the `simulate` path streams
    /// file-by-file through `emit`/`finish`.
    #[cfg(test)]
    pub fn finalize(&mut self, messages: &[OrderMessage]) -> Vec<IntervalMetrics> {
        let mut max_ts = 0u64;
        for m in messages {
            max_ts = max_ts.max(m.ts);
        }
        for t in &self.trades {
            max_ts = max_ts.max(t.trade.ts);
        }
        for b in &self.batches {
            max_ts = max_ts.max(b.ts);
        }
        for s in &self.books {
            max_ts = max_ts.max(s.ts);
        }
        self.finish(messages, max_ts)
    }

    /// Drop events whose bucket has now been emitted. The caller prunes its
    /// own shared `messages` slice to the same `emitted_upto` boundary.
    pub fn prune(&mut self) {
        let (Some(anchor), w) = (self.anchor, self.interval_width) else {
            return;
        };
        let cutoff = self.emitted_upto;
        self.trades.retain(|t| bucket_of(t.trade.ts, anchor, w) >= cutoff);
        self.batches.retain(|b| bucket_of(b.ts, anchor, w) >= cutoff);
        self.books.retain(|s| bucket_of(s.ts, anchor, w) >= cutoff);
    }

    fn build_order_states(&self, messages: &[OrderMessage]) -> HashMap<u64, OrderState> {
        let mut states: HashMap<u64, OrderState> = HashMap::new();

        for m in messages {
            if !m.accepted {
                continue;
            }
            states.entry(m.oid).or_insert_with(|| OrderState {
                user_id: m.user_id.clone(),
                limit_price: m.limit_price,
                orig_qty: m.quantity,
                filled_qty: 0,
                first_seen_ts: m.ts,
                first_fill_ts: None,
            });
        }

        for t in &self.trades {
            let trade = &t.trade;
            for oid in [trade.buy_order_id, trade.sell_order_id] {
                if let Some(state) = states.get_mut(&oid) {
                    state.filled_qty = state.filled_qty.saturating_add(trade.quantity);
                    state.first_fill_ts = Some(match state.first_fill_ts {
                        Some(existing) => existing.min(trade.ts),
                        None => trade.ts,
                    });
                }
            }
        }

        states
    }

    /// Compute the metric rows for every bucket with start in `[lo, hi)`,
    /// anchored at `anchor`, from the currently-retained events plus
    /// `messages`. Forward look-ups (realized-spread markouts, Kyle's-lambda
    /// post-trade mids) still read events past `hi`, which is why the
    /// streaming caller keeps a settle window before advancing `hi`. Returns
    /// the rows, the updated cross-flush `Carry`, and the next exclusive
    /// bucket boundary (`>= hi`) to resume emitting from.
    fn compute_range(&self, messages: &[OrderMessage], lo: u64, hi: u64, anchor: u64, carry_in: Carry) -> (Vec<IntervalMetrics>, Carry, u64) {
        let w = self.interval_width;
        let bof = |ts: u64| bucket_of(ts, anchor, w);
        let in_range = |b: u64| b >= lo && b < hi;

        let mut buckets: BTreeMap<u64, IntervalMetrics> = BTreeMap::new();
        let mut next_upto = lo;
        while next_upto < hi {
            buckets.insert(next_upto, IntervalMetrics::empty(self.engine.label(), next_upto, w));
            next_upto = next_upto.saturating_add(w);
        }

        let order_states = self.build_order_states(messages);

        // Reference price series for realized-spread markout lookups: book
        // midpoints (CDA) and batch clearing prices (FBA), sorted by ts.
        let mut price_series: Vec<(u64, f64)> = Vec::new();
        for s in &self.books {
            if let (Some(bid), Some(ask)) = (s.best_bid, s.best_ask) {
                price_series.push((s.ts, (bid as f64 + ask as f64) / 2.0));
            }
        }
        for b in &self.batches {
            if let Some(p) = b.clearing_price {
                price_series.push((b.ts, p as f64));
            }
        }
        price_series.sort_by_key(|(ts, _)| *ts);

        // `price_series` is sorted above, but NOT globally by call order:
        // `self.trades` (and therefore the sequence of `target_ts` values
        // fed in below) isn't guaranteed ts-monotonic either — see the
        // `msg_ts` comment further down for why (accepted/rejected file
        // interleaving). So this can't be a resumable cursor; it has to be
        // a fresh lookup per call. `partition_point` finds the same "first
        // entry with ts >= target_ts" that `.find` did (in O(log n) instead
        // of restarting an O(n) scan from index 0 every time), and ties at
        // ts == target_ts still resolve to whichever entry `sort_by_key`
        // (stable) placed first — books before batches, since `price_series`
        // is built books-then-batches above.
        let price_at_or_after = |target_ts: u64| -> Option<f64> {
            let idx = price_series.partition_point(|(ts, _)| *ts < target_ts);
            price_series.get(idx).map(|(_, p)| *p)
        };

        // ---- Trades: executed volume, dispersion inputs, trader surplus,
        //      effective/realized spread, price impact ----
        let mut bucket_trade_prices: HashMap<u64, Vec<f64>> = HashMap::new();
        let mut bucket_eff: HashMap<u64, (f64, f64)> = HashMap::new(); // (sum weighted bps, sum qty)
        let mut bucket_realized: HashMap<u64, [(f64, f64); REALIZED_SPREAD_HORIZONS_SECS.len()]> = HashMap::new();

        for t in &self.trades {
            let trade = &t.trade;
            let b = bof(trade.ts);
            if !in_range(b) {
                continue;
            }
            let qty = trade.quantity as f64;
            let price = trade.price as f64;

            let entry = buckets.get_mut(&b).expect("bucket exists for every in-range ts");
            entry.executed_volume += qty;
            entry.executed_notional += qty * price;
            entry.trade_count += 1;
            bucket_trade_prices.entry(b).or_default().push(price);

            if let Some(buy_state) = order_states.get(&trade.buy_order_id) {
                if let Some(limit) = buy_state.limit_price {
                    entry.trader_surplus += (limit as f64 - price).max(0.0) * qty;
                }
            }
            if let Some(sell_state) = order_states.get(&trade.sell_order_id) {
                if let Some(limit) = sell_state.limit_price {
                    entry.trader_surplus += (price - limit as f64).max(0.0) * qty;
                }
            }

            if let Some(reference) = t.reference_price {
                let m = reference as f64;
                if m > 0.0 {
                    let dev_bps = deviation_bps(price, m, t.aggressor_side);
                    let (sum_v, sum_w) = bucket_eff.entry(b).or_insert((0.0, 0.0));
                    *sum_v += dev_bps * qty;
                    *sum_w += qty;

                    let horizons = bucket_realized.entry(b).or_insert([(0.0, 0.0); REALIZED_SPREAD_HORIZONS_SECS.len()]);
                    for (i, h) in REALIZED_SPREAD_HORIZONS_SECS.iter().enumerate() {
                        if let Some(future_price) = price_at_or_after(trade.ts + h * NS_PER_SEC) {
                            // Realized spread: 2*D*(p_k - m_{k+delta})/m_k
                            let realized = match t.aggressor_side {
                                Some(Side::Buy) => 2.0 * (price - future_price) / m,
                                Some(Side::Sell) => -2.0 * (price - future_price) / m,
                                None => 2.0 * (price - future_price).abs() / m,
                            } * 10_000.0;
                            horizons[i].0 += realized * qty;
                            horizons[i].1 += qty;
                        }
                    }
                }
            }
        }

        for (b, (sum_v, sum_w)) in &bucket_eff {
            if *sum_w > 0.0 {
                buckets.get_mut(b).unwrap().effective_spread_bps = Some(sum_v / sum_w);
            }
        }
        for (b, horizons) in &bucket_realized {
            let eff = buckets.get(b).and_then(|e| e.effective_spread_bps);
            let entry = buckets.get_mut(b).unwrap();
            let assign = |slot_r: &mut Option<f64>, slot_i: &mut Option<f64>, h: (f64, f64)| {
                if h.1 > 0.0 {
                    let r = h.0 / h.1;
                    *slot_r = Some(r);
                    if let Some(e) = eff {
                        *slot_i = Some(e - r);
                    }
                }
            };
            assign(&mut entry.realized_spread_bps_1s, &mut entry.price_impact_bps_1s, horizons[0]);
            assign(&mut entry.realized_spread_bps_5s, &mut entry.price_impact_bps_5s, horizons[1]);
            assign(&mut entry.realized_spread_bps_30s, &mut entry.price_impact_bps_30s, horizons[2]);
        }

        for (b, prices) in &bucket_trade_prices {
            // Relative dispersion (coefficient of variation), reported in bps,
            // so it's scale-free and comparable across price regimes / coins —
            // an absolute stddev of PRICE_SCALE prices was level-dependent.
            // ~0 for FBA by construction (one clearing price per batch).
            let m = mean(prices);
            let disp = if m > 0.0 { stddev(prices) / m * 10_000.0 } else { 0.0 };
            let entry = buckets.get_mut(b).unwrap();
            entry.intra_interval_price_dispersion = Some(disp);
        }

        // ---- CDA Kyle's lambda: OLS slope through the origin of the
        //      relative post-trade mid move (bps) on signed order flow (SOL),
        //      one regression observation per taker sweep ----
        //
        // A marketable order that sweeps several price levels produces several
        // `Trade`s that all share `ts`, aggressor side and pre-trade
        // `reference_price` — that's ONE observation, whose `x` is the net
        // signed fill. Pass A groups fills by ts; it MUST be a `BTreeMap` so
        // Pass B folds `Σxx`/`Σxy` in ascending-ts order — identical between a
        // streaming and a one-shot pass (a `HashMap` here would randomize the
        // f64 summation order and break `streaming_emit_in_pieces_matches_one_shot_finish`).
        let mut sweep: BTreeMap<u64, (f64, f64)> = BTreeMap::new(); // ts -> (net signed qty, pre-trade mid)
        for t in &self.trades {
            if !in_range(bof(t.trade.ts)) {
                continue;
            }
            let Some(reference) = t.reference_price else { continue };
            if reference == 0 {
                continue;
            }
            let signed = match t.aggressor_side {
                Some(Side::Buy) => t.trade.quantity as f64,
                Some(Side::Sell) => -(t.trade.quantity as f64),
                // No taker/maker distinction (FBA batch trades) — the sign is
                // the whole regressor, so an unsigned observation is useless.
                None => continue,
            };
            let e = sweep.entry(t.trade.ts).or_insert((0.0, reference as f64));
            e.0 += signed;
        }
        let mut bucket_kyle: HashMap<u64, (f64, f64)> = HashMap::new(); // bucket -> (Σxx, Σxy)
        for (ts, (signed_qty, pre_mid)) in &sweep {
            let Some(post_mid) = price_at_or_after(ts + KYLE_LAMBDA_HORIZON_SECS * NS_PER_SEC) else {
                continue;
            };
            let x = *signed_qty;
            let y = (post_mid - pre_mid) / pre_mid * 10_000.0;
            let e = bucket_kyle.entry(bof(*ts)).or_insert((0.0, 0.0));
            e.0 += x * x;
            e.1 += x * y;
        }
        for (b, (sxx, sxy)) in &bucket_kyle {
            if *sxx > 0.0 {
                buckets.get_mut(b).unwrap().kyle_lambda = Some(sxy / sxx);
            }
        }

        // ---- Realized volatility & Amihud illiquidity: from the reference
        //      price series, grouped into the same buckets ----
        let mut bucket_price_points: HashMap<u64, Vec<f64>> = HashMap::new();
        for (ts, p) in &price_series {
            let b = bof(*ts);
            if !in_range(b) {
                continue;
            }
            bucket_price_points.entry(b).or_default().push(*p);
        }
        for (b, points) in &bucket_price_points {
            if points.len() >= 2 {
                let returns: Vec<f64> = points.windows(2).filter(|w| w[0] > 0.0).map(|w| (w[1] - w[0]) / w[0]).collect();
                if !returns.is_empty() {
                    // Realized volatility: root sum of squared intra-bucket
                    // returns (Andersen/Bollerslev realized-variance estimator),
                    // NOT the stddev of the returns about their mean. With a
                    // single return in the bucket this is |r|, not 0.
                    let rv = returns.iter().map(|r| r * r).sum::<f64>().sqrt();
                    buckets.get_mut(b).unwrap().realized_volatility = Some(rv);
                }
            }
        }

        let mut bucket_close: BTreeMap<u64, f64> = BTreeMap::new();
        for (b, points) in &bucket_price_points {
            if let Some(&last) = points.last() {
                bucket_close.insert(*b, last);
            }
        }
        // `prev_close` carries in from the previous emit (an earlier,
        // already-written bucket) and back out to the recorder, so
        // `amihud_illiquidity` spans flush boundaries exactly as it did when
        // `finalize` saw the whole run at once. Empty buckets don't reset it.
        let mut prev_close: Option<f64> = carry_in.prev_close;
        for start in buckets.keys().cloned().collect::<Vec<_>>() {
            if let Some(&close) = bucket_close.get(&start) {
                if let Some(prev) = prev_close {
                    if prev > 0.0 {
                        let ret = (close - prev) / prev;
                        let entry = buckets.get_mut(&start).unwrap();
                        // Canonical Amihud (2002): |return| / DOLLAR volume, not
                        // per-SOL quantity. `executed_notional` is Σ qty·price in
                        // PRICE_SCALE units, so divide it back to plain currency.
                        // Reported ×1e6 (`ILLIQ × 10^6`), the standard convention —
                        // the raw per-dollar value is ~1e-9 on real data.
                        let dollar_vol = entry.executed_notional / PRICE_SCALE as f64;
                        if dollar_vol > 0.0 {
                            entry.amihud_illiquidity = Some(ret.abs() / dollar_vol * 1e6);
                        }
                    }
                }
                prev_close = Some(close);
            }
        }
        let prev_close_out = prev_close;

        // ---- FBA Kyle's lambda: OLS slope through the origin of a priced
        //      batch's relative clearing-price move (bps) on its own net
        //      order-flow imbalance (SOL), one observation per priced batch:
        //          x = net_order_flow_k                     (SOL, signed, pre-price-selection)
        //          y = (cp_k - cp_{k-1}) / cp_{k-1} * 1e4    (bps)
        //
        // Contemporaneous — a call auction sets one price as a function of
        // that round's net demand, exactly Kyle's model. The regressor is the
        // batch's TOTAL submitted buy-minus-sell quantity (`net_order_flow`),
        // NOT the `demand_at_price − supply_at_price` residual, which
        // `select_price` minimizes and which therefore carries almost no
        // directional signal.
        //
        // Own ascending walk over `self.batches` (not the unordered
        // `bucket_batches` map below — the `prev_clearing` carry needs order).
        // `self.batches` is recorded strictly ascending in `ts` by
        // `clear_fba_batch` and `prune` only drops a prefix, so a plain `Vec`
        // walk is ts-ordered and deterministic. A `clearing_price = None`
        // batch is skipped and does not advance the carry; a batch a later
        // flush owns (`bof(ts) >= hi`) is left for the next `emit`.
        let mut bucket_kyle_fba: HashMap<u64, (f64, f64)> = HashMap::new(); // bucket -> (Σxx, Σxy)
        let mut prev_cp: Option<f64> = carry_in.prev_clearing;
        let mut last_batch_ts = 0u64;
        for bt in &self.batches {
            debug_assert!(bt.ts >= last_batch_ts, "self.batches must stay ascending by ts");
            last_batch_ts = bt.ts;
            let b = bof(bt.ts);
            if b >= hi {
                continue; // a later flush owns it — leave the carry for the next `emit`
            }
            let Some(cp) = bt.clearing_price else { continue };
            let cp = cp as f64;
            if b >= lo {
                if let Some(pcp) = prev_cp {
                    if pcp > 0.0 {
                        let x = bt.net_order_flow;
                        let y = (cp - pcp) / pcp * 10_000.0;
                        let e = bucket_kyle_fba.entry(b).or_insert((0.0, 0.0));
                        e.0 += x * x;
                        e.1 += x * y;
                    }
                }
            }
            // Advance the carry even for a `b < lo` batch (defensive — those
            // are normally pruned) so the first in-range batch has a predecessor.
            prev_cp = Some(cp);
        }
        for (b, (sxx, sxy)) in &bucket_kyle_fba {
            if *sxx > 0.0 {
                buckets.get_mut(b).unwrap().kyle_lambda = Some(sxy / sxx);
            }
        }

        // ---- Messages: order-to-trade ratio, throughput input ----
        let mut bucket_msg_count: HashMap<u64, u64> = HashMap::new();
        for m in messages {
            let b = bof(m.ts);
            if !in_range(b) {
                continue;
            }
            *bucket_msg_count.entry(b).or_insert(0) += 1;
        }
        for (b, count) in &bucket_msg_count {
            let entry = buckets.get_mut(b).unwrap();
            if entry.trade_count > 0 {
                entry.order_to_trade_ratio = Some(*count as f64 / entry.trade_count as f64);
            }
        }

        // ---- Fill rate, time to execution, order size inflation:
        //      bucketed by each order's own submission time ----
        let mut bucket_orig: HashMap<u64, f64> = HashMap::new();
        let mut bucket_filled: HashMap<u64, f64> = HashMap::new();
        let mut bucket_ttf: HashMap<u64, Vec<f64>> = HashMap::new();
        let mut bucket_user_totals: HashMap<u64, HashMap<String, (f64, f64)>> = HashMap::new();

        for state in order_states.values() {
            let b = bof(state.first_seen_ts);
            if !in_range(b) {
                continue;
            }
            *bucket_orig.entry(b).or_insert(0.0) += state.orig_qty as f64;
            *bucket_filled.entry(b).or_insert(0.0) += state.filled_qty as f64;

            if let Some(fill_ts) = state.first_fill_ts {
                let ttf_secs = fill_ts.saturating_sub(state.first_seen_ts) as f64 / NS_PER_SEC as f64;
                bucket_ttf.entry(b).or_default().push(ttf_secs);
            }

            let totals = bucket_user_totals.entry(b).or_default();
            let user_entry = totals.entry(state.user_id.clone()).or_insert((0.0, 0.0));
            user_entry.0 += state.orig_qty as f64;
            user_entry.1 += state.filled_qty as f64;
        }

        for (b, orig) in &bucket_orig {
            if *orig > 0.0 {
                let filled = bucket_filled.get(b).cloned().unwrap_or(0.0);
                buckets.get_mut(b).unwrap().fill_rate = Some(filled / orig);
            }
        }
        for (b, ttfs) in &bucket_ttf {
            if !ttfs.is_empty() {
                buckets.get_mut(b).unwrap().avg_time_to_execution_secs = Some(ttfs.iter().sum::<f64>() / ttfs.len() as f64);
            }
        }
        for (b, totals) in &bucket_user_totals {
            // Only orders that actually got SOME fill count toward
            // inflation — a never-filled order's raw size says nothing
            // about "stuffing" (over-sizing relative to what actually
            // executes), it's just unexecuted size. Including it with
            // `filled.max(1.0)` in the denominator used to make this ratio
            // explode into the hundreds, dominated by orders that simply
            // never traded rather than ones that were deliberately
            // oversized.
            let ratios: Vec<f64> = totals.values().filter(|(orig, filled)| *orig > 0.0 && *filled > 0.0).map(|(orig, filled)| orig / filled).collect();
            if !ratios.is_empty() {
                buckets.get_mut(b).unwrap().order_size_inflation = Some(ratios.iter().sum::<f64>() / ratios.len() as f64);
            }
        }

        // ---- Wall-clock instrumentation: throughput & clearing latency ----
        let mut bucket_compute_time: HashMap<u64, Duration> = HashMap::new();
        let mut bucket_compute_count: HashMap<u64, u64> = HashMap::new();
        for s in &self.books {
            let b = bof(s.ts);
            if !in_range(b) {
                continue;
            }
            *bucket_compute_time.entry(b).or_insert(Duration::ZERO) += s.compute_time;
            *bucket_compute_count.entry(b).or_insert(0) += 1;
        }
        for bt in &self.batches {
            let b = bof(bt.ts);
            if !in_range(b) {
                continue;
            }
            *bucket_compute_time.entry(b).or_insert(Duration::ZERO) += bt.compute_time;
            *bucket_compute_count.entry(b).or_insert(0) += 1;
        }
        for (b, total_time) in &bucket_compute_time {
            let entry = buckets.get_mut(b).unwrap();
            let count = bucket_compute_count.get(b).cloned().unwrap_or(0);
            if count > 0 {
                entry.avg_clearing_latency_micros = Some(total_time.as_micros() as f64 / count as f64);
            }
            let secs = total_time.as_secs_f64();
            if secs > 0.0 {
                if let Some(msg_count) = bucket_msg_count.get(b) {
                    entry.throughput_orders_per_sec = Some(*msg_count as f64 / secs);
                }
            }
        }

        // ---- FBA-only: quoted-spread analogue, depth, residual, boundary
        //      concentration, all from BatchClearedEvent ----
        //
        // `msg_ts`: every message timestamp, sorted once, so
        // `boundary_concentration` below can binary-search each batch's
        // window instead of rescanning the whole (potentially huge)
        // `messages` log per batch. NOT the same as assuming `messages`
        // itself is already ts-ordered — it isn't: input files are
        // streamed accepted-then-rejected within the same hour (see
        // `inputs::simulator::collect_input_files`), and their ts ranges
        // overlap, so `messages` can go backwards in time at that file
        // boundary. Sorting a plain `Vec<u64>` copy here avoids needing
        // `messages` itself to be sorted for anything else that reads it
        // in original stream order (e.g. `bucket_msg_count` above).
        let mut msg_ts: Vec<u64> = messages.iter().map(|m| m.ts).collect();
        msg_ts.sort_unstable();

        let mut bucket_batches: HashMap<u64, Vec<&BatchClearedEvent>> = HashMap::new();
        for bt in &self.batches {
            let b = bof(bt.ts);
            if !in_range(b) {
                continue;
            }
            bucket_batches.entry(b).or_default().push(bt);
        }
        for (b, evs) in &bucket_batches {
            let entry = buckets.get_mut(b).unwrap();

            let spreads: Vec<f64> = evs
                .iter()
                .filter_map(|e| match (e.best_unfilled_buy, e.best_unfilled_sell, e.clearing_price) {
                    (Some(buy), Some(sell), Some(p)) if p > 0 => Some(((sell as f64 - buy as f64) / p as f64) * 10_000.0),
                    _ => None,
                })
                .collect();
            if !spreads.is_empty() {
                entry.quoted_spread_bps = Some(mean(&spreads));
            }

            let depths: Vec<f64> = evs.iter().map(|e| (e.demand_at_price as f64 + e.supply_at_price as f64) / 2.0).collect();
            if !depths.is_empty() {
                entry.depth_at_best = Some(mean(&depths));
            }

            for i in 0..DEPTH_BPS_THRESHOLDS.len() {
                let vals: Vec<f64> = evs
                    .iter()
                    .map(|e| {
                        let (d, s) = e.depth_schedule[i];
                        (d as f64 + s as f64) / 2.0
                    })
                    .collect();
                if !vals.is_empty() {
                    entry.depth_within_bps[i] = Some(mean(&vals));
                }
            }

            let total_side: f64 = evs.iter().map(|e| e.demand_at_price.max(e.supply_at_price) as f64).sum();
            let total_unexec: f64 = evs.iter().map(|e| e.unexecuted_quantity as f64).sum();
            if total_side > 0.0 {
                entry.unexecuted_residual_share = Some(total_unexec / total_side);
            }

            // Boundary concentration: share of order arrivals in the final
            // 10% of each batch's own window, across batches closing in
            // this bucket. Binary-searches the pre-sorted `msg_ts` instead
            // of rescanning `messages` per batch — was O(batches x
            // messages), which is why this used to be the dominant cost on
            // a large multi-file `simulate` run; now O(batches log
            // messages).
            let mut total_msgs = 0u64;
            let mut boundary_msgs = 0u64;
            for e in evs {
                let window = e.ts.saturating_sub(e.batch_open_ts);
                if window == 0 {
                    continue;
                }
                let boundary_start = e.ts.saturating_sub(window / 10);

                // [batch_open_ts, ts] inclusive on both ends, matching the
                // original `m.ts >= e.batch_open_ts && m.ts <= e.ts`.
                let m_lo = msg_ts.partition_point(|&t| t < e.batch_open_ts);
                let m_hi = msg_ts.partition_point(|&t| t <= e.ts);
                total_msgs += (m_hi - m_lo) as u64;

                // `boundary_start >= e.batch_open_ts` always holds (it's
                // `e.ts - window/10` and `window/10 <= window`), so the
                // boundary sub-range sits inside `[lo, hi)` with no extra
                // clamping needed — matching the original nested
                // `m.ts >= boundary_start` check (itself already bounded
                // above by `m.ts <= e.ts`).
                let boundary_lo = msg_ts.partition_point(|&t| t < boundary_start);
                boundary_msgs += (m_hi - boundary_lo) as u64;
            }
            if total_msgs > 0 {
                entry.boundary_concentration = Some(boundary_msgs as f64 / total_msgs as f64);
            }
        }

        // ---- CDA-only: quoted spread, depth, book imbalance, from BookSnapshot ----
        let mut bucket_books: HashMap<u64, Vec<&BookSnapshot>> = HashMap::new();
        for s in &self.books {
            let b = bof(s.ts);
            if !in_range(b) {
                continue;
            }
            bucket_books.entry(b).or_default().push(s);
        }
        for (b, snaps) in &bucket_books {
            let entry = buckets.get_mut(b).unwrap();

            let spreads: Vec<f64> = snaps
                .iter()
                .filter_map(|s| match (s.best_bid, s.best_ask) {
                    (Some(bid), Some(ask)) if bid + ask > 0 => {
                        let mid = (bid as f64 + ask as f64) / 2.0;
                        Some(((ask as f64 - bid as f64) / mid) * 10_000.0)
                    }
                    _ => None,
                })
                .collect();
            if !spreads.is_empty() {
                entry.quoted_spread_bps = Some(mean(&spreads));
            }

            // Top-of-book only — the single best bid's and best ask's
            // remaining size, NOT the whole resting book (that's
            // `total_book_depth` below). Using `bid_depth`/`ask_depth`
            // here used to make `depth_at_best` come out ~10x LARGER than
            // `depth_within_10bps`, which is backwards.
            let touch_depths: Vec<f64> = snaps.iter().map(|s| (s.best_bid_qty as f64 + s.best_ask_qty as f64) / 2.0).collect();
            if !touch_depths.is_empty() {
                entry.depth_at_best = Some(mean(&touch_depths));
            }

            for i in 0..DEPTH_BPS_THRESHOLDS.len() {
                let vals: Vec<f64> = snaps
                    .iter()
                    .map(|s| {
                        let (bid_d, ask_d) = s.depth_schedule[i];
                        (bid_d as f64 + ask_d as f64) / 2.0
                    })
                    .collect();
                if !vals.is_empty() {
                    entry.depth_within_bps[i] = Some(mean(&vals));
                }
            }

            let imbalances: Vec<f64> = snaps
                .iter()
                .filter_map(|s| {
                    let total = s.best_bid_qty as f64 + s.best_ask_qty as f64;
                    if total > 0.0 {
                        Some((s.best_bid_qty as f64 - s.best_ask_qty as f64) / total)
                    } else {
                        None
                    }
                })
                .collect();
            if !imbalances.is_empty() {
                entry.book_imbalance = Some(mean(&imbalances));
            }

            // Whole-book depth, both sides, all price levels — the old
            // (mistaken) definition of `depth_at_best`, kept under its own
            // honest name since it's still a useful "how much liquidity is
            // resting in total" number.
            let total_depths: Vec<f64> = snaps.iter().map(|s| (s.bid_depth as f64 + s.ask_depth as f64) / 2.0).collect();
            if !total_depths.is_empty() {
                entry.total_book_depth = Some(mean(&total_depths));
            }
        }

        // ---- VWAP: executed_notional / executed_volume, per bucket, both
        //      engines — computed after the trade loop above has filled
        //      those two fields in for every bucket.
        for entry in buckets.values_mut() {
            if entry.executed_volume > 0.0 {
                entry.vwap = Some(entry.executed_notional / entry.executed_volume);
            }
        }

        // pricing_error_bps intentionally left None everywhere: it needs an
        // external reference price feed (Hyperliquid's own oracle/mark
        // price) this dataset doesn't include — see the field's doc comment.

        (buckets.into_values().collect(), Carry { prev_close: prev_close_out, prev_clearing: prev_cp }, next_upto)
    }
}

fn deviation_bps(price: f64, reference: f64, aggressor_side: Option<Side>) -> f64 {
    match aggressor_side {
        Some(Side::Buy) => 2.0 * (price - reference) / reference * 10_000.0,
        Some(Side::Sell) => -2.0 * (price - reference) / reference * 10_000.0,
        // No taker/maker distinction in a uniform-price batch auction —
        // report an unsigned deviation instead of guessing a direction.
        None => 2.0 * (price - reference).abs() / reference * 10_000.0,
    }
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    let var = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64;
    var.sqrt()
}

// ============================================================================
// CSV rendering
// ============================================================================

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.6}"),
        None => String::new(),
    }
}

pub fn csv_header() -> String {
    let mut cols = vec!["engine", "interval_start_ns", "interval_width_ns", "quoted_spread_bps", "depth_at_best"];
    for bps in DEPTH_BPS_THRESHOLDS {
        cols.push(match bps {
            10 => "depth_within_10bps",
            50 => "depth_within_50bps",
            100 => "depth_within_100bps",
            _ => "depth_within_bps",
        });
    }
    cols.extend([
        "book_imbalance",
        "total_book_depth",
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
        "pricing_error_bps",
        "executed_volume",
        "executed_notional",
        "vwap",
        "trade_count",
        "fill_rate",
        "avg_time_to_execution_secs",
        "trader_surplus",
        "order_size_inflation",
        "order_to_trade_ratio",
        "boundary_concentration",
        "throughput_orders_per_sec",
        "avg_clearing_latency_micros",
        "unexecuted_residual_share",
    ]);
    cols.join(",")
}

pub fn csv_row(m: &IntervalMetrics) -> String {
    let mut fields = vec![m.engine.to_string(), m.interval_start.to_string(), m.interval_width.to_string(), fmt_opt(m.quoted_spread_bps), fmt_opt(m.depth_at_best)];
    for v in m.depth_within_bps {
        fields.push(fmt_opt(v));
    }
    fields.extend([
        fmt_opt(m.book_imbalance),
        fmt_opt(m.total_book_depth),
        fmt_opt(m.effective_spread_bps),
        fmt_opt(m.realized_spread_bps_1s),
        fmt_opt(m.realized_spread_bps_5s),
        fmt_opt(m.realized_spread_bps_30s),
        fmt_opt(m.price_impact_bps_1s),
        fmt_opt(m.price_impact_bps_5s),
        fmt_opt(m.price_impact_bps_30s),
        fmt_opt(m.amihud_illiquidity),
        fmt_opt(m.kyle_lambda),
        fmt_opt(m.realized_volatility),
        fmt_opt(m.intra_interval_price_dispersion),
        fmt_opt(m.pricing_error_bps),
        format!("{:.6}", m.executed_volume),
        format!("{:.6}", m.executed_notional),
        fmt_opt(m.vwap),
        m.trade_count.to_string(),
        fmt_opt(m.fill_rate),
        fmt_opt(m.avg_time_to_execution_secs),
        format!("{:.6}", m.trader_surplus),
        fmt_opt(m.order_size_inflation),
        fmt_opt(m.order_to_trade_ratio),
        fmt_opt(m.boundary_concentration),
        fmt_opt(m.throughput_orders_per_sec),
        fmt_opt(m.avg_clearing_latency_micros),
        fmt_opt(m.unexecuted_residual_share),
    ]);
    fields.join(",")
}

/// Just the data rows (each newline-terminated), no header — for appending
/// an incrementally-flushed batch of intervals to a CSV that already has
/// its header line (see `inputs::simulate_cmd`).
pub fn csv_rows(series: &[IntervalMetrics]) -> String {
    let mut out = String::new();
    for row in series {
        out.push_str(&csv_row(row));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(ts: u64, oid: u64) -> OrderMessage {
        OrderMessage { ts, oid, user_id: "u".to_string(), side: Side::Buy, limit_price: None, quantity: 1, accepted: true }
    }

    fn batch(batch_open_ts: u64, ts: u64) -> BatchClearedEvent {
        BatchClearedEvent {
            ts,
            batch_open_ts,
            clearing_price: None,
            demand_at_price: 0,
            supply_at_price: 0,
            net_order_flow: 0.0,
            traded_quantity: 0,
            unexecuted_quantity: 0,
            best_unfilled_buy: None,
            best_unfilled_sell: None,
            depth_schedule: [(0, 0); DEPTH_BPS_THRESHOLDS.len()],
            compute_time: Duration::ZERO,
        }
    }

    fn book(ts: u64, best_bid: Option<u128>, best_ask: Option<u128>) -> BookSnapshot {
        BookSnapshot {
            ts,
            best_bid,
            best_ask,
            best_bid_qty: 0,
            best_ask_qty: 0,
            bid_depth: 0,
            ask_depth: 0,
            depth_schedule: [(0, 0); DEPTH_BPS_THRESHOLDS.len()],
            compute_time: Duration::ZERO,
        }
    }

    // Small deterministic LCG (no `rand` dependency in this crate) — same
    // constants as glibc's `rand()`, good enough for a reproducible fuzz
    // test, not for anything security-sensitive.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn range(&mut self, n: u64) -> u64 {
            if n == 0 { 0 } else { self.next() % n }
        }
    }

    // ---- boundary_concentration ----

    #[test]
    fn boundary_concentration_matches_hand_computed_value() {
        let mut rec = MetricsRecorder::new(EngineKind::Fba, 1_000_000_000);
        // window = [0, 100], boundary_start = 100 - 100/10 = 90.
        let messages: Vec<OrderMessage> = [0u64, 50, 90, 99, 100, 101].into_iter().enumerate().map(|(i, ts)| msg(ts, i as u64)).collect();
        rec.record_batch(batch(0, 100));

        let series = rec.finalize(&messages);
        assert_eq!(series.len(), 1);
        // total: ts in [0,100] -> {0,50,90,99,100} = 5 msgs (101 excluded).
        // boundary: ts >= 90 among those -> {90,99,100} = 3 msgs.
        assert_eq!(series[0].boundary_concentration, Some(3.0 / 5.0));
    }

    #[test]
    fn boundary_concentration_respects_inclusive_window_boundaries() {
        let mut rec = MetricsRecorder::new(EngineKind::Fba, 1_000_000_000);
        // batch_open_ts=1000, ts=2000 -> window=1000, boundary_start=1900.
        let messages = vec![
            msg(999, 1),  // just before open -> excluded entirely
            msg(1000, 2), // exactly at open -> in total, not boundary
            msg(1899, 3), // just before boundary_start -> in total, not boundary
            msg(1900, 4), // exactly at boundary_start -> in both
            msg(2000, 5), // exactly at close -> in both
            msg(2001, 6), // just after close -> excluded entirely
        ];
        rec.record_batch(batch(1000, 2000));

        let series = rec.finalize(&messages);
        assert_eq!(series.len(), 1);
        // total = {1000,1899,1900,2000} = 4, boundary = {1900,2000} = 2.
        assert_eq!(series[0].boundary_concentration, Some(2.0 / 4.0));
    }

    #[test]
    fn boundary_concentration_is_none_for_a_zero_width_batch() {
        let mut rec = MetricsRecorder::new(EngineKind::Fba, 1_000_000_000);
        let messages = vec![msg(500, 1)];
        rec.record_batch(batch(500, 500)); // window == 0 -> skipped entirely

        let series = rec.finalize(&messages);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].boundary_concentration, None);
    }

    /// Differential test: the fast `partition_point`-based computation in
    /// `finalize()` must agree exactly (integer counts, no floats involved
    /// until the final division) with a naive re-scan written directly here
    /// — the same logic the production code used before it was rewritten to
    /// avoid rescanning `messages` per batch.
    #[test]
    fn boundary_concentration_matches_naive_scan_across_random_batches() {
        let mut rng = Lcg(0xC0FFEE);
        for _ in 0..200 {
            let batch_open_ts = rng.range(1000);
            let extra = rng.range(1000) + 1; // ensure window > 0
            let ts = batch_open_ts + extra;

            let n_msgs = rng.range(30) as usize;
            let msg_tss: Vec<u64> = (0..n_msgs).map(|_| rng.range(2500)).collect();

            let mut rec = MetricsRecorder::new(EngineKind::Fba, 1_000_000_000);
            let messages: Vec<OrderMessage> = msg_tss.iter().enumerate().map(|(i, &t)| msg(t, i as u64)).collect();
            rec.record_batch(batch(batch_open_ts, ts));

            // Naive reference: exactly the original nested-loop semantics.
            let window = ts - batch_open_ts;
            let boundary_start = ts.saturating_sub(window / 10);
            let mut total = 0u64;
            let mut boundary = 0u64;
            for &t in &msg_tss {
                if t >= batch_open_ts && t <= ts {
                    total += 1;
                    if t >= boundary_start {
                        boundary += 1;
                    }
                }
            }
            let expected = if total > 0 { Some(boundary as f64 / total as f64) } else { None };

            let series = rec.finalize(&messages);
            assert_eq!(series.len(), 1);
            assert_eq!(series[0].boundary_concentration, expected, "batch_open_ts={batch_open_ts} ts={ts} msgs={msg_tss:?}");
        }
    }

    // ---- price_at_or_after (exercised indirectly via realized spread) ----

    #[test]
    fn price_at_or_after_ties_prefer_book_snapshot_over_batch_at_same_ts() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 10_000_000_000);
        let trade = Trade {
            trade_id: 1,
            price: 100,
            quantity: 1,
            buyer_id: "b".to_string(),
            seller_id: "s".to_string(),
            buy_order_id: 1,
            sell_order_id: 2,
            engine_type: EngineKind::Cda,
            ts: 0,
            trade_tx_hash: None,
            chain_id: None,
        };
        rec.record_trade(TradeEvent { trade, reference_price: Some(100), aggressor_side: Some(Side::Buy) });

        // Both land exactly at the 1s horizon target (0 + 1s), each with a
        // DIFFERENT price (100.0 vs 300.0) so the test actually fails if
        // the wrong one wins. The book snapshot must win the tie because
        // `price_series` is built books-then-batches before a stable sort.
        rec.record_book_snapshot(book(1_000_000_000, Some(90), Some(110))); // mid = 100.0
        let mut tied_batch = batch(0, 1_000_000_000);
        tied_batch.clearing_price = Some(300); // must lose the tie to the book's 100.0
        rec.record_batch(tied_batch);

        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        // Deviation of the trade price (100) from the future price found
        // (should be the book's mid, 100.0) is exactly 0.
        assert_eq!(series[0].realized_spread_bps_1s, Some(0.0));
    }

    #[test]
    fn price_at_or_after_returns_none_past_the_last_entry() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 100_000_000_000);
        let trade = Trade {
            trade_id: 1,
            price: 100,
            quantity: 1,
            buyer_id: "b".to_string(),
            seller_id: "s".to_string(),
            buy_order_id: 1,
            sell_order_id: 2,
            engine_type: EngineKind::Cda,
            ts: 0,
            trade_tx_hash: None,
            chain_id: None,
        };
        rec.record_trade(TradeEvent { trade, reference_price: Some(100), aggressor_side: Some(Side::Buy) });
        // Only price-series entry is well before the 30s horizon target.
        rec.record_book_snapshot(book(2_000_000_000, Some(90), Some(110)));

        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].realized_spread_bps_30s, None);
    }

    // ---- streaming: emit/flush/prune ----

    const W: u64 = 1_000_000_000; // 1s buckets, as `simulate`'s default

    fn cda_trade(ts: u64, oid: u64, price: u128) -> TradeEvent {
        TradeEvent {
            trade: Trade {
                trade_id: oid,
                price,
                quantity: 1,
                buyer_id: "b".to_string(),
                seller_id: "s".to_string(),
                buy_order_id: oid,
                sell_order_id: oid + 1_000_000,
                engine_type: EngineKind::Cda,
                ts,
                trade_tx_hash: None,
                chain_id: None,
            },
            reference_price: Some(price),
            aggressor_side: Some(Side::Buy),
        }
    }

    /// Feed the same synthetic CDA stream two ways — one recorder that sees
    /// everything then `finish()`es once, and one that `emit()`s settled
    /// buckets file-by-file, prunes, and `finish()`es the tail — and require
    /// the concatenated CSV rows to be byte-identical. This is the core
    /// guarantee of the streaming refactor: flushing in pieces changes
    /// nothing about the numbers.
    #[test]
    fn streaming_emit_in_pieces_matches_one_shot_finish() {
        // Settle window comfortably past the widest markout horizon (30s),
        // so a bucket is only ever emitted once all its forward prices exist.
        let settle = 35 * W;
        let seconds = 140u64;

        let mut all_msgs: Vec<OrderMessage> = Vec::new();
        let mut events: Vec<(u64, EventKind)> = Vec::new();
        enum EventKind {
            Book(BookSnapshot),
            Trade(TradeEvent),
        }
        let mut rng = Lcg(0x5EED);
        for s in 0..seconds {
            let base = s * W + 1;
            let mid = 100 + (rng.range(7) as u128); // wanders 100..=106
            all_msgs.push(msg(base, s));
            events.push((base, EventKind::Book(book(base, Some(mid.saturating_sub(1)), Some(mid + 1)))));
            if s % 3 == 0 {
                // Trade a hair after its own order's message, well inside the
                // settle window, so no metric that depends on the order's
                // submission is affected by pruning.
                events.push((base + 10, EventKind::Trade(cda_trade(base + 10, s, mid))));
                // A second, opposite-side flow at a distinct sub-second ts so
                // `kyle_lambda` gets MULTIPLE observations per bucket — this
                // guards the cross-flush f64 summation order of `bucket_kyle`,
                // not just the single-observation case.
                let mut sell = cda_trade(base + 20, s + 100_000, mid + 2);
                sell.aggressor_side = Some(Side::Sell);
                events.push((base + 20, EventKind::Trade(sell)));
            }
        }
        let final_max_ts = seconds * W;

        // (a) one-shot
        let mut one = MetricsRecorder::new(EngineKind::Cda, W);
        for (_, ev) in &events {
            match ev {
                EventKind::Book(b) => one.record_book_snapshot(b.clone()),
                EventKind::Trade(t) => one.record_trade(t.clone()),
            }
        }
        let one_rows = one.finish(&all_msgs, final_max_ts);

        // (b) streaming, one "file" per simulated second
        let mut strm = MetricsRecorder::new(EngineKind::Cda, W);
        let mut msgs_seen: Vec<OrderMessage> = Vec::new();
        let mut strm_rows: Vec<IntervalMetrics> = Vec::new();
        for s in 0..seconds {
            let sec_lo = s * W;
            let sec_hi = sec_lo + W;
            for (ts, ev) in &events {
                if *ts < sec_lo || *ts >= sec_hi {
                    continue;
                }
                match ev {
                    EventKind::Book(b) => strm.record_book_snapshot(b.clone()),
                    EventKind::Trade(t) => strm.record_trade(t.clone()),
                }
            }
            for m in all_msgs.iter().filter(|m| m.ts >= sec_lo && m.ts < sec_hi) {
                msgs_seen.push(m.clone());
            }
            let watermark = sec_hi.saturating_sub(1);
            if let Some(a) = strm.anchor() {
                let safe = watermark.saturating_sub(settle);
                if safe > a {
                    let safe_bucket = a + ((safe - a) / W) * W;
                    strm_rows.extend(strm.emit(&msgs_seen, safe_bucket));
                    strm.prune();
                    let cutoff = strm.emitted_upto();
                    msgs_seen.retain(|m| bucket_of(m.ts, a, W) >= cutoff);
                }
            }
        }
        strm_rows.extend(strm.finish(&msgs_seen, final_max_ts));

        assert_eq!(
            csv_rows(&strm_rows),
            csv_rows(&one_rows),
            "streaming flush must reproduce the one-shot series exactly"
        );
        // Sanity: the stream actually exercised multiple partial flushes.
        assert!(strm_rows.len() as u64 >= seconds, "expected one row per second, got {}", strm_rows.len());
    }

    #[test]
    fn emit_fills_gaps_with_empty_rows_for_a_contiguous_grid() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, W);
        rec.record_book_snapshot(book(0, Some(99), Some(101)));
        rec.record_book_snapshot(book(10 * W + 1, Some(99), Some(101)));

        let rows = rec.finish(&[], 10 * W + 1);
        assert_eq!(rows.len(), 11, "buckets 0..=10 inclusive");
        for (i, r) in rows.iter().enumerate() {
            assert_eq!(r.interval_start, i as u64 * W, "row {i} sits on the grid");
        }
        // Only the two populated buckets carry a spread.
        assert!(rows[0].quoted_spread_bps.is_some());
        assert!(rows[10].quoted_spread_bps.is_some());
        assert!(rows[5].quoted_spread_bps.is_none());
    }

    #[test]
    fn events_for_an_already_emitted_bucket_are_counted_not_stored() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, W);
        // Establish the anchor at ts=0 and stream a little past 5s.
        for s in 0..6u64 {
            rec.record_book_snapshot(book(s * W + 1, Some(99), Some(101)));
        }
        // Flush buckets 0..=3.
        let flushed = rec.emit(&[], 4 * W);
        assert_eq!(flushed.len(), 4);
        rec.prune();
        assert_eq!(rec.late_events_dropped(), 0);

        // A late snapshot for bucket 1 (already emitted) must be rejected.
        rec.record_book_snapshot(book(1 * W + 500, Some(1), Some(3)));
        assert_eq!(rec.late_events_dropped(), 1);

        // ...and it must not perturb bucket 1 on any later flush.
        let rest = rec.finish(&[], 6 * W);
        // rest covers buckets 4 and 5 only; bucket 1 was already emitted.
        assert!(rest.iter().all(|r| r.interval_start >= 4 * W));
    }

    // ---- Kyle's lambda ----

    /// A CDA `TradeEvent` with an explicit side / quantity / pre-trade mid.
    fn cda_flow(ts: u64, side: Side, qty: u128, reference: u128) -> TradeEvent {
        TradeEvent {
            trade: Trade {
                trade_id: 1,
                price: reference,
                quantity: qty,
                buyer_id: "b".to_string(),
                seller_id: "s".to_string(),
                buy_order_id: 1,
                sell_order_id: 2,
                engine_type: EngineKind::Cda,
                ts,
                trade_tx_hash: None,
                chain_id: None,
            },
            reference_price: Some(reference),
            aggressor_side: Some(side),
        }
    }

    /// A fully-specified FBA `BatchClearedEvent` for lambda tests.
    fn fba_clear(ts: u64, batch_open_ts: u64, clearing_price: Option<u128>, net_flow: f64, demand: u128, supply: u128) -> BatchClearedEvent {
        BatchClearedEvent {
            ts,
            batch_open_ts,
            clearing_price,
            demand_at_price: demand,
            supply_at_price: supply,
            net_order_flow: net_flow,
            traded_quantity: demand.min(supply),
            unexecuted_quantity: demand.abs_diff(supply),
            best_unfilled_buy: None,
            best_unfilled_sell: None,
            depth_schedule: [(0, 0); DEPTH_BPS_THRESHOLDS.len()],
            compute_time: Duration::ZERO,
        }
    }

    #[test]
    fn kyle_lambda_cda_recovers_a_known_slope() {
        // One 10s bucket so every event lands in bucket 0.
        let big = 10 * W;
        let pre = 1_000_000u128;
        let lambda = 2.0; // bps of mid move per SOL of signed flow
        // For signed flow x, place the +5s markout book mid at
        // pre*(1 + lambda*x/1e4) so y == lambda*x exactly.
        let post = |x: i128| -> u128 { (pre as f64 * (1.0 + lambda * x as f64 / 10_000.0)).round() as u128 };

        let mut rec = MetricsRecorder::new(EngineKind::Cda, big);
        // Observation 1: net +5 (single Buy).
        rec.record_trade(cda_flow(1_000_000, Side::Buy, 5, pre));
        rec.record_book_snapshot(book(1_000_000 + 5 * W, Some(post(5)), Some(post(5))));
        // Observation 2: net -3 (single Sell).
        rec.record_trade(cda_flow(2_000_000, Side::Sell, 3, pre));
        rec.record_book_snapshot(book(2_000_000 + 5 * W, Some(post(-3)), Some(post(-3))));
        // Observation 3: a sweep — two Buys at the SAME ts, net +5.
        rec.record_trade(cda_flow(3_000_000, Side::Buy, 2, pre));
        rec.record_trade(cda_flow(3_000_000, Side::Buy, 3, pre));
        rec.record_book_snapshot(book(3_000_000 + 5 * W, Some(post(5)), Some(post(5))));
        // Observation 4: net-zero sweep (Buy 3 + Sell 3) — must be inert.
        rec.record_trade(cda_flow(4_000_000, Side::Buy, 3, pre));
        rec.record_trade(cda_flow(4_000_000, Side::Sell, 3, pre));

        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        // Sxx = 25 + 9 + 25 = 59 ; Sxy = 50 + 18 + 50 = 118 ; lambda = 2.0.
        let got = series[0].kyle_lambda.expect("lambda computable");
        assert!((got - lambda).abs() < 1e-9, "recovered lambda {got}, expected {lambda}");
    }

    #[test]
    fn kyle_lambda_cda_is_none_when_the_markout_mid_is_missing() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 10 * W);
        rec.record_trade(cda_flow(1_000_000, Side::Buy, 5, 1_000_000));
        // Only price point is well BEFORE the trade's +5s horizon target.
        rec.record_book_snapshot(book(2_000_000, Some(1_000_000), Some(1_000_000)));

        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].kyle_lambda, None);
    }

    #[test]
    fn kyle_lambda_fba_from_net_order_flow() {
        // Contemporaneous: batch k's clearing-price move (vs batch k-1)
        // regressed on batch k's own net order flow.
        let big = 100 * W;
        let lambda = 125.0;
        let mut rec = MetricsRecorder::new(EngineKind::Fba, big);
        // batch0: seeds prev_clearing = 1_000_000. Its own flow is irrelevant.
        rec.record_batch(fba_clear(1_000_000, 0, Some(1_000_000), 0.0, 0, 0));
        // batch1: x = 8, y = (1_100_000-1_000_000)/1_000_000*1e4 = 1000 = 125*8.
        rec.record_batch(fba_clear(2_000_000, 1_000_000, Some(1_100_000), 8.0, 0, 0));
        // batch2: x = -4, y = (1_045_000-1_100_000)/1_100_000*1e4 = -500 = 125*-4.
        rec.record_batch(fba_clear(3_000_000, 2_000_000, Some(1_045_000), -4.0, 0, 0));
        // batch3: no price -> inert, prev_clearing unchanged at 1_045_000.
        rec.record_batch(fba_clear(4_000_000, 3_000_000, None, 99.0, 0, 0));
        // batch4: x = 2, y = (1_071_125-1_045_000)/1_045_000*1e4 = 250 = 125*2.
        rec.record_batch(fba_clear(5_000_000, 4_000_000, Some(1_071_125), 2.0, 0, 0));

        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        // Sxx = 64 + 16 + 4 = 84 ; Sxy = 8000 + 2000 + 500 = 10500 ; lambda = 125.
        let got = series[0].kyle_lambda.expect("lambda computable");
        assert!((got - lambda).abs() < 1e-6, "recovered lambda {got}, expected {lambda}");
    }

    #[test]
    fn kyle_lambda_fba_carries_prev_clearing_across_a_flush() {
        let mut rec = MetricsRecorder::new(EngineKind::Fba, W);
        rec.record_batch(fba_clear(1, 0, Some(1_000_000), 0.0, 0, 0)); // bucket 0: seeds prev_clearing
        rec.record_batch(fba_clear(W + 1, W, Some(1_100_000), 8.0, 0, 0)); // bucket 1: x = 8

        // Flush bucket 0 alone: first batch, no predecessor -> no lambda.
        let r0 = rec.emit(&[], W);
        assert_eq!(r0.len(), 1);
        assert_eq!(r0[0].kyle_lambda, None);
        rec.prune();

        // Bucket 1's lambda uses the clearing price carried out of bucket 0's emit:
        // x = 8, y = (1_100_000 - 1_000_000)/1_000_000 * 1e4 = 1000 ; lambda = 8000/64 = 125.
        let r1 = rec.finish(&[], W + 1);
        assert_eq!(r1.len(), 1);
        let got = r1[0].kyle_lambda.expect("lambda computable after the flush boundary");
        assert!((got - 125.0).abs() < 1e-6, "carried lambda {got}, expected 125");
    }

    #[test]
    fn streaming_fba_batches_emit_in_pieces_matches_one_shot() {
        // Same differential guarantee as the CDA test, for the FBA path —
        // exercises the `prev_clearing` (Kyle's lambda) AND `prev_close`
        // (Amihud) carries across flush boundaries, which nothing else does.
        let settle = 35 * W;
        let seconds = 140u64;

        let mut all_msgs: Vec<OrderMessage> = Vec::new();
        let mut batches: Vec<BatchClearedEvent> = Vec::new();
        let mut rng = Lcg(0xBA7C1A);
        for s in 0..seconds {
            let ts = s * W + 1;
            all_msgs.push(msg(ts, s));
            let cp = if s % 11 == 5 { None } else { Some(1_000_000 + rng.range(4000) as u128) };
            let net_flow = rng.range(400) as f64 - 200.0; // wanders -200..+199
            batches.push(fba_clear(ts, s * W, cp, net_flow, rng.range(50) as u128, rng.range(50) as u128));
        }
        let final_max_ts = seconds * W;

        let mut one = MetricsRecorder::new(EngineKind::Fba, W);
        for b in &batches {
            one.record_batch(b.clone());
        }
        let one_rows = one.finish(&all_msgs, final_max_ts);

        let mut strm = MetricsRecorder::new(EngineKind::Fba, W);
        let mut msgs_seen: Vec<OrderMessage> = Vec::new();
        let mut strm_rows: Vec<IntervalMetrics> = Vec::new();
        for s in 0..seconds {
            let sec_lo = s * W;
            let sec_hi = sec_lo + W;
            for b in batches.iter().filter(|b| b.ts >= sec_lo && b.ts < sec_hi) {
                strm.record_batch(b.clone());
            }
            for m in all_msgs.iter().filter(|m| m.ts >= sec_lo && m.ts < sec_hi) {
                msgs_seen.push(m.clone());
            }
            let watermark = sec_hi.saturating_sub(1);
            if let Some(a) = strm.anchor() {
                let safe = watermark.saturating_sub(settle);
                if safe > a {
                    let safe_bucket = a + ((safe - a) / W) * W;
                    strm_rows.extend(strm.emit(&msgs_seen, safe_bucket));
                    strm.prune();
                    let cutoff = strm.emitted_upto();
                    msgs_seen.retain(|m| bucket_of(m.ts, a, W) >= cutoff);
                }
            }
        }
        strm_rows.extend(strm.finish(&msgs_seen, final_max_ts));

        assert_eq!(
            csv_rows(&strm_rows),
            csv_rows(&one_rows),
            "FBA streaming flush must reproduce the one-shot series exactly"
        );
        assert!(strm_rows.len() as u64 >= seconds, "expected at least one row per second, got {}", strm_rows.len());
    }

    // ---- fixed-formula value checks (Amihud dollar volume, realized-vol
    //      estimator, relative dispersion) ----

    #[test]
    fn realized_volatility_is_root_sum_of_squared_returns() {
        // Three mids in one bucket -> two returns: +0.1 and -0.1.
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 10 * W);
        rec.record_book_snapshot(book(1_000_000, Some(100), Some(100)));
        rec.record_book_snapshot(book(2_000_000, Some(110), Some(110)));
        rec.record_book_snapshot(book(3_000_000, Some(99), Some(99)));
        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        // sqrt(0.1^2 + (-0.1)^2) = sqrt(0.02) — NOT the 0.1 stddev of {0.1,-0.1}.
        let got = series[0].realized_volatility.expect("rv computable");
        assert!((got - 0.02_f64.sqrt()).abs() < 1e-9, "rv {got}, expected {}", 0.02_f64.sqrt());

        // A single return in the bucket now reports |r|, not 0.
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 10 * W);
        rec.record_book_snapshot(book(1_000_000, Some(100), Some(100)));
        rec.record_book_snapshot(book(2_000_000, Some(105), Some(105)));
        let series = rec.finalize(&[]);
        assert_eq!(series[0].realized_volatility, Some(0.05));
    }

    #[test]
    fn amihud_illiquidity_divides_the_return_by_dollar_volume() {
        let p = |x: u128| x * PRICE_SCALE;
        let mut rec = MetricsRecorder::new(EngineKind::Cda, W);
        // Bucket A: one mid -> close p(100), no trades.
        rec.record_book_snapshot(book(1, Some(p(100)), Some(p(100))));
        // Bucket B: close p(110) and a trade of 5 @ p(200) -> dollar volume
        // = 5*200 = 1000 (after dividing notional back out of PRICE_SCALE).
        rec.record_book_snapshot(book(1_000_000_001, Some(p(110)), Some(p(110))));
        rec.record_trade(cda_flow(1_000_000_100, Side::Buy, 5, p(200)));
        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].amihud_illiquidity, None, "no prev_close for the first bucket");
        // |(110-100)/100| / 1000 * 1e6 = 0.1 / 1000 * 1e6 = 100 (ILLIQ x 1e6).
        let got = series[1].amihud_illiquidity.expect("amihud computable");
        assert!((got - 100.0).abs() < 1e-6, "amihud {got}, expected 100");
    }

    #[test]
    fn intra_interval_price_dispersion_is_relative_in_bps() {
        let mut rec = MetricsRecorder::new(EngineKind::Cda, 10 * W);
        rec.record_trade(cda_flow(1_000_000, Side::Buy, 1, 100));
        rec.record_trade(cda_flow(2_000_000, Side::Buy, 1, 102));
        let series = rec.finalize(&[]);
        assert_eq!(series.len(), 1);
        // stddev({100,102}) = 1.0 ; mean = 101 ; 1/101 * 1e4 bps.
        let got = series[0].intra_interval_price_dispersion.expect("dispersion computable");
        assert!((got - (1.0 / 101.0 * 10_000.0)).abs() < 1e-9, "dispersion {got}");
    }
}

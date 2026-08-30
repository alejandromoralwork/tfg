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

use crate::types::{EngineKind, Side, Trade};

/// Basis-point offsets the depth-within-x-bps metric is reported at.
pub const DEPTH_BPS_THRESHOLDS: [u32; 3] = [10, 50, 100];

const NS_PER_SEC: u64 = 1_000_000_000;
const REALIZED_SPREAD_HORIZONS_SECS: [u64; 3] = [1, 5, 30];

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

pub struct MetricsRecorder {
    engine: EngineKind,
    interval_width: u64,
    trades: Vec<TradeEvent>,
    batches: Vec<BatchClearedEvent>,
    books: Vec<BookSnapshot>,
}

impl MetricsRecorder {
    /// `interval_width_ns` is the bucket width in nanoseconds — use the
    /// same value for the FBA and CDA recorders in one `simulate` run so
    /// the two resulting time series sit on the same time grid.
    pub fn new(engine: EngineKind, interval_width_ns: u64) -> Self {
        Self {
            engine,
            interval_width: interval_width_ns,
            trades: Vec::new(),
            batches: Vec::new(),
            books: Vec::new(),
        }
    }

    pub fn record_trade(&mut self, trade: TradeEvent) {
        self.trades.push(trade);
    }

    pub fn record_batch(&mut self, batch: BatchClearedEvent) {
        self.batches.push(batch);
    }

    pub fn record_book_snapshot(&mut self, snapshot: BookSnapshot) {
        self.books.push(snapshot);
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

    /// Produce the metric time series: one row per `interval_width`-wide
    /// bucket spanning the full range of recorded timestamps.
    ///
    /// `messages` is owned by the caller (`inputs::simulate_cmd::run`) and
    /// shared by reference across the FBA and CDA recorders' `finalize()`
    /// calls, rather than each recorder holding its own cloned copy — on a
    /// large multi-file replay the raw message log can run into the
    /// billions of entries, so a second full copy (plus a second
    /// heap-allocated `user_id: String` per entry) was real, avoidable
    /// memory pressure. Both recorders only ever read this data.
    pub fn finalize(&self, messages: &[OrderMessage]) -> Vec<IntervalMetrics> {
        if self.interval_width == 0 {
            return Vec::new();
        }

        let mut min_ts = u64::MAX;
        let mut max_ts = 0u64;
        for m in messages {
            min_ts = min_ts.min(m.ts);
            max_ts = max_ts.max(m.ts);
        }
        for t in &self.trades {
            min_ts = min_ts.min(t.trade.ts);
            max_ts = max_ts.max(t.trade.ts);
        }
        for b in &self.batches {
            min_ts = min_ts.min(b.ts);
            max_ts = max_ts.max(b.ts);
        }
        for s in &self.books {
            min_ts = min_ts.min(s.ts);
            max_ts = max_ts.max(s.ts);
        }
        if min_ts == u64::MAX {
            return Vec::new(); // nothing recorded
        }

        let bucket_of = |ts: u64| -> u64 {
            let offset = ts.saturating_sub(min_ts);
            min_ts + (offset / self.interval_width) * self.interval_width
        };

        let n_buckets = (max_ts.saturating_sub(min_ts)) / self.interval_width + 1;
        let mut buckets: BTreeMap<u64, IntervalMetrics> = BTreeMap::new();
        for i in 0..n_buckets {
            let start = min_ts + i * self.interval_width;
            buckets.insert(start, IntervalMetrics::empty(self.engine.label(), start, self.interval_width));
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
            let b = bucket_of(trade.ts);
            let qty = trade.quantity as f64;
            let price = trade.price as f64;

            let entry = buckets.get_mut(&b).expect("bucket exists for every recorded ts");
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
            let entry = buckets.get_mut(b).unwrap();
            entry.intra_interval_price_dispersion = Some(stddev(prices));
        }

        // ---- Realized volatility & Amihud illiquidity: from the reference
        //      price series, grouped into the same buckets ----
        let mut bucket_price_points: HashMap<u64, Vec<f64>> = HashMap::new();
        for (ts, p) in &price_series {
            bucket_price_points.entry(bucket_of(*ts)).or_default().push(*p);
        }
        for (b, points) in &bucket_price_points {
            if points.len() >= 2 {
                let returns: Vec<f64> = points.windows(2).filter(|w| w[0] > 0.0).map(|w| (w[1] - w[0]) / w[0]).collect();
                if !returns.is_empty() {
                    buckets.get_mut(b).unwrap().realized_volatility = Some(stddev(&returns));
                }
            }
        }

        let mut bucket_close: BTreeMap<u64, f64> = BTreeMap::new();
        for (b, points) in &bucket_price_points {
            if let Some(&last) = points.last() {
                bucket_close.insert(*b, last);
            }
        }
        let mut prev_close: Option<f64> = None;
        for start in buckets.keys().cloned().collect::<Vec<_>>() {
            if let Some(&close) = bucket_close.get(&start) {
                if let Some(prev) = prev_close {
                    if prev > 0.0 {
                        let ret = (close - prev) / prev;
                        let entry = buckets.get_mut(&start).unwrap();
                        if entry.executed_volume > 0.0 {
                            entry.amihud_illiquidity = Some(ret.abs() / entry.executed_volume);
                        }
                    }
                }
                prev_close = Some(close);
            }
        }

        // ---- Messages: order-to-trade ratio, throughput input ----
        let mut bucket_msg_count: HashMap<u64, u64> = HashMap::new();
        for m in messages {
            *bucket_msg_count.entry(bucket_of(m.ts)).or_insert(0) += 1;
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
            let b = bucket_of(state.first_seen_ts);
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
            let b = bucket_of(s.ts);
            *bucket_compute_time.entry(b).or_insert(Duration::ZERO) += s.compute_time;
            *bucket_compute_count.entry(b).or_insert(0) += 1;
        }
        for bt in &self.batches {
            let b = bucket_of(bt.ts);
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
            bucket_batches.entry(bucket_of(bt.ts)).or_default().push(bt);
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
                let lo = msg_ts.partition_point(|&t| t < e.batch_open_ts);
                let hi = msg_ts.partition_point(|&t| t <= e.ts);
                total_msgs += (hi - lo) as u64;

                // `boundary_start >= e.batch_open_ts` always holds (it's
                // `e.ts - window/10` and `window/10 <= window`), so the
                // boundary sub-range sits inside `[lo, hi)` with no extra
                // clamping needed — matching the original nested
                // `m.ts >= boundary_start` check (itself already bounded
                // above by `m.ts <= e.ts`).
                let boundary_lo = msg_ts.partition_point(|&t| t < boundary_start);
                boundary_msgs += (hi - boundary_lo) as u64;
            }
            if total_msgs > 0 {
                entry.boundary_concentration = Some(boundary_msgs as f64 / total_msgs as f64);
            }
        }

        // ---- CDA-only: quoted spread, depth, book imbalance, from BookSnapshot ----
        let mut bucket_books: HashMap<u64, Vec<&BookSnapshot>> = HashMap::new();
        for s in &self.books {
            bucket_books.entry(bucket_of(s.ts)).or_default().push(s);
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

        buckets.into_values().collect()
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

/// Render a full time series as CSV text (header + one row per interval).
pub fn to_csv(series: &[IntervalMetrics]) -> String {
    let mut out = String::new();
    out.push_str(&csv_header());
    out.push('\n');
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
}

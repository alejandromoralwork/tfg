//! Turns the raw event log an engine run produces into the time-series
//! metric catalogue from docs/expose.tex. This is where the "engine
//! independent of metrics calculation" boundary lives: `MetricsCollector`
//! never touches `ContinuousEngine`/`BatchAuctionEngine` internals, it only
//! ever sees the plain events in `crate::events` that the simulation
//! harness constructs from an engine's public output.
//!
//! Units: every `ts` field anywhere in this crate is assumed to be
//! nanoseconds since the Unix epoch, matching the Hyperliquid L4 dataset's
//! own convention (see data/SCHEMA.md). The simulation harness is
//! responsible for keeping synthetic/CLI-generated timestamps in the same
//! units.

use crate::events::{
    BatchClearedEvent, BookSnapshot, EngineKind, OrderMessage, TradeEvent, DEPTH_BPS_THRESHOLDS,
};
use crate::interval::IntervalMetrics;
use engines::common::Side;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

const NS_PER_SEC: u64 = 1_000_000_000;
const REALIZED_SPREAD_HORIZONS_SECS: [u64; 3] = [1, 5, 30];

/// Per-order fill-tracking state, built once from the full message + trade
/// history at `finalize()` time (not incrementally — several metrics, like
/// fill rate, need to know an order's *eventual* outcome, which may only be
/// known once the whole run has been observed).
struct OrderState {
    user_id: String,
    limit_price: Option<u128>,
    orig_qty: u128,
    filled_qty: u128,
    first_seen_ts: u64,
    first_fill_ts: Option<u64>,
}

pub struct MetricsCollector {
    engine: EngineKind,
    interval_width: u64,
    messages: Vec<OrderMessage>,
    trades: Vec<TradeEvent>,
    batches: Vec<BatchClearedEvent>,
    books: Vec<BookSnapshot>,
}

impl MetricsCollector {
    /// `interval_width` is the bucket width in nanoseconds — pass the same
    /// value (matching your batch interval tau) when constructing a
    /// collector for each engine, so the two resulting time series sit on
    /// the same time grid (see docs/expose.tex, "Comparability Protocol").
    pub fn new(engine: EngineKind, interval_width_ns: u64) -> Self {
        Self {
            engine,
            interval_width: interval_width_ns,
            messages: Vec::new(),
            trades: Vec::new(),
            batches: Vec::new(),
            books: Vec::new(),
        }
    }

    pub fn record_message(&mut self, msg: OrderMessage) {
        self.messages.push(msg);
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

    fn build_order_states(&self) -> HashMap<u64, OrderState> {
        let mut states: HashMap<u64, OrderState> = HashMap::new();

        for m in &self.messages {
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
    pub fn finalize(&self) -> Vec<IntervalMetrics> {
        if self.interval_width == 0 {
            return Vec::new();
        }

        let mut min_ts = u64::MAX;
        let mut max_ts = 0u64;
        for m in &self.messages {
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

        let order_states = self.build_order_states();

        // Reference price series used for realized-spread markout lookups:
        // book midpoints (CDA) and batch clearing prices (FBA), sorted by ts.
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

        let price_at_or_after = |target_ts: u64| -> Option<f64> {
            price_series.iter().find(|(ts, _)| *ts >= target_ts).map(|(_, p)| *p)
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

                    let horizons = bucket_realized
                        .entry(b)
                        .or_insert([(0.0, 0.0); REALIZED_SPREAD_HORIZONS_SECS.len()]);
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
                let returns: Vec<f64> = points
                    .windows(2)
                    .filter(|w| w[0] > 0.0)
                    .map(|w| (w[1] - w[0]) / w[0])
                    .collect();
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
        for m in &self.messages {
            *bucket_msg_count.entry(bucket_of(m.ts)).or_insert(0) += 1;
        }
        for (b, count) in &bucket_msg_count {
            let entry = buckets.get_mut(b).unwrap();
            if entry.trade_count > 0 {
                entry.order_to_trade_ratio = Some(*count as f64 / entry.trade_count as f64);
            }
        }

        // ---- Fill rate, time to execution, order size inflation: bucketed
        //      by each order's own submission time ----
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
                buckets.get_mut(b).unwrap().avg_time_to_execution_secs =
                    Some(ttfs.iter().sum::<f64>() / ttfs.len() as f64);
            }
        }
        for (b, totals) in &bucket_user_totals {
            let ratios: Vec<f64> = totals
                .values()
                .filter(|(orig, _)| *orig > 0.0)
                .map(|(orig, filled)| orig / filled.max(1.0))
                .collect();
            if !ratios.is_empty() {
                buckets.get_mut(b).unwrap().order_size_inflation =
                    Some(ratios.iter().sum::<f64>() / ratios.len() as f64);
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
        let mut bucket_batches: HashMap<u64, Vec<&BatchClearedEvent>> = HashMap::new();
        for bt in &self.batches {
            bucket_batches.entry(bucket_of(bt.ts)).or_default().push(bt);
        }
        for (b, evs) in &bucket_batches {
            let entry = buckets.get_mut(b).unwrap();

            let spreads: Vec<f64> = evs
                .iter()
                .filter_map(|e| match (e.best_unfilled_buy, e.best_unfilled_sell, e.clearing_price) {
                    (Some(buy), Some(sell), Some(p)) if p > 0 => {
                        Some(((sell as f64 - buy as f64) / p as f64) * 10_000.0)
                    }
                    _ => None,
                })
                .collect();
            if !spreads.is_empty() {
                entry.quoted_spread_bps = Some(mean(&spreads));
            }

            let depths: Vec<f64> = evs
                .iter()
                .map(|e| (e.demand_at_price as f64 + e.supply_at_price as f64) / 2.0)
                .collect();
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
            // 10% of each batch's own window, across batches closing in this
            // bucket. Straightforward O(batches x messages) scan — fine at
            // thesis-simulation scale; replace with a sorted-timestamp
            // lookup if run against the full-scale dataset.
            let mut total_msgs = 0u64;
            let mut boundary_msgs = 0u64;
            for e in evs {
                let window = e.ts.saturating_sub(e.batch_open_ts);
                if window == 0 {
                    continue;
                }
                let boundary_start = e.ts.saturating_sub(window / 10);
                for m in &self.messages {
                    if m.ts >= e.batch_open_ts && m.ts <= e.ts {
                        total_msgs += 1;
                        if m.ts >= boundary_start {
                            boundary_msgs += 1;
                        }
                    }
                }
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

            let depths: Vec<f64> = snaps.iter().map(|s| (s.bid_depth as f64 + s.ask_depth as f64) / 2.0).collect();
            if !depths.is_empty() {
                entry.depth_at_best = Some(mean(&depths));
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
                    let total = s.bid_depth as f64 + s.ask_depth as f64;
                    if total > 0.0 {
                        Some((s.bid_depth as f64 - s.ask_depth as f64) / total)
                    } else {
                        None
                    }
                })
                .collect();
            if !imbalances.is_empty() {
                entry.book_imbalance = Some(mean(&imbalances));
            }
        }

        // pricing_error_bps intentionally left None everywhere: it needs an
        // external reference price feed (e.g. Hyperliquid's own oracle/mark
        // price) that isn't wired into the collector yet. See IntervalMetrics
        // doc comment.

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

use std::time::Instant;
use engines::common::{AssetPair, MatchingEngine, Order, Side, Trade, PRICE_SCALE};
use engines::cda::ContinuousEngine;
use metrics::{BookSnapshot, EngineKind, IntervalMetrics, MetricsCollector, OrderMessage, TradeEvent, DEPTH_BPS_THRESHOLDS};

// Mirrors FbaSimulator's structure so both engines are driven and measured
// the same way. Like FbaSimulator, ContinuousEngine itself never sees the
// MetricsCollector — this wrapper is the only thing that knows both exist.
pub struct CdaSimulator {
    pub engine: ContinuousEngine,
    pub order_id_counter: u64,
    pub global_order_history: Vec<Order>,
    pub global_trade_history: Vec<Trade>,
    pub metrics: MetricsCollector,
}

impl CdaSimulator {
    pub fn new(interval_width_ns: u64) -> Self {
        Self {
            engine: ContinuousEngine::new(),
            order_id_counter: 1,
            global_order_history: Vec::new(),
            global_trade_history: Vec::new(),
            metrics: MetricsCollector::new(EngineKind::Cda, interval_width_ns),
        }
    }

    pub fn metrics_series(&self) -> Vec<IntervalMetrics> {
        self.metrics.finalize()
    }

    pub fn add_order(&mut self, side: Side, raw_price: u128, qty: u128, user: String) -> Vec<Trade> {
        self.order_id_counter += 1;
        let pair = AssetPair::default();
        let internal_price = raw_price * PRICE_SCALE;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let order = Order::limit(self.order_id_counter, user, pair, side, internal_price, qty, timestamp);

        self.ingest(order)
    }

    /// Records the message, matches/rests the order, and snapshots the
    /// book — the part of `add_order` that doesn't care how the `Order`
    /// was built. Also the entry point used by the `load` CLI command (see
    /// `loader::load_order_status_csv`) to feed real historical orders
    /// (real `oid`/`ts`/`status_id`) through the exact same
    /// metrics/matching path, without duplicating this logic.
    pub fn ingest(&mut self, order: Order) -> Vec<Trade> {
        self.metrics.record_message(OrderMessage {
            ts: order.ts,
            oid: order.oid,
            user_id: order.user_id.clone(),
            side: order.side(),
            limit_price: order.limit_price(),
            quantity: order.orig_sz,
            accepted: order.is_new_live_order(),
        });

        // Pre-trade midpoint, captured before processing — the reference
        // price any resulting trades get measured against.
        let reference_price = {
            let book = self.engine.book_state();
            let best_bid = book.bids.first().and_then(|o| o.limit_price());
            let best_ask = book.asks.first().and_then(|o| o.limit_price());
            midpoint(best_bid, best_ask)
        };

        self.global_order_history.push(order.clone());

        let match_start = Instant::now();
        let trades = self.engine.process_order(order.clone());
        let compute_time = match_start.elapsed();

        for trade in &trades {
            self.metrics.record_trade(TradeEvent {
                trade: trade.clone(),
                reference_price,
                aggressor_side: Some(order.side()),
            });
        }
        self.global_trade_history.extend(trades.clone());

        // Post-trade book snapshot.
        let book = self.engine.book_state();
        let best_bid = book.bids.first().and_then(|o| o.limit_price());
        let best_ask = book.asks.first().and_then(|o| o.limit_price());
        let bid_depth: u128 = book.bids.iter().map(|o| o.remaining).sum();
        let ask_depth: u128 = book.asks.iter().map(|o| o.remaining).sum();
        let mid = midpoint(best_bid, best_ask);

        let depth_schedule = match mid {
            Some(m) => crate::depth::depth_schedule(
                m,
                book.bids
                    .iter()
                    .map(|o| (o.limit_price().unwrap_or(m), o.side(), o.remaining))
                    .chain(book.asks.iter().map(|o| (o.limit_price().unwrap_or(m), o.side(), o.remaining))),
            ),
            None => [(0, 0); DEPTH_BPS_THRESHOLDS.len()],
        };

        self.metrics.record_book_snapshot(BookSnapshot {
            ts: order.ts,
            best_bid,
            best_ask,
            bid_depth,
            ask_depth,
            depth_schedule,
            compute_time,
        });

        trades
    }
}

fn midpoint(best_bid: Option<u128>, best_ask: Option<u128>) -> Option<u128> {
    match (best_bid, best_ask) {
        (Some(b), Some(a)) => Some((b + a) / 2),
        (Some(b), None) => Some(b),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

//! Continuous Double Auction: every order is matched (or rested) the
//! instant it arrives, against a live resting book of bids/asks. Ported
//! from the inline matching logic proven out in the earlier
//! `thesis-market-models` workspace — no separate shared "orderbook"
//! abstraction, this engine just walks its own `bids`/`asks` directly.

use crate::types::{Amount, EngineKind, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

/// CDA's own orderbook: the live resting bids/asks, this engine's full
/// trade history, and the running state its own metric methods read from.
pub struct CdaOrderBook {
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
    pub executed_trades: Vec<Trade>,
    next_trade_id: u64,

    // Cumulative quantity ever submitted as live demand.
    total_submitted_qty: Amount,
    // Cumulative quantity actually matched, counted once per side (so a
    // trade of qty `q` adds `2*q` — `q` toward the buy order's own
    // fulfillment, `q` toward the sell order's — matching how
    // `total_submitted_qty` counts each order's quantity independently of
    // its side). Tracked directly rather than inferred as
    // "submitted - still resting": a market order that finds no (or only
    // partial) liquidity never rests, so it would otherwise vanish from
    // that subtraction without ever counting as unfilled.
    total_filled_qty: Amount,
}

fn get_price(kind: &OrderKind) -> Price {
    match kind {
        OrderKind::Limit { price } => *price,
        OrderKind::Market => PRICE_SCALE,
    }
}

/// Whether an incoming order (taker) can cross a resting order (maker) at
/// the maker's price. A market order on either side always crosses. The
/// crossing direction depends on which side the taker is on: a buy
/// crosses a resting ask when it's willing to pay at least the ask's
/// price (`buy_px >= ask_px`); a sell crosses a resting bid when it's
/// willing to accept at most the bid's price (`sell_px <= bid_px`) — the
/// OPPOSITE comparison, so `taker_side` can't be dropped in favor of a
/// single symmetric check.
fn check_price_match(taker_kind: &OrderKind, maker_kind: &OrderKind, taker_side: Side) -> bool {
    match (taker_kind, maker_kind) {
        (OrderKind::Market, _) | (_, OrderKind::Market) => true,
        (OrderKind::Limit { price: taker_px }, OrderKind::Limit { price: maker_px }) => match taker_side {
            Side::Buy => *taker_px >= *maker_px,
            Side::Sell => *taker_px <= *maker_px,
        },
    }
}

impl CdaOrderBook {
    pub fn new() -> Self {
        Self {
            bids: Vec::new(),
            asks: Vec::new(),
            executed_trades: Vec::new(),
            next_trade_id: 1,
            total_submitted_qty: 0,
            total_filled_qty: 0,
        }
    }

    /// Match the incoming order against the resting book (price-time
    /// priority), then rest whatever's left of a limit order. Returns the
    /// trades this single order produced (also appended to
    /// `self.executed_trades`).
    pub fn submit(&mut self, mut order: Order) -> Vec<Trade> {
        // A cancellation removes a still-resting order by oid (see
        // `Order::is_cancellation` for why `filled` events are
        // deliberately NOT handled the same way — this simulation's CDA
        // matching decides fills independently of whatever Hyperliquid's
        // own engine did).
        if order.is_cancellation() {
            self.cancel(order.oid);
            return Vec::new();
        }

        // Rejections and un-triggered conditional orders never touch the book.
        if !order.is_new_live_order() || order.remaining == 0 {
            return Vec::new();
        }

        self.total_submitted_qty = self.total_submitted_qty.saturating_add(order.remaining);

        let mut trades = Vec::new();

        match order.side() {
            Side::Buy => {
                while !self.asks.is_empty() && order.remaining > 0 {
                    let best_ask = &mut self.asks[0];
                    if !check_price_match(&order.kind(), &best_ask.kind(), Side::Buy) {
                        break;
                    }

                    let execution_price = get_price(&best_ask.kind());
                    let fill_qty = order.remaining.min(best_ask.remaining);
                    if fill_qty == 0 {
                        break;
                    }

                    order.reduce(fill_qty);
                    best_ask.reduce(fill_qty);
                    self.total_filled_qty = self.total_filled_qty.saturating_add(fill_qty * 2);

                    trades.push(Trade {
                        trade_id: self.next_trade_id,
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: order.user_id.clone(),
                        seller_id: best_ask.user_id.clone(),
                        buy_order_id: order.oid,
                        sell_order_id: best_ask.oid,
                        engine_type: EngineKind::Cda,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_ask.remaining == 0 {
                        self.asks.remove(0);
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    self.bids.push(order);
                    self.bids.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(0);
                        let p_b = b.limit_price().unwrap_or(0);
                        p_b.cmp(&p_a).then_with(|| a.ts.cmp(&b.ts)).then_with(|| a.oid.cmp(&b.oid))
                    });
                }
            }
            Side::Sell => {
                while !self.bids.is_empty() && order.remaining > 0 {
                    let best_bid = &mut self.bids[0];
                    if !check_price_match(&order.kind(), &best_bid.kind(), Side::Sell) {
                        break;
                    }

                    let execution_price = get_price(&best_bid.kind());
                    let fill_qty = order.remaining.min(best_bid.remaining);
                    if fill_qty == 0 {
                        break;
                    }

                    order.reduce(fill_qty);
                    best_bid.reduce(fill_qty);
                    self.total_filled_qty = self.total_filled_qty.saturating_add(fill_qty * 2);

                    trades.push(Trade {
                        trade_id: self.next_trade_id,
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: best_bid.user_id.clone(),
                        seller_id: order.user_id.clone(),
                        buy_order_id: best_bid.oid,
                        sell_order_id: order.oid,
                        engine_type: EngineKind::Cda,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_bid.remaining == 0 {
                        self.bids.remove(0);
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    self.asks.push(order);
                    self.asks.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(u128::MAX);
                        let p_b = b.limit_price().unwrap_or(u128::MAX);
                        p_a.cmp(&p_b).then_with(|| a.ts.cmp(&b.ts)).then_with(|| a.oid.cmp(&b.oid))
                    });
                }
            }
        }

        self.executed_trades.extend(trades.clone());
        trades
    }

    /// Removes a still-resting order by `oid` from either side of the
    /// book, if present. Returns whether anything was actually removed —
    /// a cancel for an `oid` this book never saw as live, or that already
    /// matched away, is a harmless no-op.
    pub fn cancel(&mut self, oid: u64) -> bool {
        let before = self.bids.len() + self.asks.len();
        self.bids.retain(|o| o.oid != oid);
        self.asks.retain(|o| o.oid != oid);
        (self.bids.len() + self.asks.len()) != before
    }

    // ---- Core metrics, computed on demand from this orderbook's own state ----

    pub fn best_bid(&self) -> Option<Price> {
        self.bids.first().and_then(|o| o.limit_price())
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.asks.first().and_then(|o| o.limit_price())
    }

    pub fn trade_count(&self) -> usize {
        self.executed_trades.len()
    }

    pub fn executed_volume(&self) -> Amount {
        self.executed_trades.iter().map(|t| t.quantity).sum()
    }

    pub fn executed_notional(&self) -> Amount {
        self.executed_trades
            .iter()
            .map(|t| t.quantity.saturating_mul(t.price) / PRICE_SCALE)
            .sum()
    }

    pub fn quoted_spread_bps(&self) -> Option<f64> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let mid = (bid as f64 + ask as f64) / 2.0;
        if mid == 0.0 {
            return None;
        }
        Some(((ask as f64 - bid as f64) / mid) * 10_000.0)
    }

    fn bid_depth(&self) -> Amount {
        self.bids.iter().map(|o| o.remaining).sum()
    }

    fn ask_depth(&self) -> Amount {
        self.asks.iter().map(|o| o.remaining).sum()
    }

    /// Total resting volume (bids + asks).
    pub fn depth_at_best(&self) -> Amount {
        self.bid_depth() + self.ask_depth()
    }

    /// `(bid_depth - ask_depth) / (bid_depth + ask_depth)` at the top of
    /// book — a CDA-only signal, since a batch auction has no resting book.
    pub fn book_imbalance(&self) -> Option<f64> {
        let bid = self.bid_depth();
        let ask = self.ask_depth();
        let total = bid + ask;
        if total == 0 {
            return None;
        }
        Some((bid as f64 - ask as f64) / total as f64)
    }

    /// Filled / submitted, across everything ever submitted to this book.
    pub fn fill_rate(&self) -> Option<f64> {
        if self.total_submitted_qty == 0 {
            return None;
        }
        Some(self.total_filled_qty as f64 / self.total_submitted_qty as f64)
    }
}

impl Default for CdaOrderBook {
    fn default() -> Self {
        Self::new()
    }
}

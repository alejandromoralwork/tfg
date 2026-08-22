//! Continuous Double Auction: every order is matched (or rested) the
//! instant it arrives, against a live resting book of bids/asks.
// Orderbook struct carries the live resting book, executed trades, 
// and metrics so that module metrics can calculate the metrics at running time on each snapshot.

use crate::types::{Amount, EngineKind, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

/// CDA's own orderbook: the live resting bids/asks, this engine's full
/// trade history, and the running state its own metric methods read from.

pub struct CdaOrderBook {
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
    pub executed_trades: Vec<Trade>,
    next_trade_id: u64,

    // ---- Bookkeeping for fill_rate() = total_filled_qty / total_submitted_qty ----
    //
    // total_submitted_qty: every live order that ever entered this book
    // adds its OWN quantity here, regardless of side. A buy for 6 and a
    // sell for 6 add 6 + 6 = 12 to this total, not 6 — each order's
    // demand/supply is counted independently, the same way a trader would
    // count "how much volume has been thrown at this book in total."
    //
    // total_filled_qty: every time a trade of quantity `q` executes, it
    // fills `q` worth of the buy order's demand AND `q` worth of the sell
    // order's supply — two separate quantities got satisfied, matching
    // the two separate quantities total_submitted_qty counted for them.
    // So each trade adds `q` twice here (`fill_qty * 2` at the call
    // sites), once per side, to stay on the same footing as
    // total_submitted_qty. If it only added `q` once, a book where
    // *everything* submitted eventually got fully matched would still
    // only reach fill_rate = 0.5, not 1.0.
    //
    // Worked example (see `cda_market_order_partial_liquidity_fill_rate`
    // in inputs/test_suite.rs for this as a real, running test):
    //   1. Jesus submits: sell 6 SOL @130 (limit)         total_submitted_qty:  0 -> 6
    //      Nothing resting to match against -> it just rests, unfilled so far.
    //   2. Alejandro submits:   buy  6 SOL (market order)        total_submitted_qty:  6 -> 12
    //      Alejandro's order fully matches Jesus's resting sell -> ONE trade, qty 6.
    //      That trade filled 6 of Jesus's supply AND 6 of Alejandro's demand:
    //                                                       total_filled_qty:    0 -> 6 (Jesus's side) -> 12 (Alejandro's side)
    //   3. Alvaro submits:   buy  3 SOL (market order)        total_submitted_qty: 12 -> 15
    //      Jesus's ask is gone, nothing left to match -> Alvaro's order finds
    //      NO liquidity. Market orders never rest (see `submit` below), so
    //      it doesn't sit on the book either — it just vanishes.
    //                                                       total_filled_qty:    stays 12
    //
    //   fill_rate = total_filled_qty / total_submitted_qty = 12 / 15 = 0.8
    //   — correctly says Jesus and Alejandro's orders fully filled, but Alvaro's 3
    //   units never traded.
    //
    // Why not derive this instead of tracking it separately? The tempting
    // shortcut is `filled = total_submitted_qty - still_resting_qty`
    // (`depth_at_best()`). That's exactly what `FbaOrderBook` does, and
    // it's correct there because nothing in FBA ever disappears without
    // either matching or staying queued for the next batch. It's WRONG
    // here: in step 3 above, Alvaro's order never rests, so it would vanish
    // from both sides of that subtraction — not counted as "still
    // resting," but also never actually filled — silently inflating
    // fill_rate to 12/12 = 1.0 instead of the correct 0.8. 
    // Tracking total_filled_qty directly, incremented at the
    // moment each trade happens, avoids that trap entirely.

    total_submitted_qty: Amount,
    total_filled_qty: Amount,
}

/// Picks the price a trade executes at. The standard continuous-matching
/// convention: whoever posted liquidity first sets the price, and an
/// incoming order's own price only ever decides whether it's eligible to
/// cross — never what it pays. This is also
/// what makes CDA's pricing different from FBA's, here every trade can
/// print at a different price (whatever each maker posted), where FBA
/// forces every trade in a batch to the same uniform clearing price.
///
/// `OrderKind::Limit { price } => *price` — correct and the only path
/// that's actually reachable: the maker's own posted price.
///
/// `OrderKind::Market => PRICE_SCALE` — this branch exists only so the
/// match is exhaustive; it can never actually run. A market order is
/// never rested (see `submit` below: only `OrderKind::Limit` orders ever
/// get pushed into `self.bids`/`self.asks`), so `best_ask`/`best_bid` —
/// and therefore whatever gets passed into this function — can never BE a
/// `Market` order. `PRICE_SCALE` here isn't a real price (it's the raw
/// fixed-point value for "1.000000", with no relation to where the
/// market is actually trading); it's a placeholder that happens to type-
/// check. If some future change ever let a market order rest, this
/// function would silently start returning that meaningless placeholder
/// as a real trade price instead of failing loudly — worth hardening
/// (e.g. `unreachable!()`) if that invariant ever becomes less obviously
/// true than it is today.

fn get_price(kind: &OrderKind) -> Price {
    match kind {
        OrderKind::Limit { price } => *price,
        OrderKind::Market => PRICE_SCALE,
    }
}

/// Whether an incoming order (/taker) can cross a resting order (maker) at
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

/// Sort key for `bids`: ascending order on this key gives price
/// DESCENDING (best/highest bid first, via `Reverse`), then ts ascending,
/// then oid ascending — identical ordering to the old
/// `p_b.cmp(&p_a).then(ts).then(oid)` comparator, just expressed as a key
/// so `partition_point`/binary search can use it directly.
fn bid_sort_key(o: &Order) -> (std::cmp::Reverse<u128>, u64, u64) {
    (std::cmp::Reverse(o.limit_price().unwrap_or(0)), o.ts, o.oid)
}

/// Sort key for `asks`: ascending on this key gives price ascending
/// (best/lowest ask first), then ts ascending, then oid ascending —
/// identical ordering to the old `p_a.cmp(&p_b).then(ts).then(oid)`.
fn ask_sort_key(o: &Order) -> (u128, u64, u64) {
    (o.limit_price().unwrap_or(u128::MAX), o.ts, o.oid)
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
        // `Order::is_cancellation` but `filled` events are
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

                //loop until ask side is empty or order is fully filled unless no match then stop
                while !self.asks.is_empty() && order.remaining > 0 {
                    let best_ask = &mut self.asks[0];//is a sorted vector, so the first element is the best ask
                    if !check_price_match(&order.kind(), &best_ask.kind(), Side::Buy) {
                        break;
                    } // if best ask and the incoming buy order do not match, break the loop

                    let execution_price = get_price(&best_ask.kind()); //kind() fun is called becausse get_price expects an OrderKind, and best_ask is an Order, so we need to get the kind of the best_ask order
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
                    // Binary-search for the insertion point rather than
                    // push+full-sort — `bids` is already sorted, so this
                    // needs only O(log n) comparisons instead of
                    // re-comparing the entire book on every single
                    // incoming order. At real-data scale (hundreds of
                    // thousands of resting orders) the O(n log n) full
                    // sort made `submit` effectively quadratic overall —
                    // this doesn't change what order the book ends up in,
                    // only how cheaply it gets there.
                    let key = bid_sort_key(&order);
                    let idx = self.bids.partition_point(|o| bid_sort_key(o) < key);
                    self.bids.insert(idx, order);
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
                    // Same binary-search insertion as the bid side above.
                    let key = ask_sort_key(&order);
                    let idx = self.asks.partition_point(|o| ask_sort_key(o) < key);
                    self.asks.insert(idx, order);
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

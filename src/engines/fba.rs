//! Frequent Batch Auction: collects orders into `pending_orders` and clears
//! them all at once at a single uniform price, chosen to maximize matched
//! volume. Ported from the closed-form clearing logic proven out in the
//! earlier `thesis-market-models` workspace — this project trades a single
//! fixed pair, so the uniform clearing price has a closed-form solution (a
//! scan over submitted limit prices); no external LP solver is needed.

use std::collections::{BTreeSet, HashMap};

use crate::types::{Amount, EngineKind, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

/// Output of a single batch clearing.
#[derive(Clone, Debug)]
pub struct ClearingResult {
    pub clearing_price: Price,
    pub traded_quantity: Amount,
    pub demand_at_price: Amount,
    pub supply_at_price: Amount,
    pub trades: Vec<Trade>,
}

/// FBA's own orderbook: the pending-order buffer waiting for the next
/// `clear()`, this engine's full trade history, and the running state its
/// own metric methods read from. Self-contained — a caller just calls
/// `submit`/`clear`/the metric getters, nothing else needs to reach inside.
pub struct FbaOrderBook {
    pub pending_orders: Vec<Order>,
    pub executed_trades: Vec<Trade>,
    pub last_clearing_price: Option<Price>,
    next_trade_id: u64,

    // Cumulative quantity ever submitted as live demand — combined with
    // whatever's still sitting in `pending_orders` right now, this is all
    // `fill_rate` needs (no separate order-state tracking required).
    total_submitted_qty: Amount,

    // Snapshot of the most recent clear()'s outcome, for
    // `unexecuted_residual_share`.
    last_demand_at_price: Amount,
    last_supply_at_price: Amount,
    last_unexecuted_quantity: Amount,
}

impl FbaOrderBook {
    pub fn new() -> Self {
        Self {
            pending_orders: Vec::new(),
            executed_trades: Vec::new(),
            last_clearing_price: None,
            next_trade_id: 1,
            total_submitted_qty: 0,
            last_demand_at_price: 0,
            last_supply_at_price: 0,
            last_unexecuted_quantity: 0,
        }
    }

    /// Rejections, cancellations, fill/lifecycle records, and un-triggered
    /// conditional orders never enter the batch — drop them here rather
    /// than let a naive replay of the raw L4 stream corrupt the batch with
    /// records that were never actually live demand.
    pub fn submit(&mut self, order: Order) {
        if order.is_new_live_order() {
            self.total_submitted_qty = self.total_submitted_qty.saturating_add(order.remaining);
            self.pending_orders.push(order);
        }
    }

    /// Clear the current batch at a single uniform price, rationing by
    /// price-time priority, and roll any unfilled residual straight back
    /// into `pending_orders` for the next batch. Fully self-contained.
    pub fn clear(&mut self) -> Option<ClearingResult> {
        let orders = std::mem::take(&mut self.pending_orders);
        if orders.is_empty() {
            return None;
        }

        let candidates = self.candidate_prices(&orders);
        let Some((clearing_price, demand_at_price, supply_at_price)) = self.select_price(&orders, candidates) else {
            // No candidate price to anchor on (e.g. an all-market-order
            // batch with no price history yet) — nothing executes. Put the
            // batch back rather than silently dropping it: `orders` was
            // already drained out of `self.pending_orders` above via
            // `mem::take`, so without this it would just vanish.
            self.pending_orders = orders;
            return None;
        };

        let mut buys = self.eligible_orders(&orders, Side::Buy, clearing_price);
        let mut sells = self.eligible_orders(&orders, Side::Sell, clearing_price);
        let mut trades = Vec::new();

        let batch_ts = orders.iter().map(|o| o.ts).max().unwrap_or(0);
        let mut buy_index = 0usize;
        let mut sell_index = 0usize;

        while buy_index < buys.len() && sell_index < sells.len() {
            let fill = buys[buy_index].remaining.min(sells[sell_index].remaining);
            if fill == 0 {
                if buys[buy_index].remaining == 0 { buy_index += 1; }
                if sells[sell_index].remaining == 0 { sell_index += 1; }
                continue;
            }

            let mut buy_order = buys[buy_index].clone();
            let mut sell_order = sells[sell_index].clone();
            buy_order.reduce(fill);
            sell_order.reduce(fill);
            buys[buy_index] = buy_order.clone();
            sells[sell_index] = sell_order.clone();

            trades.push(Trade {
                trade_id: self.next_trade_id,
                price: clearing_price,
                quantity: fill,
                buyer_id: buy_order.user_id.clone(),
                seller_id: sell_order.user_id.clone(),
                buy_order_id: buy_order.oid,
                sell_order_id: sell_order.oid,
                engine_type: EngineKind::Fba,
                ts: batch_ts,
                trade_tx_hash: None,
                chain_id: None,
            });
            self.next_trade_id += 1;

            if buys[buy_index].remaining == 0 { buy_index += 1; }
            if sells[sell_index].remaining == 0 { sell_index += 1; }
        }

        let traded_quantity: Amount = trades.iter().map(|t| t.quantity).sum();
        if traded_quantity > 0 {
            self.last_clearing_price = Some(clearing_price);
        }

        // Residual: whatever's left unfilled on the heavier side, computed
        // against the ORIGINAL batch (not the eligible-only clones above),
        // so orders that weren't even eligible at this price stay fully
        // intact instead of silently vanishing.
        let mut fills: HashMap<u64, Amount> = HashMap::new();
        for trade in &trades {
            *fills.entry(trade.buy_order_id).or_insert(0) += trade.quantity;
            *fills.entry(trade.sell_order_id).or_insert(0) += trade.quantity;
        }

        let mut residual_orders = Vec::new();
        for mut order in orders {
            let filled = fills.get(&order.oid).copied().unwrap_or(0);
            order.remaining = order.remaining.saturating_sub(filled);
            if order.remaining > 0 {
                residual_orders.push(order);
            }
        }
        self.last_demand_at_price = demand_at_price;
        self.last_supply_at_price = supply_at_price;
        // Deliberately NOT `residual_orders`' total remaining: that sum
        // includes orders that were never even eligible at this clearing
        // price (e.g. a buy limit below it), which can vastly outweigh
        // demand_at_price/supply_at_price (both eligible-only) and would
        // push the resulting share past 1. The heavier *eligible* side's
        // leftover is exactly `|demand - supply|` (since traded_quantity
        // is `min(demand, supply)`) — always within [0, max(demand, supply)],
        // so `unexecuted_residual_share` stays a genuine share.
        self.last_unexecuted_quantity = demand_at_price.abs_diff(supply_at_price);
        self.pending_orders = residual_orders;
        self.executed_trades.extend(trades.clone());

        Some(ClearingResult {
            clearing_price,
            traded_quantity,
            demand_at_price,
            supply_at_price,
            trades,
        })
    }

    /// Candidate clearing prices are exactly the submitted limit prices: the
    /// demand/supply step functions only change value at those points, so
    /// the volume-maximizing price is always achievable at one of them.
    fn candidate_prices(&self, orders: &[Order]) -> BTreeSet<Price> {
        let mut candidates = BTreeSet::new();
        for order in orders {
            if let Some(price) = order.limit_price() {
                candidates.insert(price);
            }
        }
        if candidates.is_empty() {
            // All-market-order batch: no price info of its own, anchor on
            // the last price this book actually cleared a trade at.
            if let Some(last_price) = self.last_clearing_price {
                candidates.insert(last_price);
            }
        }
        candidates
    }

    fn select_price(&self, orders: &[Order], candidates: BTreeSet<Price>) -> Option<(Price, Amount, Amount)> {
        let mut best: Option<(Price, Amount, Amount)> = None;

        for price in candidates {
            let demand = self.aggregate_volume(orders, Side::Buy, price);
            let supply = self.aggregate_volume(orders, Side::Sell, price);
            let volume = demand.min(supply);
            let imbalance = demand.abs_diff(supply);

            let better = match best {
                None => true,
                Some((best_price, best_demand, best_supply)) => {
                    let best_volume = best_demand.min(best_supply);
                    let best_imbalance = best_demand.abs_diff(best_supply);

                    if volume != best_volume {
                        volume > best_volume
                    } else if imbalance != best_imbalance {
                        imbalance < best_imbalance
                    } else {
                        // Final tie-break: prefer continuity with the last
                        // executed clearing price.
                        match self.last_clearing_price {
                            Some(reference) => {
                                let diff_new = price.abs_diff(reference);
                                let diff_best = best_price.abs_diff(reference);
                                diff_new < diff_best || (diff_new == diff_best && price < best_price)
                            }
                            None => price < best_price,
                        }
                    }
                }
            };

            if better {
                best = Some((price, demand, supply));
            }
        }

        best
    }

    fn aggregate_volume(&self, orders: &[Order], side: Side, price: Price) -> Amount {
        orders
            .iter()
            .filter(|order| order.side() == side)
            .filter(|order| match order.kind() {
                OrderKind::Market => true,
                OrderKind::Limit { price: limit_price } => match side {
                    Side::Buy => limit_price >= price,
                    Side::Sell => limit_price <= price,
                },
            })
            .map(|order| order.remaining)
            .sum()
    }

    /// Orders eligible to trade at `price`, sorted by price-time priority:
    /// most aggressive price first (market orders ahead of all limit
    /// orders), and among orders at the same price, earliest submission
    /// time first.
    fn eligible_orders(&self, orders: &[Order], side: Side, price: Price) -> Vec<Order> {
        let mut eligible: Vec<Order> = orders
            .iter()
            .filter(|order| order.side() == side)
            .filter(|order| match order.kind() {
                OrderKind::Market => true,
                OrderKind::Limit { price: limit_price } => match side {
                    Side::Buy => limit_price >= price,
                    Side::Sell => limit_price <= price,
                },
            })
            .cloned()
            .collect();

        eligible.sort_by_key(|order| {
            let aggressiveness = self.order_priority(order);
            (aggressiveness, order.ts, order.oid)
        });

        eligible
    }

    fn order_priority(&self, order: &Order) -> (u8, Price) {
        match (order.side(), order.kind()) {
            (Side::Buy, OrderKind::Market) | (Side::Sell, OrderKind::Market) => (0, 0),
            (Side::Buy, OrderKind::Limit { price }) => (1, u128::MAX - price),
            (Side::Sell, OrderKind::Limit { price }) => (1, price),
        }
    }

    // ---- Core metrics, computed on demand from this orderbook's own state ----

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

    pub fn best_unfilled_buy(&self) -> Option<Price> {
        self.pending_orders
            .iter()
            .filter(|o| o.side() == Side::Buy)
            .filter_map(|o| o.limit_price())
            .max()
    }

    pub fn best_unfilled_sell(&self) -> Option<Price> {
        self.pending_orders
            .iter()
            .filter(|o| o.side() == Side::Sell)
            .filter_map(|o| o.limit_price())
            .min()
    }

    /// Implied spread between the best unfilled buy/sell in the pending
    /// buffer (the FBA counterpart of a resting book's quoted spread).
    pub fn quoted_spread_bps(&self) -> Option<f64> {
        let buy = self.best_unfilled_buy()?;
        let sell = self.best_unfilled_sell()?;
        let reference = self.last_clearing_price.unwrap_or((buy + sell) / 2);
        if reference == 0 {
            return None;
        }
        Some(((sell as f64 - buy as f64) / reference as f64) * 10_000.0)
    }

    /// Total remaining volume currently sitting in the pending buffer.
    pub fn depth_at_best(&self) -> Amount {
        self.pending_orders.iter().map(|o| o.remaining).sum()
    }

    /// Filled / submitted, across everything ever submitted to this book.
    pub fn fill_rate(&self) -> Option<f64> {
        if self.total_submitted_qty == 0 {
            return None;
        }
        let still_pending: Amount = self.pending_orders.iter().map(|o| o.remaining).sum();
        let filled = self.total_submitted_qty.saturating_sub(still_pending);
        Some(filled as f64 / self.total_submitted_qty as f64)
    }

    /// Share of the heavier side's volume that went unexecuted at the last
    /// batch's clearing price.
    pub fn unexecuted_residual_share(&self) -> Option<f64> {
        let total_side = self.last_demand_at_price.max(self.last_supply_at_price);
        if total_side == 0 {
            return None;
        }
        Some(self.last_unexecuted_quantity as f64 / total_side as f64)
    }
}

impl Default for FbaOrderBook {
    fn default() -> Self {
        Self::new()
    }
}

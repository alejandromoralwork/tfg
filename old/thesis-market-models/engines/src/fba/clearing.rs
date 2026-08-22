use std::collections::BTreeSet;

// Standardized import block
use crate::common::{
    Amount, AssetPair, MatchingEngine, Order, OrderBookState,
    OrderKind, Price, Side, Trade
};

/// Output of a batch auction clearing. This simulation only ever trades a
/// single asset pair (SOL/USD — see AssetPair::default()), so there is no
/// per-pair routing: one batch produces at most one clearing result.
#[derive(Clone, Debug)]
pub struct ClearingResult {
    pub pair: AssetPair,
    pub clearing_price: Price,
    pub traded_quantity: Amount,
    pub demand_at_price: Amount,
    pub supply_at_price: Amount,
    pub trades: Vec<Trade>,
}

/// Frequent Batch Auction (FBA) engine, trading a single fixed asset pair.
///
/// Clearing follows the standard uniform-price call-auction rule:
///   1. The clearing price is the submitted limit price that maximizes
///      matched volume `min(demand, supply)`.

///   2. Ties on matched volume are broken by minimizing the leftover
///      imbalance `|demand - supply|`.


///   3. Remaining ties are broken by continuity with the last price this
///      engine actually cleared a trade at (minimizes artificial price
///      jumps between batches — the standard convention real call auctions
///      use, e.g. an opening cross referencing the prior close).
/// At the chosen price, orders are rationed by price-time priority: orders
/// strictly better than the clearing price fill in full; among orders
/// exactly at the clearing price (the marginal, "at the money" orders),
/// earlier submission time wins. See `order_priority`/`eligible_orders`.
#[derive(Debug)]
pub struct BatchAuctionEngine {
    pub orders: Vec<Order>,        // Orders collected during the current epoch
    pub book_viewer: OrderBookState, // Kept for MatchingEngine::book_state(); this
                                      // engine doesn't maintain a live resting book
                                      // the way the CDA does, so it's always empty.
    pub next_trade_id: u64,
    /// The clearing price of the most recent batch that actually executed a
    /// trade. Serves two purposes: (a) it's the reference price a batch
    /// falls back to when it contains no limit orders at all (an
    /// all-market-order batch has no price information of its own to
    /// discover a price from), and (b) it's used to break residual ties
    /// between candidate prices that tie on both matched volume and
    /// imbalance, preferring the price closest to it.
    pub last_clearing_price: Option<Price>,
}

impl BatchAuctionEngine {
    pub fn new() -> Self {
        Self {
            orders: Vec::new(),
            book_viewer: OrderBookState::default(),
            next_trade_id: 1,
            last_clearing_price: None,
        }
    }

    pub fn submit(&mut self, order: Order) {
        // Rejections, cancellations, fill/lifecycle records, and un-triggered
        // conditional orders never enter the batch — drop them here rather
        // than let a naive replay of the raw L4 stream corrupt the batch
        // with records that were never actually live demand.
        if order.is_new_live_order() {
            self.orders.push(order);
        }
    }

    /// Clear the current batch at a single uniform price. Any volume that
    /// cannot be matched on the heavier side of the book at that price is
    /// left unexecuted (see `simulation::FbaSimulator::clear_window`, which
    /// rolls it into the next batch).
    pub fn clear(&mut self) -> Option<ClearingResult> {
        let orders = std::mem::take(&mut self.orders);
        self.clear_orders(&orders)
    }

    pub fn clear_orders(&mut self, orders: &[Order]) -> Option<ClearingResult> {
        if orders.is_empty() {
            return None;
        }

        let pair = orders[0].pair.clone();
        // The batch's own "close" timestamp isn't tracked separately by this
        // engine — use the latest timestamp among the orders being cleared
        // as a reasonable proxy for when the batch closed.
        let batch_ts = orders.iter().map(|o| o.ts).max().unwrap_or(0);
        let candidate_prices = self.candidate_prices(orders);
        // No candidates means there were no limit orders in this batch AND no
        // prior clearing price to anchor on: there is no basis to discover a
        // price from, so this batch simply does not execute.
        let (clearing_price, demand_at_price, supply_at_price) = self.select_price(orders, candidate_prices)?;
        //please to check the file I have about auction price calculation



        let mut buys = self.eligible_orders(orders, Side::Buy, clearing_price);
        let mut sells = self.eligible_orders(orders, Side::Sell, clearing_price);
        let mut trades = Vec::new();

        let mut buy_index = 0usize;
        let mut sell_index = 0usize;

        //loop through the eligible buy and sell orders to match them and create trades
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
                pair: pair.clone(),
                price: clearing_price,
                quantity: fill,
                buyer_id: buy_order.user_id.clone(),
                seller_id: sell_order.user_id.clone(),
                buy_order_id: buy_order.oid,
                sell_order_id: sell_order.oid,
                ts: batch_ts,
                trade_tx_hash: None,
                chain_id: None,
            });
            self.next_trade_id += 1;

            if buys[buy_index].remaining == 0 { buy_index += 1; }
            if sells[sell_index].remaining == 0 { sell_index += 1; }
        }

        let traded_quantity = trades.iter().map(|trade| trade.quantity).sum();

        // Only adopt this as the new reference price if real volume traded —
        // a price selected for a batch that matched nothing isn't a
        // meaningful market price to anchor future batches on.
        if traded_quantity > 0 {
            self.last_clearing_price = Some(clearing_price);
        }

        Some(ClearingResult {
            pair,
            clearing_price,
            traded_quantity,
            demand_at_price,
            supply_at_price,
            trades,
        })
    }

    /// Candidate clearing prices are exactly the submitted limit prices: the
    /// demand/supply step functions only change value at those points, so
    /// the volume-maximizing price is always achievable at one of them
    /// (standard result for uniform-price call auctions — no other price
    /// needs to be considered).
    fn candidate_prices(&self, orders: &[Order]) -> BTreeSet<Price> {
        let mut candidates = BTreeSet::new();

        for order in orders {
            if let Some(price) = order.limit_price() {
                candidates.insert(price);
            }
        }

        if candidates.is_empty() {
            // No limit orders in this batch (e.g. an all-market-order batch):
            // there is no price information in the batch itself to discover a
            // clearing price from. Anchor on the last price this engine
            // actually cleared a trade at, if one exists.
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
                        // Final tie-break: prefer the price closest to the
                        // last executed clearing price, to avoid an
                        // artificial price jump that has no basis in the
                        // order flow itself. Falls back to the lower price
                        // deterministically if there's no history yet.
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
    /// orders, since they're willing to transact at any price), and among
    /// orders at the same price, earliest submission time first. This is
    /// what makes rationing at the margin fall out correctly from the
    /// sequential matching loop in `clear_orders`: strictly-better-priced
    /// orders are drained first and fill in full, so only the orders
    /// exactly at the clearing price are ever left partially unfilled, and
    /// among those the earliest-submitted ones are filled first.
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
}

// 🟢 IMPLEMENTATION: Match the CDA Engine trait signature
impl MatchingEngine for BatchAuctionEngine {
    fn process_order(&mut self, order: Order) -> Vec<Trade> {
        self.submit(order);
        Vec::new()
    }

    fn on_epoch_end(&mut self) -> Vec<Trade> {
        self.clear().map(|result| result.trades).unwrap_or_default()
    }

    fn book_state(&self) -> &OrderBookState {
        &self.book_viewer
    }
}

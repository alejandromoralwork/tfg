use std::collections::{BTreeMap, BTreeSet};

use crate::common::{AssetPair, Amount, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};
use crate::optimizer::{SettlementOptimizer, SettlementSummary};

#[derive(Clone, Debug)]
pub struct ClearingResult {
    pub pair: AssetPair,
    pub clearing_price: Price,
    pub traded_quantity: Amount,
    pub demand_at_price: Amount,
    pub supply_at_price: Amount,
    pub trades: Vec<Trade>,
}

#[derive(Clone, Debug)]
pub struct MultiAssetClearingResult {
    pub pair_results: Vec<ClearingResult>,
    pub settlement: SettlementSummary,
    pub leftover_batches: BTreeMap<AssetPair, Vec<Order>>,
}

#[derive(Default, Debug)]
pub struct MultiAssetEngine {
    batches: BTreeMap<AssetPair, Vec<Order>>,
    inner: BatchAuctionEngine,
    optimizer: SettlementOptimizer,
}

impl MultiAssetEngine {
    pub fn new() -> Self {
        Self {
            batches: BTreeMap::new(),
            inner: BatchAuctionEngine::new(),
            optimizer: SettlementOptimizer::new(),
        }
    }

    pub fn submit(&mut self, order: Order) {
        self.batches.entry(order.pair.clone()).or_default().push(order);
    }

    /// Clears with a pair-first heuristic: run pair clearing per pair to extract
    /// direct Coincidence-of-Wants trades, then pass collected trades to the
    /// settlement optimizer. Remaining unmatched order fragments are returned
    /// in `leftover_batches` for external routing.
    pub fn clear_all(&mut self) -> MultiAssetClearingResult {
        let pairs: Vec<AssetPair> = self.batches.keys().cloned().collect();
        let mut pair_results: Vec<ClearingResult> = Vec::new();
        let mut all_trades: Vec<Trade> = Vec::new();

        for pair in &pairs {
            if let Some(orders) = self.batches.get(pair).cloned() {
                // Use inner engine to compute a pair clearing (does not consume our map)
                if let Some(result) = self.inner.clear_orders(pair, &orders) {
                    // collect trades and result
                    all_trades.extend(result.trades.clone());
                    pair_results.push(result);
                }
            }
        }

        // Build leftover batches by subtracting filled amounts from original orders
        let mut leftover: BTreeMap<AssetPair, Vec<Order>> = BTreeMap::new();

        for pair in &pairs {
            if let Some(original_orders) = self.batches.get(pair).cloned() {
                // compute fills per order id
                let mut fills: BTreeMap<u64, Amount> = BTreeMap::new();
                for trade in &all_trades {
                    // buyer filled
                    *fills.entry(trade.buy_order_id).or_insert(0) += trade.quantity;
                    *fills.entry(trade.sell_order_id).or_insert(0) += trade.quantity;
                }

                let mut remaining_orders: Vec<Order> = Vec::new();
                for mut order in original_orders {
                    let filled = fills.get(&order.id).cloned().unwrap_or(0);
                    if filled >= order.remaining {
                        order.remaining = 0;
                    } else {
                        order.remaining = order.remaining.saturating_sub(filled);
                    }

                    if order.remaining > 0 {
                        remaining_orders.push(order);
                    }
                }

                if !remaining_orders.is_empty() {
                    leftover.insert(pair.clone(), remaining_orders);
                }
            }
        }

        let settlement = self.optimizer.optimize_trades(&all_trades);

        // replace engine batches with leftover (to be externally routed later)
        self.batches = leftover.clone();

        MultiAssetClearingResult {
            pair_results,
            settlement,
            leftover_batches: leftover,
        }
    }
}

#[derive(Default, Debug)]
pub struct BatchAuctionEngine {
    batches: BTreeMap<AssetPair, Vec<Order>>,
    next_trade_id: u64,
}

impl BatchAuctionEngine {
    pub fn new() -> Self {
        Self {
            batches: BTreeMap::new(),
            next_trade_id: 1,
        }
    }

    pub fn submit(&mut self, order: Order) {
        self.batches.entry(order.pair.clone()).or_default().push(order);
    }

    pub fn clear_pair(&mut self, pair: &AssetPair) -> Option<ClearingResult> {
        let orders = self.batches.remove(pair)?;
        self.clear_orders(pair, &orders)
    }

    pub fn clear_orders(&mut self, pair: &AssetPair, orders: &[Order]) -> Option<ClearingResult> {
        let candidate_prices = self.candidate_prices(orders);
        let (clearing_price, demand_at_price, supply_at_price) = self.select_price(orders, candidate_prices)?;

        let mut buys = self.eligible_orders(orders, Side::Buy, clearing_price);
        let mut sells = self.eligible_orders(orders, Side::Sell, clearing_price);
        let mut trades = Vec::new();

        let mut buy_index = 0usize;
        let mut sell_index = 0usize;

        while buy_index < buys.len() && sell_index < sells.len() {
            if buys[buy_index].participant_id == sells[sell_index].participant_id {
                if self.order_priority(&buys[buy_index]) <= self.order_priority(&sells[sell_index]) {
                    buy_index += 1;
                } else {
                    sell_index += 1;
                }
                continue;
            }

            let fill = buys[buy_index].remaining.min(sells[sell_index].remaining);
            if fill == 0 {
                if buys[buy_index].remaining == 0 {
                    buy_index += 1;
                }
                if sells[sell_index].remaining == 0 {
                    sell_index += 1;
                }
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
                buyer_id: buy_order.participant_id.clone(),
                seller_id: sell_order.participant_id.clone(),
                buy_order_id: buy_order.id,
                sell_order_id: sell_order.id,
            });
            self.next_trade_id += 1;

            if buys[buy_index].remaining == 0 {
                buy_index += 1;
            }
            if sells[sell_index].remaining == 0 {
                sell_index += 1;
            }
        }

        let traded_quantity = trades.iter().map(|trade| trade.quantity).sum();

        Some(ClearingResult {
            pair: pair.clone(),
            clearing_price,
            traded_quantity,
            demand_at_price,
            supply_at_price,
            trades,
        })
    }

    pub fn clear_all(&mut self) -> Vec<ClearingResult> {
        let pairs: Vec<AssetPair> = self.batches.keys().cloned().collect();
        let mut results = Vec::new();

        for pair in pairs {
            if let Some(result) = self.clear_pair(&pair) {
                results.push(result);
            }
        }

        results
    }

    fn candidate_prices(&self, orders: &[Order]) -> BTreeSet<Price> {
        let mut candidates = BTreeSet::new();
        let mut saw_limit = false;

        for order in orders {
            if let Some(price) = order.limit_price() {
                saw_limit = true;
                candidates.insert(price);
            }
        }

        if !saw_limit {
            candidates.insert(PRICE_SCALE);
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
                Some((best_price, best_volume, best_imbalance)) => {
                    volume > best_volume
                        || (volume == best_volume && imbalance < best_imbalance)
                        || (volume == best_volume && imbalance == best_imbalance && price > best_price)
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
            .filter(|order| order.side == side)
            .filter(|order| match order.kind {
                OrderKind::Market => true,
                OrderKind::Limit { price: limit_price } => match side {
                    Side::Buy => limit_price >= price,
                    Side::Sell => limit_price <= price,
                },
            })
            .map(|order| order.remaining)
            .sum()
    }

    fn eligible_orders(&self, orders: &[Order], side: Side, price: Price) -> Vec<Order> {
        let mut eligible: Vec<Order> = orders
            .iter()
            .filter(|order| order.side == side)
            .filter(|order| match order.kind {
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
            (aggressiveness, order.timestamp, order.id)
        });

        eligible
    }

    fn order_priority(&self, order: &Order) -> (u8, Price) {
        match (order.side, order.kind) {
            (Side::Buy, OrderKind::Market) | (Side::Sell, OrderKind::Market) => (0, 0),
            (Side::Buy, OrderKind::Limit { price }) => (1, u128::MAX - price),
            (Side::Sell, OrderKind::Limit { price }) => (1, price),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{AssetPair, Order, Side, PRICE_SCALE};

    #[test]
    fn clearing_price_maximizes_volume() {
        let pair = AssetPair::new("AAA", "USDC");
        let mut engine = BatchAuctionEngine::new();

        let orders = vec![
            Order::limit(1, "A", pair.clone(), Side::Buy, 11 * PRICE_SCALE, 10, 1),
            Order::limit(2, "B", pair.clone(), Side::Buy, 10 * PRICE_SCALE, 8, 2),
            Order::limit(3, "C", pair.clone(), Side::Sell, 9 * PRICE_SCALE, 9, 3),
            Order::limit(4, "D", pair.clone(), Side::Sell, 10 * PRICE_SCALE, 7, 4),
            Order::limit(5, "E", pair.clone(), Side::Sell, 11 * PRICE_SCALE, 6, 5),
        ];

        for order in orders {
            engine.submit(order);
        }

        let result = engine.clear_pair(&pair).expect("pair must clear");
        assert_eq!(result.clearing_price, 10 * PRICE_SCALE);
        assert_eq!(result.traded_quantity, 15);
        assert_eq!(result.demand_at_price, 18);
        assert_eq!(result.supply_at_price, 16);
    }
}

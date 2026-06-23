
use std::collections::{BTreeMap, BTreeSet};

// Standardized import block
use crate::common::{
    Amount, AssetPair, MatchingEngine, Order, OrderBookState, 
    OrderKind, Price, Side, Trade, PRICE_SCALE
};

// Optimizer remains separate as it is a sibling module within FBA
use crate::fba::optimizer::{SettlementOptimizer, SettlementSummary};



/// Output of a single-pair FBA (Frequent Batch Auction) clearing.
/// The clearing price maximizes traded volume (this maximizes the surplus) while respecting limit price constraints.
#[derive(Clone, Debug)]
pub struct ClearingResult {
    pub pair: AssetPair,
    /// The uniform clearing price at which all trades execute, should be constant.
    pub clearing_price: Price,
    /// Total quantity traded.
    pub traded_quantity: Amount,
    /// Total demand buy-side volume at the clearing price.
    pub demand_at_price: Amount,
    /// Total supply sell-side volume at the clearing price.
    pub supply_at_price: Amount,
    /// All trades executed at the clearing price.
    pub trades: Vec<Trade>,
}

/// Output of multi-asset batch auction clearing by pair.
#[derive(Clone, Debug)]
pub struct MultiAssetClearingResult {
    /// Per-pair clearing results; each pair clears independently.
    pub pair_results: Vec<ClearingResult>,
    /// Optimized settlement plan bundling net transfers across all trades.
    pub settlement: SettlementSummary,
    /// Orders with remaining quantity after clearing; can be routed elsewhere (COB, AMM, next batch).
    pub leftover_batches: BTreeMap<AssetPair, Vec<Order>>,
}

/// Multi-asset batch auction engine using a pair-first clearing heuristic.
#[derive(Default, Debug)]
pub struct MultiAssetEngine {
    pub batches: BTreeMap<AssetPair, Vec<Order>>,
    pub inner: BatchAuctionEngine,
    pub optimizer: SettlementOptimizer,
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

    pub fn clear_all(&mut self) -> MultiAssetClearingResult {
        let pairs: Vec<AssetPair> = self.batches.keys().cloned().collect();
        let mut pair_results: Vec<ClearingResult> = Vec::new();
        let mut all_trades: Vec<Trade> = Vec::new();

        for pair in &pairs {
            if let Some(orders) = self.batches.get(pair).cloned() {
                if let Some(result) = self.inner.clear_orders(pair, &orders) {
                    all_trades.extend(result.trades.clone());
                    pair_results.push(result);
                }
            }
        }

        let mut leftover: BTreeMap<AssetPair, Vec<Order>> = BTreeMap::new();

        for pair in &pairs {
            if let Some(original_orders) = self.batches.get(pair).cloned() {
                let mut fills: BTreeMap<u64, Amount> = BTreeMap::new();
                for trade in &all_trades {
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
        self.batches = leftover.clone();

        // Keep the inner batch tracking state synchronized
        self.inner.batches = leftover.clone();

        MultiAssetClearingResult {
            pair_results,
            settlement,
            leftover_batches: leftover,
        }
    }
}

/// Single-pair Frequent Batch Auction (FBA) engine.
#[derive(Default, Debug)]
pub struct BatchAuctionEngine {
    pub batches: BTreeMap<AssetPair, Vec<Order>>,
    pub book_viewer: OrderBookState, // Satisfies the common interface inspection contract
    pub next_trade_id: u64,
}

impl BatchAuctionEngine {
    pub fn new() -> Self {
        Self {
            batches: BTreeMap::new(),
            book_viewer: OrderBookState::new(),
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
                buyer_id: buy_order.participant_id.clone(),
                seller_id: sell_order.participant_id.clone(),
                buy_order_id: buy_order.id,
                sell_order_id: sell_order.id,
                trade_tx_hash: None,
                chain_id: None,
            });
            self.next_trade_id += 1;

            if buys[buy_index].remaining == 0 { buy_index += 1; }
            if sells[sell_index].remaining == 0 { sell_index += 1; }
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
                Some((best_price, best_demand, best_supply)) => {
                    let best_volume = best_demand.min(best_supply);
                    let best_imbalance = best_demand.abs_diff(best_supply);
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

// 🟢 NEW: Implement the matching engine simulation contract so it wires to simulator.rs automatically
impl MatchingEngine for BatchAuctionEngine {
    fn process_order(&mut self, order: Order) -> Vec<Trade> {
        self.submit(order);
        Vec::new() // Discrete matching returns empty sets during the collection step
    }

    fn on_epoch_end(&mut self) -> Vec<Trade> {
        let cross_results = self.clear_all();
        cross_results.into_iter().flat_map(|res| res.trades).collect()
    }

    fn book_state(&self) -> &OrderBookState {
        &self.book_viewer
    }
}
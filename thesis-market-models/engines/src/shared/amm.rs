use std::collections::HashMap;
use crate::common::{AssetPair, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

#[derive(Clone, Debug)]
pub struct AMMPool {
    pub pair: AssetPair,
    pub reserve_x: u128,
    pub reserve_y: u128,
}

/// A  non-overflowing bitwise integer square root algorithm.
/// Avoids the risks of division by zero or infinite loops .
fn u128_sqrt(value: u128) -> u128 {
    if value == 0 { return 0; }
    let mut res = 0;
    let mut add = 1u128 << 63; // Sqrt bit pointer (1 << 126 is the highest power of 4 fitting in u128)
    
    for _ in 0..64 {
        let temp = res | add;
        if let Some(sq) = temp.checked_mul(temp) {
            if sq <= value {
                res = temp;
            }
        }
        add >>= 1;
    }
    res
}

impl AMMPool {
    pub fn new(pair: AssetPair, reserve_x: u128, reserve_y: u128) -> Self {
        Self { pair, reserve_x, reserve_y }
    }

    pub fn price(&self) -> Price {
        if self.reserve_x == 0 { return u128::MAX; }
        (self.reserve_y.saturating_mul(PRICE_SCALE)) / self.reserve_x
    }

    pub fn apply_arbitrage(&mut self, target_price: Price) -> (u128, u128, bool) {
        if target_price == 0 || self.reserve_x == 0 || self.reserve_y == 0 {
            return (0, 0, false);
        }

        let current_p = self.price();
        if current_p == target_price {
            return (0, 0, false); 
        }

        let old_x = self.reserve_x;
        let old_y = self.reserve_y;
        
        // NOTE: If reserves are excessively large, k can saturate to u128::MAX.
        let k = old_x.saturating_mul(old_y);
        let numerator = k.saturating_mul(PRICE_SCALE);
        let x_target = u128_sqrt(numerator / target_price);
        
        if x_target == 0 { return (0, 0, false); }
        
        // Enforce a ceiling match on y_target to preserve or slightly grow the invariant k
        let mut y_target = k / x_target;
        if k % x_target != 0 {
            y_target = y_target.saturating_add(1);
        }

        self.reserve_x = x_target;
        self.reserve_y = y_target;

        if x_target > old_x {
            let dx = x_target - old_x;
            let dy = old_y.saturating_sub(y_target);
            (dx, dy, true)
        } else {
            let dx = old_x - x_target;
            let dy = y_target.saturating_sub(old_y);
            (dx, dy, false)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ArbitrageOracle {
    latest_prices: HashMap<AssetPair, Price>,
}

impl ArbitrageOracle {
    pub fn new() -> Self {
        Self { latest_prices: HashMap::new() }
    }

    pub fn observe_batch(&mut self, pair: &AssetPair, trades: &[Trade]) -> Option<Price> {
        if trades.is_empty() {
            return self.latest_prices.get(pair).cloned();
        }

        let mut total_volume: u128 = 0;
        let mut total_notional: u128 = 0; 

        for trade in trades {
            if &trade.pair == pair {
                total_volume = total_volume.saturating_add(trade.quantity);
                let trade_value = trade.quantity.saturating_mul(trade.price);
                total_notional = total_notional.saturating_add(trade_value);
            }
        }

        if total_volume > 0 {
            let batch_price = total_notional / total_volume;
            self.latest_prices.insert(pair.clone(), batch_price);
            Some(batch_price)
        } else {
            self.latest_prices.get(pair).cloned()
        }
    }

    pub fn seed_price(&mut self, pair: AssetPair, price: Price) {
        self.latest_prices.insert(pair, price);
    }

    pub fn get_price(&self, pair: &AssetPair) -> Option<Price> {
        self.latest_prices.get(pair).cloned()
    }
}

#[derive(Debug)]
pub struct ArbitrageEngine {
    pub oracle: ArbitrageOracle,
}

impl ArbitrageEngine {
    pub fn new() -> Self {
        Self { oracle: ArbitrageOracle::new() }
    }

    pub fn synchronize_amm(&mut self, pool: &mut AMMPool, latest_trades: &[Trade]) -> Option<Price> {
        let target_price = self.oracle.observe_batch(&pool.pair, latest_trades);
        if let Some(price) = target_price {
            pool.apply_arbitrage(price);
            Some(price)
        } else {
            None
        }
    }

    pub fn fill_residual_orders(
        &self, 
        pool: &mut AMMPool, 
        residual_orders: &mut Vec<Order>, 
        trade_id_counter: &mut u64
    ) -> Vec<Trade> {
        // Enforce deterministic priority queue execution
        residual_orders.sort_by(|a, b| {
            let a_market = matches!(a.kind, OrderKind::Market);
            let b_market = matches!(b.kind, OrderKind::Market);
            
            if a_market != b_market {
                return b_market.cmp(&a_market);
            }
            
            match (a.side, b.side) {
                (Side::Buy, Side::Buy) => {
                    let p_a = a.limit_price().unwrap_or(0);
                    let p_b = b.limit_price().unwrap_or(0);
                    p_b.cmp(&p_a)
                }
                (Side::Sell, Side::Sell) => {
                    let p_a = a.limit_price().unwrap_or(u128::MAX);
                    let p_b = b.limit_price().unwrap_or(u128::MAX);
                    p_a.cmp(&p_b)
                }
                (Side::Buy, Side::Sell) => std::cmp::Ordering::Less,
                (Side::Sell, Side::Buy) => std::cmp::Ordering::Greater,
            }
            .then_with(|| a.timestamp.cmp(&b.timestamp))
            .then_with(|| a.id.cmp(&b.id))
        });

        let mut amm_trades = Vec::new();
        let mut unfillable = Vec::new();

        for order in residual_orders.drain(..) {
            if order.remaining == 0 { continue; }

            match order.side {
                Side::Buy => {
                    if order.remaining >= pool.reserve_x {
                        unfillable.push(order);
                        continue;
                    }

                    let dx = order.remaining;
                    let numerator = pool.reserve_y.saturating_mul(dx);
                    let denominator = pool.reserve_x.saturating_sub(dx);
                    if denominator == 0 { unfillable.push(order); continue; }
                    
                    // 💡 PROTECTION RULE: Round UP for incoming funds (user pays the pool)
                    let dy = (numerator + denominator - 1) / denominator;
                    
                    let effective_price = match dy.checked_mul(PRICE_SCALE) {
                        Some(val) => val / dx,
                        None => u128::MAX,
                    };

                    if effective_price <= order.limit_price().unwrap_or(u128::MAX) {
                        pool.reserve_x = pool.reserve_x.saturating_sub(dx);
                        pool.reserve_y = pool.reserve_y.saturating_add(dy);
                        
                        *trade_id_counter += 1;
                        amm_trades.push(Trade {
                            trade_id: *trade_id_counter,
                            pair: pool.pair.clone(),
                            price: effective_price,
                            quantity: dx,
                            buyer_id: order.participant_id.clone(),
                            seller_id: "AMM_POOL".to_string(),
                            buy_order_id: order.id,
                            sell_order_id: 0,
                            trade_tx_hash: None,
                            chain_id: None,
                        });
                    } else {
                        unfillable.push(order);
                    }
                }
                Side::Sell => {
                    let dx = order.remaining;
                    let numerator = pool.reserve_y.saturating_mul(dx);
                    let denominator = pool.reserve_x.saturating_add(dx);
                    
                    // 💡 PROTECTION RULE: Round DOWN for outgoing funds (pool pays the user)
                    let dy = numerator / denominator;
                    if dy == 0 { unfillable.push(order); continue; }
                    
                    let effective_price = match dy.checked_mul(PRICE_SCALE) {
                        Some(val) => val / dx,
                        None => 0,
                    };

                    if effective_price >= order.limit_price().unwrap_or(0) {
                        pool.reserve_x = pool.reserve_x.saturating_add(dx);
                        pool.reserve_y = pool.reserve_y.saturating_sub(dy);

                        *trade_id_counter += 1;
                        amm_trades.push(Trade {
                            trade_id: *trade_id_counter,
                            pair: pool.pair.clone(),
                            price: effective_price,
                            quantity: dx,
                            buyer_id: "AMM_POOL".to_string(),
                            seller_id: order.participant_id.clone(),
                            buy_order_id: 0, 
                            sell_order_id: order.id,
                            trade_tx_hash: None,
                            chain_id: None,
                        });
                    } else {
                        unfillable.push(order);
                    }
                }
            }
        }

        *residual_orders = unfillable;
        amm_trades
    }
}

#[cfg(test)]
mod simulation_tests {
    use super::*;
    use crate::common::AssetPair;

    #[test]
    fn test_oracle_vwap_and_engine_sync() {
        let pair = AssetPair::new("ETH", "USDC");
        let mut pool = AMMPool::new(pair.clone(), 1_000_000, 2_500_000);
        let mut arb_engine = ArbitrageEngine::new();

        let mock_batch_trades = vec![
            Trade {
                trade_id: 1,
                pair: pair.clone(),
                price: 2_000_000, 
                quantity: 100_000,
                buyer_id: "UserA".into(),
                seller_id: "UserB".into(),
                buy_order_id: 101,
                sell_order_id: 201,
                trade_tx_hash: None,
                chain_id: None,
            },
            Trade {
                trade_id: 2,
                pair: pair.clone(),
                price: 2_000_000, 
                quantity: 50_000,
                buyer_id: "UserC".into(),
                seller_id: "UserD".into(),
                buy_order_id: 102,
                sell_order_id: 202,
                trade_tx_hash: None,
                chain_id: None,
            }
        ];

        let detected_price = arb_engine.synchronize_amm(&mut pool, &mock_batch_trades);
        assert_eq!(detected_price, Some(2_000_000));
        
        let tolerance = 5;
        let final_amm_price = pool.price();
        assert!((final_amm_price as i128 - 2_000_000i128).abs() <= tolerance);
    }

    #[test]
    fn test_empty_batch_retains_stale_price() {
        let pair = AssetPair::new("WBTC", "USDT");
        let mut arb_engine = ArbitrageEngine::new();
        arb_engine.oracle.seed_price(pair.clone(), 60_000_000);

        let empty_trades: Vec<Trade> = vec![];
        let price = arb_engine.oracle.observe_batch(&pair, &empty_trades);
        assert_eq!(price, Some(60_000_000));
    }
}
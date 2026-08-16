use std::collections::HashMap;
use std::time::Instant;
use engines::common::{AssetPair, Order, Trade, Side, PRICE_SCALE};
use engines::fba::{BatchAuctionEngine, SettlementOptimizer};
use metrics::{BatchClearedEvent, EngineKind, IntervalMetrics, MetricsCollector, OrderMessage, TradeEvent, DEPTH_BPS_THRESHOLDS};

// This simulation only ever trades a single asset pair — SOL/USD (see
// AssetPair::default()) — so there is no per-pair routing anywhere below.
//
// FbaSimulator drives the FBA engine AND independently records every order
// message / trade / batch outcome into a `MetricsCollector`. The engine
// itself (`BatchAuctionEngine`) has no idea this collector exists — see
// metrics::collector for that boundary.
pub struct FbaSimulator {
    pub batch_engine: BatchAuctionEngine,
    pub optimizer: SettlementOptimizer,
    pub pending_orders: Vec<Order>,
    pub global_order_history: Vec<Order>,
    pub global_trade_history: Vec<Trade>,
    pub order_id_counter: u64,
    pub trade_id_counter: u64,
    pub metrics: MetricsCollector,
}

impl FbaSimulator {
    pub fn new(interval_width_ns: u64) -> Self {
        Self {
            batch_engine: BatchAuctionEngine::new(),
            optimizer: SettlementOptimizer::new(),
            pending_orders: Vec::new(),
            global_order_history: Vec::new(),
            global_trade_history: Vec::new(),
            order_id_counter: 1,
            trade_id_counter: 0,
            metrics: MetricsCollector::new(EngineKind::Fba, interval_width_ns),
        }
    }

    pub fn metrics_series(&self) -> Vec<IntervalMetrics> {
        self.metrics.finalize()
    }

    pub fn add_order(&mut self, side: Side, raw_price: u128, qty: u128, user: String) -> u64 {
        let pair = AssetPair::default();

        // Calculate the internal scaled price
        let internal_price = raw_price * PRICE_SCALE;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let order = Order::limit(
            self.order_id_counter,
            user,
            pair,
            side,
            internal_price,
            qty,
            timestamp
        );

        self.metrics.record_message(OrderMessage {
            ts: order.ts,
            oid: order.oid,
            user_id: order.user_id.clone(),
            side: order.side(),
            limit_price: order.limit_price(),
            quantity: order.orig_sz,
            accepted: order.is_new_live_order(),
        });

        self.pending_orders.push(order.clone());
        self.global_order_history.push(order);

        let assigned_id = self.order_id_counter;
        self.order_id_counter += 1;
        assigned_id
    }

    pub fn clear_window(&mut self) {
        if self.pending_orders.is_empty() {
            println!("⚠️ Window buffer is empty. No discrete allocations can clear!");
            return;
        }

        println!("\n🔄 [FBA Window Closed] Computing Uniform Market Clearing...");
        println!("==============================================================");

        let original_orders: Vec<Order> = self.pending_orders.drain(..).collect();
        let batch_open_ts = original_orders.iter().map(|o| o.ts).min().unwrap_or(0);
        let batch_close_ts = original_orders.iter().map(|o| o.ts).max().unwrap_or(0);

        for order in original_orders.clone() {
            self.batch_engine.submit(order);
        }

        // Capture the reference price BEFORE clearing mutates it — this is
        // what any trades produced by this batch get measured against for
        // effective-spread purposes (see TradeEvent::reference_price docs).
        let reference_before = self.batch_engine.last_clearing_price;

        let clear_start = Instant::now();
        let clearing_opt = self.batch_engine.clear();
        let compute_time = clear_start.elapsed();

        if let Some(mut clearing) = clearing_opt {
            println!("✅ Uniform Clearing Calculated Successfully!");
            println!("   Execution Rate (Uniform Price) : {} USDT", crate::display::format_price(clearing.clearing_price));
            println!("   Total Executed Asset Mass       : {} units", clearing.traded_quantity);

            println!("\n📜 Detailed Execution Trade Log:");
            if clearing.trades.is_empty() {
                println!("   (No trades matched within this crossover threshold)");
            } else {
                for trade in &mut clearing.trades {
                    self.trade_id_counter += 1;
                    trade.trade_id = self.trade_id_counter;

                    println!("   Match ID #{:<3} | {} (Order #{}) bought {} units from {} (Order #{}) @ {} USDT",
                        trade.trade_id,
                        trade.buyer_id,
                        trade.buy_order_id,
                        trade.quantity,
                        trade.seller_id,
                        trade.sell_order_id,
                        crate::display::format_price(trade.price)
                    );
                }
            }

            for trade in &clearing.trades {
                self.metrics.record_trade(TradeEvent {
                    trade: trade.clone(),
                    reference_price: reference_before,
                    // No taker/maker distinction in a uniform-price batch
                    // auction — see TradeEvent::aggressor_side docs.
                    aggressor_side: None,
                });
            }
            self.global_trade_history.extend(clearing.trades.clone());

            let summary = self.optimizer.optimize_trades(&clearing.trades);
            println!("\n📦 Graph Settlement Optimization Performance:");
            println!("   Raw Bilateral Trades Count      : {}", clearing.trades.len());
            println!("   Compressed Structural Transfers : {}", summary.plan.optimized_transfer_count);

            let savings = if clearing.trades.len() > summary.plan.optimized_transfer_count {
                let total_trades = clearing.trades.len() as f64;
                let optimized_transfers = summary.plan.optimized_transfer_count as f64;
                ((total_trades - optimized_transfers) / total_trades) * 100.0
            } else {
                0.0
            };
            println!("   Blockchain Transaction Savings  : {:.1}%", savings);

            // Determine the residual: whatever remains unfilled on the heavier
            // side of the book at the clearing price. There is no external
            // liquidity source (e.g. an AMM) to absorb it — it simply stays
            // unexecuted and rolls over into the next batch window.
            let mut fills: HashMap<u64, u128> = HashMap::new();
            for trade in &clearing.trades {
                *fills.entry(trade.buy_order_id).or_insert(0) += trade.quantity;
                *fills.entry(trade.sell_order_id).or_insert(0) += trade.quantity;
            }

            // Depth-within-bps schedule computed from the batch's own order
            // list, around the clearing price — before it's consumed below.
            let depth_schedule = crate::depth::depth_schedule(
                clearing.clearing_price,
                original_orders.iter().map(|o| {
                    // Market orders have no limit price; treat them as
                    // resting exactly at the clearing price (distance 0),
                    // since they're willing to transact at any price.
                    (o.limit_price().unwrap_or(clearing.clearing_price), o.side(), o.remaining)
                }),
            );

            let mut residual_orders = Vec::new();
            for mut order in original_orders {
                let filled = fills.get(&order.oid).cloned().unwrap_or(0);
                if filled >= order.remaining {
                    order.remaining = 0;
                } else {
                    order.remaining = order.remaining.saturating_sub(filled);
                }

                if order.remaining > 0 {
                    residual_orders.push(order);
                }
            }

            let best_unfilled_buy = residual_orders.iter()
                .filter(|o| o.side() == Side::Buy)
                .filter_map(|o| o.limit_price())
                .max();
            let best_unfilled_sell = residual_orders.iter()
                .filter(|o| o.side() == Side::Sell)
                .filter_map(|o| o.limit_price())
                .min();
            let unexecuted_quantity: u128 = residual_orders.iter().map(|o| o.remaining).sum();

            self.metrics.record_batch(BatchClearedEvent {
                ts: batch_close_ts,
                batch_open_ts,
                clearing_price: Some(clearing.clearing_price),
                demand_at_price: clearing.demand_at_price,
                supply_at_price: clearing.supply_at_price,
                traded_quantity: clearing.traded_quantity,
                unexecuted_quantity,
                best_unfilled_buy,
                best_unfilled_sell,
                depth_schedule,
                compute_time,
            });

            if !residual_orders.is_empty() {
                println!("\n⏭️  {} order(s) left unexecuted at this clearing price — rolled over to the next window.", residual_orders.len());
                for order in residual_orders {
                    self.pending_orders.push(order);
                }
            }
        } else {
            // No clearing price could be determined for this batch (see
            // BatchAuctionEngine::clear_orders) — still record the attempt so
            // throughput/clearing-latency account for the wasted computation.
            let unexecuted_quantity: u128 = original_orders.iter().map(|o| o.remaining).sum();
            self.metrics.record_batch(BatchClearedEvent {
                ts: batch_close_ts,
                batch_open_ts,
                clearing_price: None,
                demand_at_price: 0,
                supply_at_price: 0,
                traded_quantity: 0,
                unexecuted_quantity,
                best_unfilled_buy: None,
                best_unfilled_sell: None,
                depth_schedule: [(0, 0); DEPTH_BPS_THRESHOLDS.len()],
                compute_time,
            });

            println!("❌ Convergence Failure: No mathematical crossover found inside this batch window.");
            for order in original_orders {
                self.pending_orders.push(order);
            }
        }
    }
}

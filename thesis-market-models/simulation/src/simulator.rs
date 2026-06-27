use std::collections::{HashMap, BTreeMap};
use std::time::{SystemTime, UNIX_EPOCH};
use engines::common::{AssetPair, Order, Trade, Side, PRICE_SCALE};
use engines::fba::{BatchAuctionEngine, SettlementOptimizer, AMMPool, ArbitrageEngine};

pub struct FbaSimulator {
    pub reference_ccy: String,
    pub batch_engine: BatchAuctionEngine,
    pub optimizer: SettlementOptimizer,
    pub arb_engine: ArbitrageEngine,
    pub amm_pools: HashMap<AssetPair, AMMPool>,
    pub pending_orders: Vec<Order>,
    pub global_order_history: Vec<Order>,
    pub global_trade_history: Vec<Trade>,
    pub order_id_counter: u64,
    pub trade_id_counter: u64,
}

impl FbaSimulator {
    pub fn new(reference_ccy: &str) -> Self {
        let mut sim = Self {
            reference_ccy: reference_ccy.to_string(),
            batch_engine: BatchAuctionEngine::new(),
            optimizer: SettlementOptimizer::new(),
            arb_engine: ArbitrageEngine::new(),
            amm_pools: HashMap::new(),
            pending_orders: Vec::new(),
            global_order_history: Vec::new(),
            global_trade_history: Vec::new(),
            order_id_counter: 1,
            trade_id_counter: 0,
        };
        sim.seed_default_pools();
        sim
    }

    fn seed_default_pools(&mut self) {
        let default_assets = vec![
            ("BTC", 65_000, 10, 650_000), //this are initial prices that we load into the ORACLE, so that if there is no trade history it fills order at this price.
            ("ETH", 3_500, 100, 350_000),
            ("SOL", 150, 1_000, 150_000),
        ];
        
        for (base, target_price, reserve_x, reserve_y) in default_assets {
            let pair = AssetPair::new(base, &self.reference_ccy);
            let pool = AMMPool::new(pair.clone(), reserve_x, reserve_y * PRICE_SCALE);
            self.amm_pools.insert(pair.clone(), pool);
            self.arb_engine.oracle.seed_price(pair.clone(), target_price * PRICE_SCALE);
        }
    }

   pub fn add_order(&mut self, side: Side, asset: &str, raw_price: u128, qty: u128, user: String) -> u64 {
    let pair = AssetPair::new(asset, &self.reference_ccy);
    
    // 1. Calculate the internal scaled price first
    let internal_price = raw_price * PRICE_SCALE;

    if !self.amm_pools.contains_key(&pair) {
        println!("🌱 [MARKET CREATION] Initializing new pool for {} at {} USDT...", pair.label(), raw_price);
        
        // 2. Define a starting liquidity depth (e.g., 100 units of the base asset)
        let initial_reserve_x = 100;
        
        // 3. The quote asset reserve MUST match the price formula: Y = X * Price
        // We multiply by PRICE_SCALE here to match your existing pool initialization math
        let initial_reserve_y = initial_reserve_x * raw_price * PRICE_SCALE; 
        
        // 4. Create the pool and seed the oracle with the dynamically calculated price
        let fallback_pool = AMMPool::new(pair.clone(), initial_reserve_x, initial_reserve_y);
        self.amm_pools.insert(pair.clone(), fallback_pool);
        self.arb_engine.oracle.seed_price(pair.clone(), internal_price);
    }

    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let order = Order::limit(
        self.order_id_counter, 
        user, 
        pair, 
        side, 
        internal_price, 
        qty, 
        timestamp
    );
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
        
        // 🟢 FIXED: Changed to BTreeMap to guarantee deterministic execution sequence for asset clusters
        let mut orders_by_pair: BTreeMap<AssetPair, Vec<Order>> = BTreeMap::new();
        for order in self.pending_orders.drain(..) {
            orders_by_pair.entry(order.pair.clone()).or_default().push(order);
        }

        for (pair, orders) in orders_by_pair {
            println!("\n📊 Processing Liquidity Matrix for Cluster: {}", pair.label());
            println!("==============================================================");
            
            let original_orders = orders.clone();

            for order in orders {
                self.batch_engine.submit(order);
            }

            if let Some(mut clearing) = self.batch_engine.clear_pair(&pair) {
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

                if let Some(pool) = self.amm_pools.get_mut(&pair) {
                    println!("\n⚖️ Automated Rebalancing Invariant Check:");
                    if let Some(target) = self.arb_engine.synchronize_amm(pool, &clearing.trades) {
                        println!("   AMM structural curve realigned to true target: {} USDT", crate::display::format_price(target));
                    }
                }

                let mut fills: HashMap<u64, u128> = HashMap::new();
                for trade in &clearing.trades {
                    *fills.entry(trade.buy_order_id).or_insert(0) += trade.quantity;
                    *fills.entry(trade.sell_order_id).or_insert(0) += trade.quantity;
                }

                let mut residual_orders = Vec::new();
                for mut order in original_orders {
                    let filled = fills.get(&order.id).cloned().unwrap_or(0);
                    if filled >= order.remaining {
                        order.remaining = 0;
                    } else {
                        order.remaining = order.remaining.saturating_sub(filled);
                    }

                    if order.remaining > 0 {
                        residual_orders.push(order);
                    }
                }

if !residual_orders.is_empty() {
    if let Some(pool) = self.amm_pools.get_mut(&pair) {
        let pool_price = pool.price();
        let oracle_price = self.arb_engine.oracle.get_price(&pair).unwrap_or(pool_price);

        // 1. Calculate absolute difference safely
        let diff = if pool_price > oracle_price {
            pool_price.saturating_sub(oracle_price)
        } else {
            oracle_price.saturating_sub(pool_price)
        };

        // 2. Cross-multiplication check: (diff * 100) > (oracle * 5)
        // This is mathematically equivalent to (diff/oracle) > 0.05 without integer truncation
        let is_deviation_too_high = oracle_price > 0 && 
            diff.saturating_mul(100) > oracle_price.saturating_mul(5);

        if is_deviation_too_high {
            println!("⚠️ SECURITY ALERT: AMM Pool price deviation > 5%. Blocking residual fill.");
            // Orders stay in 'residual_orders', so they will be pushed back to 'pending_orders' below
        } else {
            println!("🌊 Routing Leftover Residual Orders through AMM Pool Curve...");
            
            let amm_executed_trades = self.arb_engine.fill_residual_orders(
                pool, 
                &mut residual_orders, 
                &mut self.trade_id_counter
            );
            
            if amm_executed_trades.is_empty() {
                println!("   No leftover orders satisfied constant-product curve limit constraints.");
            } else {
                for trade in &amm_executed_trades {
                    let direction_str = if trade.buyer_id == "AMM_POOL" { "SELL" } else { "BUY" };
                    println!("   [AMM FILL] Match ID #{:<3} | {} routed order to AMM pool (Quantity: {} units @ Effective Rate: {} USDT)", 
                        trade.trade_id, direction_str, trade.quantity, crate::display::format_price(trade.price));
                }
                self.global_trade_history.extend(amm_executed_trades);
            }
        }
    }
}
                for order in residual_orders {
                    self.pending_orders.push(order);
                }

            } else {
                println!("❌ Convergence Failure: No mathematical crossover found inside this batch window.");
                for order in original_orders {
                    self.pending_orders.push(order);
                }
            }
        }
    }
}
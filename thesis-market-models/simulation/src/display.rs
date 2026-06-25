// 🟢 FIX 1: Import local FbaSimulator from simulation crate
use crate::simulator::FbaSimulator; 
// 🟢 FIX 2: Import common types from the engines library
use engines::common::{OrderKind, Side, PRICE_SCALE, Trade};

pub fn format_price(price: u128) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{whole}.{fractional:06}")
}

pub fn render_batch_buffer(sim: &FbaSimulator) {
    println!("--- 📋 Current Pending FBA Window Accumulation Buffer ---");
    if sim.pending_orders.is_empty() {
        println!("(No orders currently inside this discrete window buffer)");
    } else {
        for o in &sim.pending_orders {
            let side_str = if o.side == Side::Buy { "BUY " } else { "SELL" };
            let price_str = o.limit_price().map_or("MARKET".to_string(), |p| format_price(p));
            println!("ID: {:<3} | User: {:<8} | Pair: {:<8} | {} | Qty: {:<4} | Max Limit: {} USDT", 
                o.id, o.participant_id, o.pair.label(), side_str, o.remaining, price_str);
        }
    }
}

pub fn render_amm_matrices(sim: &FbaSimulator) {
    println!("--- ⚖️ Multi-Currency Liquidity Matrices ---");
    for (pair, pool) in &sim.amm_pools {
        println!("📍 Market Pair Cluster: {}", pair.label());
        println!("   Reserves      : {} {} / {} {}", 
            pool.reserve_x, pair.base, 
            format_price(pool.reserve_y), pair.quote);
        println!("   Marginal Rate : {} {}", format_price(pool.price()), pair.quote);
        if let Some(oracle_price) = sim.arb_engine.oracle.get_price(pair) {
            println!("   Oracle Target : {} {}", format_price(oracle_price), pair.quote);
        }
        println!("--------------------------------------");
    }
}

pub fn render_historical_ledger(sim: &FbaSimulator) {
    println!("\n==========================================================================");
    println!("📜                     SYSTEM HISTORICAL EXECUTION LOG                    ");
    println!("==========================================================================");

    println!("\n🛒 ALL HISTORICAL ORDERS SUBMITTED:");
    println!("--------------------------------------------------------------------------");
    if sim.global_order_history.is_empty() {
        println!("  No orders recorded inside tracking logs yet.");
    } else {
        println!("  {:<8} | {:<12} | {:<8} | {:<5} | {:<10} | {:<12}", "ID", "Participant", "Pair", "Side", "Total Qty", "Limit Price");
        println!("  ------------------------------------------------------------------------");
        for o in &sim.global_order_history {
            let side_str = format!("{:?}", o.side).to_uppercase();
            let limit_str = match o.kind {
                OrderKind::Market => "MARKET".to_string(),
                OrderKind::Limit { price } => format_price(price),
            };
            println!("  #{:<7} | {:<12} | {:<8} | {:<5} | {:<10} | {:<12}", 
                o.id, o.participant_id, o.pair.label(), side_str, o.quantity, limit_str);
        }
    }

    println!("\n⚖️ UNIFORM BATCH AUCTION TRADES (P2P CO-CLEARING):");
    println!("--------------------------------------------------------------------------");
    // 🟢 FIX 3: Use the imported Trade type directly
    let batch_trades: Vec<&Trade> = sim.global_trade_history
        .iter()
        .filter(|t| t.buyer_id != "AMM_POOL" && t.seller_id != "AMM_POOL")
        .collect();

    if batch_trades.is_empty() {
        println!("  No peer-to-peer uniform clearing trades have executed yet.");
    } else {
        println!("  {:<10} | {:<8} | {:<12} | {:<12} | {:<10} | {:<12}", "Trade ID", "Pair", "Buyer", "Seller", "Quantity", "Price");
        println!("  ------------------------------------------------------------------------");
        for t in batch_trades {
            println!("  #{:<9} | {:<8} | {:<12} | {:<12} | {:<10} | {:<12}", 
                t.trade_id, t.pair.label(), t.buyer_id, t.seller_id, t.quantity, format_price(t.price));
        }
    }

    println!("\n🌊 AMM POOL RESIDUAL LIQUIDITY SWAPS (BONDING CURVE FILLS):");
    println!("--------------------------------------------------------------------------");
    // 🟢 FIX 4: Use the imported Trade type directly
    let amm_trades: Vec<&Trade> = sim.global_trade_history
        .iter()
        .filter(|t| t.buyer_id == "AMM_POOL" || t.seller_id == "AMM_POOL")
        .collect();

    if amm_trades.is_empty() {
        println!("  No residual items have crossed the constant-product curve matrix path yet.");
    } else {
        println!("  {:<10} | {:<8} | {:<24} | {:<12} | {:<10} | {:<12}", "Trade ID", "Pair", "AMM Pool Direction", "Counterparty", "Quantity", "Curve Price");
        println!("  ------------------------------------------------------------------------");
        for t in amm_trades {
            let (action_direction, counterparty) = if t.buyer_id == "AMM_POOL" {
                ("POOL_BUY (User Liquidated)", &t.seller_id)
            } else {
                ("POOL_SELL (User Sapped)", &t.buyer_id)
            };

            println!("  #{:<9} | {:<8} | {:<24} | {:<12} | {:<10} | {:<12}", 
                t.trade_id, t.pair.label(), action_direction, counterparty, t.quantity, format_price(t.price));
        }
    }
    println!("\n==========================================================================\n");
}
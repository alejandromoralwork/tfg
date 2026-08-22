
use crate::simulator::FbaSimulator;
use crate::cda_simulator::CdaSimulator;
use engines::common::{OrderKind, Side, PRICE_SCALE};

pub fn format_price(price: u128) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{whole}.{fractional:06}")
}

pub fn render_batch_buffer(sim: &FbaSimulator) {
    println!("---  Current Pending FBA Window Accumulation Buffer ---");
    if sim.pending_orders.is_empty() {
        println!("(No orders currently inside this discrete window buffer)");
    } else {
        for o in &sim.pending_orders {
            let side_str = if o.side() == Side::Buy { "BUY " } else { "SELL" };
            let price_str = o.limit_price().map_or("MARKET".to_string(), |p| format_price(p));
            println!("ID: {:<3} | User: {:<8} | Pair: {:<8} | {} | Qty: {:<4} | Max Limit: {} USDT",
                o.oid, o.user_id, o.pair.label(), side_str, o.remaining, price_str);
        }
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
            let side_str = format!("{:?}", o.side()).to_uppercase();
            let limit_str = match o.kind() {
                OrderKind::Market => "MARKET".to_string(),
                OrderKind::Limit { price } => format_price(price),
            };
            println!("  #{:<7} | {:<12} | {:<8} | {:<5} | {:<10} | {:<12}",
                o.oid, o.user_id, o.pair.label(), side_str, o.orig_sz, limit_str);
        }
    }

    println!("\n⚖️ UNIFORM BATCH AUCTION TRADES (P2P CO-CLEARING):");
    println!("--------------------------------------------------------------------------");

    if sim.global_trade_history.is_empty() {
        println!("  No peer-to-peer uniform clearing trades have executed yet.");
    } else {
        println!("  {:<10} | {:<8} | {:<12} | {:<12} | {:<10} | {:<12}", "Trade ID", "Pair", "Buyer", "Seller", "Quantity", "Price");
        println!("  ------------------------------------------------------------------------");
        for t in &sim.global_trade_history {
            println!("  #{:<9} | {:<8} | {:<12} | {:<12} | {:<10} | {:<12}",
                t.trade_id, t.pair.label(), t.buyer_id, t.seller_id, t.quantity, format_price(t.price));
        }
    }
    println!("\n==========================================================================\n");
}

/// Prints the RQ2 metric time series computed so far for both engines
/// side by side — the same time-grid comparison docs/expose.tex's
/// Comparability Protocol calls for. `metrics::report::to_csv` is what a
/// real replay run should use to export the full 25-column table for
/// analysis outside the simulator.
pub fn render_metrics(fba: &FbaSimulator, cda: &CdaSimulator) {
    println!("\n==========================================================================");
    println!("📈                       RQ2 METRIC TIME SERIES                            ");
    println!("==========================================================================");

    let fba_series = fba.metrics_series();
    let cda_series = cda.metrics_series();

    println!("\n--- FBA ({} interval buckets) ---", fba_series.len());
    if fba_series.is_empty() {
        println!("  (No batches cleared yet — run 'clear' at least once.)");
    } else {
        print!("{}", metrics::report::to_summary_table(&fba_series));
    }

    println!("\n--- CDA ({} interval buckets) ---", cda_series.len());
    if cda_series.is_empty() {
        println!("  (No orders processed yet.)");
    } else {
        print!("{}", metrics::report::to_summary_table(&cda_series));
    }

    println!("\n(Full metric catalogue, including realized/effective spread, price impact,");
    println!(" depth-within-bps, order-to-trade ratio, and clearing latency, is available");
    println!(" via metrics::report::to_csv() on the same series for export/analysis.)");
    println!("==========================================================================\n");
}
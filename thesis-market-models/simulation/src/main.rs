use engines::common::{AssetPair, Order, Side, PRICE_SCALE};
use engines::cob::OrderBook;
use engines::fba::{BatchAuctionEngine, SettlementOptimizer};

fn main() {
    let pair = AssetPair::new("TOKENA", "USDC");
    let mut batch_engine = BatchAuctionEngine::new();

    let orders = vec![
        Order::limit(1, "Alice", pair.clone(), Side::Buy, 11 * PRICE_SCALE, 10, 1),
        Order::limit(2, "Bob", pair.clone(), Side::Buy, 10 * PRICE_SCALE, 8, 2),
        Order::limit(3, "Carol", pair.clone(), Side::Sell, 9 * PRICE_SCALE, 9, 3),
        Order::limit(4, "Dave", pair.clone(), Side::Sell, 10 * PRICE_SCALE, 7, 4),
        Order::limit(5, "Erin", pair.clone(), Side::Buy, 10 * PRICE_SCALE, 6, 5),
        Order::limit(6, "Erin", pair.clone(), Side::Sell, 10 * PRICE_SCALE, 6, 6),
    ];

    for order in orders {
        batch_engine.submit(order);
    }

    let clearing = batch_engine.clear_pair(&pair).expect("pair should clear");

    println!("FBA mono-asset batch: {}", pair.label());
    println!("Clearing price: {}", format_price(clearing.clearing_price));
    println!("Demand at price: {}", clearing.demand_at_price);
    println!("Supply at price: {}", clearing.supply_at_price);
    println!("Executed quantity: {}", clearing.traded_quantity);
    println!("Executed trades: {}", clearing.trades.len());

    let optimizer = SettlementOptimizer::new();
    let summary = optimizer.optimize_trades(&clearing.trades);

    println!("Naive transfer legs: {}", summary.plan.naive_transfer_count);
    println!("Optimized settlement edges: {}", summary.plan.optimized_transfer_count);

    for edge in &summary.plan.edges {
        println!("{} -> {}", edge.from, edge.to);
        for transfer in &edge.transfers {
            println!("  {} {}", transfer.amount, transfer.asset);
        }
    }

    let mut cob = OrderBook::new(pair.clone());
    let live_trade_1 = cob.submit(Order::limit(10, "Fiona", pair.clone(), Side::Buy, 10 * PRICE_SCALE, 4, 10));
    let live_trade_2 = cob.submit(Order::limit(11, "George", pair.clone(), Side::Sell, 10 * PRICE_SCALE, 4, 11));
    println!("COB example trade count: {}", live_trade_1.len() + live_trade_2.len());
}

fn format_price(price: u128) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{whole}.{fractional:06}")
}

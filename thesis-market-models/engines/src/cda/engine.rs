use std::collections::HashMap;
use crate::common::{
    Amount, AssetPair, MatchingEngine, Order, OrderBookState, 
    OrderKind, Price, Side, Trade, PRICE_SCALE
};

#[derive(Debug)]
pub struct ContinuousEngine {
    // Used a HashMap to store the books per AssetPair
    pub books: HashMap<AssetPair, OrderBookState>,
    pub next_trade_id: u64,
}

impl ContinuousEngine {
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
            next_trade_id: 1,
        }
    }
}

// Extract price safely and convert into u128
fn get_price(kind: &OrderKind) -> u128 {
    match kind {
        OrderKind::Limit { price } => *price,
        OrderKind::Market => PRICE_SCALE,
    }
}

// Check if prices allow a match
fn check_price_match(order_kind: &OrderKind, maker_kind: &OrderKind) -> bool {
    match (order_kind, maker_kind) {
        (OrderKind::Market, _) | (_, OrderKind::Market) => true,
        (OrderKind::Limit { price: p1 }, OrderKind::Limit { price: p2 }) => *p1 >= *p2,
    }
}

// pass CE to the main trait
impl MatchingEngine for ContinuousEngine {
    fn process_order(&mut self, mut order: Order) -> Vec<Trade> {
        let mut executed_trades = Vec::new();
        if order.remaining == 0 {
            return executed_trades;
        }

        // Fetch the specific book from the HashMap
        // I use entry/or_insert_with to ensure the book is created if it's the first time we see this pair
        let pair_book = self.books
            .entry(order.pair.clone())
            .or_insert_with(|| OrderBookState::new(order.pair.clone()));

        match order.side {
            Side::Buy => {
                while !pair_book.asks.is_empty() && order.remaining > 0 {


                    //for buy we look at the best aks, and if our order prices is higher or equalt than the ask it goes in

                    let best_ask = &mut pair_book.asks[0];

                    if !check_price_match(&order.kind, &best_ask.kind) { break; }

                    let execution_price = get_price(&best_ask.kind);
                    let fill_qty = order.remaining.min(best_ask.remaining); //to get the fill quantity we take the minimum of the remaining quantity of the order and the best ask
                    
                    if fill_qty == 0 { break; }

                    order.reduce(fill_qty);
                    best_ask.reduce(fill_qty);

                    executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        pair: order.pair.clone(),
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: order.participant_id.clone(),
                        seller_id: best_ask.participant_id.clone(),
                        buy_order_id: order.id,
                        sell_order_id: best_ask.id,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_ask.remaining == 0 {
                        pair_book.asks.remove(0); 
                    }
                }

                if order.remaining > 0 && matches!(order.kind, OrderKind::Limit { .. }) {
                    pair_book.bids.push(order);
                    pair_book.bids.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(0);
                        let p_b = b.limit_price().unwrap_or(0);
                        p_b.cmp(&p_a)
                            .then_with(|| a.timestamp.cmp(&b.timestamp))
                            .then_with(|| a.id.cmp(&b.id))
                    });
                }
            }
            Side::Sell => {
                while !pair_book.bids.is_empty() && order.remaining > 0 {
                    let best_bid = &mut pair_book.bids[0];

                    if !check_price_match(&best_bid.kind, &order.kind) { break; }

                    let execution_price = get_price(&best_bid.kind);
                    let fill_qty = order.remaining.min(best_bid.remaining);
                    
                    if fill_qty == 0 { break; }

                    order.reduce(fill_qty);
                    best_bid.reduce(fill_qty);

                    executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        pair: order.pair.clone(),
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: best_bid.participant_id.clone(),
                        seller_id: order.participant_id.clone(),
                        buy_order_id: best_bid.id,
                        sell_order_id: order.id,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_bid.remaining == 0 {
                        pair_book.bids.remove(0);
                    }
                }

                if order.remaining > 0 && matches!(order.kind, OrderKind::Limit { .. }) {
                    pair_book.asks.push(order);
                    pair_book.asks.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(u128::MAX);
                        let p_b = b.limit_price().unwrap_or(u128::MAX);
                        p_a.cmp(&p_b)
                            .then_with(|| a.timestamp.cmp(&b.timestamp))
                            .then_with(|| a.id.cmp(&b.id))
                    });
                }
            }
        }

        executed_trades
    }

    fn on_epoch_end(&mut self) -> Vec<Trade> {
        Vec::new() 
    }

    // This trait method return type may need adjustment in the trait definition
    // to match the fact that we now hold a HashMap.
    fn book_state(&self) -> &HashMap<AssetPair, OrderBookState> {
        &self.books
    }
}
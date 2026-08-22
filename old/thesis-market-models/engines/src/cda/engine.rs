use crate::common::{
    MatchingEngine, Order, OrderBookState,
    OrderKind, Side, Trade, PRICE_SCALE
};

#[derive(Debug)]
pub struct ContinuousEngine { // main class for the continuous matching engine
    // This simulation only ever trades one asset pair (SOL/USD — see
    // AssetPair::default()), so a single book is all that's needed.
    pub book: OrderBookState,
    pub next_trade_id: u64,
}

impl ContinuousEngine {
    pub fn new() -> Self {
        Self {
            book: OrderBookState::default(),
            next_trade_id: 1, // is set to 1 the first entry when init the instance of the engine
        }
    }
}

// Extract price safely and convert into u128, to check if is profitable to attach these funs to orderkind class
fn get_price(kind: &OrderKind) -> u128 {
    match kind {
        OrderKind::Limit { price } => *price,
        OrderKind::Market => PRICE_SCALE, // this is of the price scale is so that i dont need to work with decimals for safe computation and avoid rounding errors
    }
}

// Check if there is a match between two orders taker vs maker
fn check_price_match(order_kind: &OrderKind, maker_kind: &OrderKind) -> bool {
    match (order_kind, maker_kind) {
        (OrderKind::Market, _) | (_, OrderKind::Market) => true, //if any of maker or taker order is a market order then is true
        (OrderKind::Limit { price: p1 }, OrderKind::Limit { price: p2 }) => *p1 >= *p2, //if both are limit; then taker should be equal or higher in price so that it exist a match;
    }
}

// pass CE to the main trait
impl MatchingEngine for ContinuousEngine {
    fn process_order(&mut self, mut order: Order) -> Vec<Trade> {
        let mut executed_trades = Vec::new();

        // Rejections, cancellations, fill/lifecycle records, and un-triggered
        // conditional orders never touch the book — drop them here rather
        // than let a naive replay of the raw L4 stream corrupt the book with
        // records that were never actually live demand.
        if !order.is_new_live_order() {
            return executed_trades;
        }
        if order.remaining == 0 {
            return executed_trades;//this an error check in case there are orders with zero amount
        }

        let book = &mut self.book;

        match order.side() {
            Side::Buy => {//for buy orders....
                while !book.asks.is_empty() && order.remaining > 0 {


                    // When an order comes into the engine, three things can happen:

                    // It matches with nothing: It just sits on the book. (Returns 0 trades).

                    // It matches perfectly with one order: (Returns 1 trade).

                    // It is huge and eats through multiple smaller orders: If you submit a market buy for 10 SOL, you might buy 2 SOL from Alice, 3 from Bob, and 5 from Charlie. (Returns 3 trades).


                    //for buy we look at the best aks, and if our order prices is higher or equalt than the ask it goes in

                    let best_ask = &mut book.asks[0];//normally the list of ask should be ordered by best price that is why we get the zero index

                    if !check_price_match(&order.kind(), &best_ask.kind()) { break; }

                    let execution_price = get_price(&best_ask.kind());
                    let fill_qty = order.remaining.min(best_ask.remaining); //to get the fill quantity we take the minimum of the remaining quantity of the order and the best ask

                    if fill_qty == 0 { break; }

                    order.reduce(fill_qty);
                    best_ask.reduce(fill_qty);

                    executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        pair: order.pair.clone(),
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: order.user_id.clone(),
                        seller_id: best_ask.user_id.clone(),
                        buy_order_id: order.oid,
                        sell_order_id: best_ask.oid,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_ask.remaining == 0 {
                        book.asks.remove(0);
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    book.bids.push(order);
                    book.bids.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(0);
                        let p_b = b.limit_price().unwrap_or(0);
                        p_b.cmp(&p_a)
                            .then_with(|| a.ts.cmp(&b.ts))
                            .then_with(|| a.oid.cmp(&b.oid))
                    });
                }
            }
            Side::Sell => {
                while !book.bids.is_empty() && order.remaining > 0 {
                    let best_bid = &mut book.bids[0];

                    if !check_price_match(&best_bid.kind(), &order.kind()) { break; }

                    let execution_price = get_price(&best_bid.kind());
                    let fill_qty = order.remaining.min(best_bid.remaining);

                    if fill_qty == 0 { break; }

                    order.reduce(fill_qty);
                    best_bid.reduce(fill_qty);

                    executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        pair: order.pair.clone(),
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: best_bid.user_id.clone(),
                        seller_id: order.user_id.clone(),
                        buy_order_id: best_bid.oid,
                        sell_order_id: order.oid,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if best_bid.remaining == 0 {
                        book.bids.remove(0);
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    book.asks.push(order);
                    book.asks.sort_by(|a, b| {
                        let p_a = a.limit_price().unwrap_or(u128::MAX);
                        let p_b = b.limit_price().unwrap_or(u128::MAX);
                        p_a.cmp(&p_b)
                            .then_with(|| a.ts.cmp(&b.ts))
                            .then_with(|| a.oid.cmp(&b.oid))
                    });
                }
            }
        }

        executed_trades
    }

    fn on_epoch_end(&mut self) -> Vec<Trade> {
        Vec::new()
    }

    fn book_state(&self) -> &OrderBookState {
        &self.book
    }
}

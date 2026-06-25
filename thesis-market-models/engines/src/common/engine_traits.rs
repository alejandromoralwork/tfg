



use std::collections::HashMap;
use crate::common::{AssetPair, OrderBookState, Order, Trade};

pub trait MatchingEngine {
    fn process_order(&mut self, order: Order) -> Vec<Trade>;
    fn on_epoch_end(&mut self) -> Vec<Trade>;
    // Updated signature to return the HashMap
    fn book_state(&self) -> &HashMap<AssetPair, OrderBookState>;
}
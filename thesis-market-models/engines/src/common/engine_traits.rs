



use std::collections::HashMap;
use crate::common::{AssetPair, OrderBookState, Order, Trade};

pub trait MatchingEngine {
    fn process_order(&mut self, order: Order) -> Vec<Trade>;
    // This method is called at the end of each epoch to perform any necessary cleanup or state updates.
    fn on_epoch_end(&mut self) -> Vec<Trade>;
    // return the HashMap to show the state of the books for all asset pairs
    fn book_state(&self) -> &HashMap<AssetPair, OrderBookState>;
}
use crate::common::{Order, OrderBookState, Trade};

// This simulation only ever trades a single asset pair (SOL/USD — see
// AssetPair::default()), so a matching engine only needs to expose one book,
// not a map keyed by pair.
pub trait MatchingEngine {
    fn process_order(&mut self, order: Order) -> Vec<Trade>;
    // This method is called at the end of each epoch to perform any necessary cleanup or state updates.
    fn on_epoch_end(&mut self) -> Vec<Trade>;
    // return the state of the single order book this engine maintains.
    fn book_state(&self) -> &OrderBookState;
}

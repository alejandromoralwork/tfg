use crate::common::{AssetPair, Order};

/// Passive state holder for a resting limit-order book: which orders sit on
/// each side. Actual matching logic lives in `engines::cda::ContinuousEngine`
/// (it walks `bids`/`asks` directly) — this struct only exists so
/// `MatchingEngine::book_state()` has something to return, and so the FBA
/// engine (which never rests an order) has an always-empty book to hand back
/// for the same trait method.
#[derive(Clone, Debug)]
pub struct OrderBookState {
    pub pair: AssetPair,
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
}

impl OrderBookState {
    pub fn new(pair: AssetPair) -> Self {
        Self {
            pair,
            bids: Vec::new(),
            asks: Vec::new(),
        }
    }
}

impl Default for OrderBookState {
    /// This simulation only trades the default SOL/USD pair, so a book can
    /// always be constructed without specifying one explicitly.
    fn default() -> Self {
        Self::new(AssetPair::default())
    }
}

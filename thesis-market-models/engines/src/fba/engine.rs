use crate::common::{MatchingEngine, Order, OrderBookState, Trade};
use crate::fba::optimizer::{SettlementOptimizer, SettlementSummary};


#[derive(Debug)]
pub struct BatchAuctionEngine {
    pub order_buffer: Vec<Order>,      // Collects orders until the epoch ends
    pub current_book: OrderBookState, // Represents the state of the book
    pub optimizer: SettlementOptimizer,
}

impl BatchAuctionEngine {
    pub fn new(optimizer: SettlementOptimizer) -> Self {
        Self {
            order_buffer: Vec::new(),
            current_book: OrderBookState::new(),
            optimizer,
        }
    }
}

impl MatchingEngine for BatchAuctionEngine {
    // FBA does not match instantly; it buffers the order for the next clearing
    fn process_order(&mut self, order: Order) -> Vec<Trade> {
        self.order_buffer.push(order);
        Vec::new() // Return empty vector: no trades occur until the batch clears
    }

    fn on_epoch_end(&mut self) -> Vec<Trade> {
        // 1. Run the SettlementOptimizer on the collected buffer
        let summary: SettlementSummary = self.optimizer.optimize(&self.order_buffer);
        
        // 2. Clear the buffer for the next epoch
        self.order_buffer.clear();
        
        // 3. Return the calculated trades from the batch
        summary.trades
    }

    fn book_state(&self) -> &OrderBookState {
        &self.current_book
    }
}
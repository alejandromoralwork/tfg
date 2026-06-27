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
        if self.order_buffer.is_empty() {
            return Vec::new();
        }

        // 1. Calculate Net Positions per Participant
        // Map Key: (ParticipantID, Side) -> Net Quantity
        let mut net_map: HashMap<(String, Side), u128> = HashMap::new();

        for order in &self.order_buffer {
            let key = (order.participant_id.clone(), order.side);
            *net_map.entry(key).or_insert(0) += order.remaining;
        }

        // 2. Reconstruct "Net Orders" for the Optimizer
        // This converts the aggregated map back into a list of Orders
        let mut net_orders: Vec<Order> = net_map
            .into_iter()
            .map(|((participant, side), qty)| {
                // We create a "virtual" order representing the participant's net position
                Order {
                    id: 0, // IDs aren't strictly required for the optimizer logic
                    participant_id: participant,
                    pair: self.order_buffer[0].pair.clone(), // Assuming single-pair batches
                    side,
                    kind: crate::common::OrderKind::Limit, // Default to limit for FBA
                    remaining: qty,
                    timestamp: 0,
                    limit_price: None, // Or aggregate price logic if needed
                }
            })
            .collect();

        // 3. Run the SettlementOptimizer on the Net Orders
        let summary: SettlementSummary = self.optimizer.optimize(&net_orders);
        
        // 4. Clear the buffer for the next epoch
        self.order_buffer.clear();
        
        summary.trades
    }

    fn book_state(&self) -> &OrderBookState {
        &self.current_book
    }
}
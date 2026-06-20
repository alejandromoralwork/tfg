//! Frequent batch auction clearing and settlement optimization.

mod clearing;
mod optimizer;

pub use clearing::{BatchAuctionEngine, ClearingResult};
pub use optimizer::{SettlementOptimizer, SettlementSummary};

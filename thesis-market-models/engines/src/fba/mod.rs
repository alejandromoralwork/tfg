//! Frequent batch auction clearing and settlement optimization.
//!
//! This engine trades a single fixed pair (SOL/USD), so the uniform
//! clearing price has a closed-form solution (see `clearing::select_price`)
//! and no general LP-solver path is needed at runtime — see
//! docs/ENGINE_DESIGN.md §1.4.

mod clearing;

// Keep core optimization logic inside the engine library
#[path = "../shared/optimizer.rs"]
mod optimizer;

// Clearing & Settlement Re-exports
pub use clearing::{BatchAuctionEngine, ClearingResult};
pub use optimizer::{SettlementOptimizer, SettlementSummary};

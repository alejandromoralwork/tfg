//! Frequent batch auction clearing and settlement optimization.

mod clearing;
mod lp_encoder;
mod solver;

// Keep core optimization logic inside the engine library
#[path = "../shared/optimizer.rs"] 
mod optimizer;

#[path = "../shared/amm.rs"]
mod amm;

// Clearing & Settlement Re-exports
pub use clearing::{BatchAuctionEngine, ClearingResult};
pub use clearing::{MultiAssetEngine, MultiAssetClearingResult};
pub use optimizer::{SettlementOptimizer, SettlementSummary};
pub use amm::{AMMPool, ArbitrageEngine, ArbitrageOracle};
pub use solver::LPBuilder;
pub use lp_encoder::LPEncoder;

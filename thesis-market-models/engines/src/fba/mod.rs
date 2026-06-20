//! Frequent batch auction clearing and settlement optimization.

mod clearing;
mod optimizer;
mod amm;
mod lp_encoder;
mod solver;

pub use clearing::{BatchAuctionEngine, ClearingResult};
pub use optimizer::{SettlementOptimizer, SettlementSummary};
pub use clearing::{MultiAssetEngine, MultiAssetClearingResult};
pub use amm::AMMPool;
pub use solver::LPBuilder;
pub use lp_encoder::LPEncoder;

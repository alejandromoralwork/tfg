use crate::common::{Batch, Order};
use crate::fba::AMMPool;
use crate::fba::solver::LPBuilder;

/// LP encoder translates a batch of orders and AMM linear segments into
/// a textual LP model using `LPBuilder`.
///
/// This is a starter implementation that emits a minimal valid LP file for
/// small examples. It will be extended to encode full decision variables
/// (`v_k`, prices `p_i`) and AMM piecewise-linear constraints.
#[derive(Debug, Clone)]
pub struct LPEncoder {}

impl LPEncoder {
    pub fn new() -> Self {
        Self {}
    }

    /// Encode a batch and optional AMM pools into an LP model string.
    ///
    /// Currently emits a placeholder objective and a trivial constraint to
    /// ensure produced LP files are syntactically valid. Will be extended
    /// to encode the full FBA LP as described in the thesis notes.
    pub fn encode_batch(&self, _batch: &Batch, _pools: &[AMMPool]) -> String {
        let mut lp = LPBuilder::new();

        // Placeholder: set empty objective (LPBuilder emits Minimize),
        // real model must maximize sum v_k (converted to minimization).
        lp.set_objective("0");

        // Add a trivial constraint so the LP is valid
        lp.add_constraint("c_placeholder", "0 >= 0");

        lp.to_lp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::AssetPair;

    #[test]
    fn lp_encoder_emits_lp() {
        let encoder = LPEncoder::new();
        let pair = AssetPair::new("X", "Y");
        let order = Order::market(1, "p1", pair, crate::common::Side::Sell, 100u128, 0);
        let batch = Batch { id: 1, orders: vec![order] };
        let s = encoder.encode_batch(&batch, &[]);
        assert!(s.contains("Minimize"));
        assert!(s.contains("Subject To"));
    }
}

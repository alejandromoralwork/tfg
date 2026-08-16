
/* ============================================================================
 * MODULE: LP ENCODER (Frequent Batch Auction Implementation)
 * * DESCRIPTION:
 * This module acts as a translator between our simulation's runtime data
 * structures and an external Linear Programming (LP) optimization solver.
 * It takes a standard 'Batch' of collected orders and encodes them into a
 * standardized textual LP file format (.lp).
 *
 * SIMULATION PIPELINE FLOW:
 * [Batch Orders] ---> [LPEncoder] ---> (.lp Text File) ---> [LP Solver]
 * │
 * [Uniform Clearing Price] <-------------------------------------------┘
 * * ============================================================================
 * MATHEMATICAL CORE FORMULATION
 * ============================================================================
 * * 1. THE OBJECTIVE FUNCTION (Social Welfare Maximization)
 * The primary goal of the Frequent Batch Auction is to maximize market liquidity
 * and matching efficiency. The solver is directed to maximize the sum of all
 * executed order volumes (v_k) across the entire batch:
 * * Maximize: Sum( v_k )  for all orders k

 * * 2. STRUCTURAL CONSTRAINTS
 * - Conservation of Flow (Market Clearing): The total volume of the asset
 * bought by participants must exactly equal the volume sold:
 * Sum( Buy_X ) - Sum( Sell_X ) = 0
 *
 * - Price Boundaries: No buyer fills above their maximum limit price, and
 * no seller fills below their minimum limit price.
 * ============================================================================
 * NOTE: There is no external liquidity source (e.g. an AMM) backing this
 * batch. Volume that cannot be matched at the clearing price simply remains
 * unexecuted, per the flow-conservation constraint above.
 * ============================================================================
 * STATUS: Starter framework emitting layout skeletons for syntactical validation.
 * For a single asset, this LP has a closed-form solution (see
 * `fba::clearing::BatchAuctionEngine::select_price`, which the working engine
 * actually uses instead of an external LP solver); this encoder is kept for
 * the formal/theoretical statement of the clearing problem.
 * ============================================================================
 */


///
/// This is a starter implementation that emits a minimal valid LP file for
/// small examples. It will be extended to encode full decision variables
/// (`v_k`, prices `p_i`) as needed.

// 1. Common types (standardized)
use crate::common::{Order, Side};
use crate::common::Batch;

// 2. FBA solver
use crate::fba::solver::LPBuilder;


/// LP encoder translates a batch of orders into a textual LP model using `LPBuilder`.
#[derive(Debug, Clone, Default)]
pub struct LPEncoder;

impl LPEncoder {
    pub fn new() -> Self {
        Self
    }

    /// Encode a batch into an LP model string.
    pub fn encode_batch(&self, batch: &Batch) -> String {
        let mut lp = LPBuilder::new();

        let mut objective_terms = Vec::new();
        let mut base_flow_terms = Vec::new();

        // ==========================================
        // 1. ENCODE USER ORDERS
        // ==========================================
        for order in &batch.orders {
            let v_name = format!("v_{}", order.oid); // Decision variable: filled volume

            // Constraint: Fill volume cannot be negative, and cannot exceed order size
            lp.add_constraint(
                &format!("bound_min_{}", order.oid),
                &format!("{} >= 0", v_name)
            );
            lp.add_constraint(
                &format!("bound_max_{}", order.oid),
                &format!("{} <= {}", v_name, order.orig_sz)
            );

            // Objective: Maximize total filled volume.
            // LP solvers natively Minimize, so we minimize the negative sum: -v_1 - v_2 ...
            objective_terms.push(format!("-1 {}", v_name));

            // Track Conservation of Flow
            if order.side() == Side::Buy {
                base_flow_terms.push(format!("+ 1 {}", v_name));
            } else {
                base_flow_terms.push(format!("- 1 {}", v_name));
            }

            // Note: In a complete implementation, you would also calculate and track
            // the Quote flow here based on the clearing price (p * v_k).
        }

        // ==========================================
        // 2. FINALIZE & ASSEMBLE THE LP FILE
        // ==========================================

        // Set the Objective Function (Maximize Volume)
        if objective_terms.is_empty() {
            lp.set_objective("0");
        } else {
            lp.set_objective(&objective_terms.join(" "));
        }

        // Set the Conservation of Flow Constraints. Any residual imbalance that
        // cannot be matched at the clearing price is left unexecuted — there is
        // no external liquidity source to absorb it.
        if !base_flow_terms.is_empty() {
            lp.add_constraint("conservation_base", &format!("{} = 0", base_flow_terms.join(" ")));
        }

        lp.to_lp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{AssetPair, Order};

    #[test]
    fn lp_encoder_emits_valid_fba_constraints() {
        let encoder = LPEncoder::new();
        let pair = AssetPair::new("X", "Y");

        let order1 = Order::market(1, "p1", pair.clone(), crate::common::Side::Sell, 100u128, 0);
        let order2 = Order::market(2, "p2", pair.clone(), crate::common::Side::Buy, 50u128, 0);

        let batch = Batch { id: 1, orders: vec![order1, order2] };

        let s = encoder.encode_batch(&batch);

        assert!(s.contains("Minimize"));
        assert!(s.contains("Subject To"));

        // Check Objective function
        assert!(s.contains("-1 v_1"));
        assert!(s.contains("-1 v_2"));

        // Check boundary constraints
        assert!(s.contains("v_1 <= 100"));
        assert!(s.contains("v_2 <= 50"));

        // Check conservation of flow
        assert!(s.contains("conservation_base"));
    }
}

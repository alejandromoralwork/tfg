//! Shared depth-within-bps computation, used identically by the CDA book
//! wrapper and the FBA batch wrapper so both engines' depth_within_bps
//! metric comes from the exact same formula, computed here in the harness
//! rather than inside either matching engine.

use engines::common::Side;
use metrics::DEPTH_BPS_THRESHOLDS;

/// Cumulative resting/schedule volume within each of `DEPTH_BPS_THRESHOLDS`
/// basis points of `reference`, split by side. `levels` is any iterator of
/// (price, side, quantity) — the CDA book's bids/asks, or an FBA batch's raw
/// order list.
pub fn depth_schedule(
    reference: u128,
    levels: impl Iterator<Item = (u128, Side, u128)>,
) -> [(u128, u128); DEPTH_BPS_THRESHOLDS.len()] {
    let mut schedule = [(0u128, 0u128); DEPTH_BPS_THRESHOLDS.len()];
    if reference == 0 {
        return schedule;
    }
    for (price, side, qty) in levels {
        let diff = if price > reference { price - reference } else { reference - price };
        let bps = diff.saturating_mul(10_000) / reference;
        for (i, threshold) in DEPTH_BPS_THRESHOLDS.iter().enumerate() {
            if bps <= *threshold as u128 {
                match side {
                    Side::Buy => schedule[i].0 += qty,
                    Side::Sell => schedule[i].1 += qty,
                }
            }
        }
    }
    schedule
}

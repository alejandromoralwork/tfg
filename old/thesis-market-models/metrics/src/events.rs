//! Plain event types the matching engines' output gets turned into before
//! being handed to a `MetricsCollector`. None of these types depend on
//! engine internals — they only reuse the plain data types (`Order`,
//! `Trade`, `Side`) that engines already emit, so a caller (the simulation
//! harness) can construct them purely from what an engine returns.

use engines::common::{Side, Trade};
use std::time::Duration;

/// Which engine produced the events being collected. Both engines feed the
/// exact same event/metric machinery — this tag is only used to label the
/// resulting time series so CDA and FBA runs can be told apart in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Cda,
    Fba,
}

impl EngineKind {
    pub fn label(&self) -> &'static str {
        match self {
            EngineKind::Cda => "CDA",
            EngineKind::Fba => "FBA",
        }
    }
}

/// A single order-flow message observed by the harness, recorded BEFORE any
/// engine-level accept/reject gating (`Order::is_new_live_order`).
///
/// This is deliberately captured upstream of the engine's own accept/reject
/// decision: metrics like the order-to-trade ratio and message-traffic
/// statistics need to see rejected/cancelled/un-triggered messages too, not
/// just what actually entered a book or batch. Feed every record the
/// replay/CLI produces here, tagging `accepted` with whatever
/// `Order::is_new_live_order()` returned.
#[derive(Debug, Clone)]
pub struct OrderMessage {
    pub ts: u64,
    pub oid: u64,
    pub user_id: String,
    pub side: Side,
    pub limit_price: Option<u128>,
    pub quantity: u128,
    /// Whether this message was accepted as a new live order by the engine.
    /// `false` covers rejections, cancellations, fills, and un-triggered
    /// conditional orders (see `Order::is_new_live_order`).
    pub accepted: bool,
}

/// A trade produced by either engine, plus the two pieces of context needed
/// to compute microstructure metrics from it that aren't part of the core
/// `Trade` type (deliberately — they're metrics-analysis concepts, not
/// matching-engine concepts, so they live here rather than growing
/// `engines::common::Trade`):
///
/// - `reference_price`: the price this trade should be measured against for
///   effective-spread / price-impact purposes. For the CDA, this is the book
///   midpoint immediately before the incoming order was matched. For the
///   FBA, there's no continuously updating quote to reference, so it's the
///   last price this engine actually cleared a trade at prior to this batch
///   (`BatchAuctionEngine::last_clearing_price` at submission time) — `None`
///   if there's no prior reference yet.
/// - `aggressor_side`: which side initiated the trade (the incoming/taker
///   order), needed for the signed `D_k` in the effective/realized-spread
///   formulas. Well-defined for the CDA (the incoming order that triggered
///   the match). Not meaningful for the FBA — a uniform-price call auction
///   has no taker/maker distinction — so it's `None` there, and the
///   collector falls back to an unsigned price-deviation measure instead of
///   discarding the trade.
#[derive(Debug, Clone)]
pub struct TradeEvent {
    pub trade: Trade,
    pub reference_price: Option<u128>,
    pub aggressor_side: Option<Side>,
}

/// Fixed set of basis-point offsets the depth-within-x-bps metric is
/// reported at, for both engines.
pub const DEPTH_BPS_THRESHOLDS: [u32; 3] = [10, 50, 100];

/// Emitted once per FBA batch clearing attempt, whether or not it produced
/// any trades.
#[derive(Debug, Clone)]
pub struct BatchClearedEvent {
    /// When the batch closed (used for interval bucketing).
    pub ts: u64,
    /// When the batch opened — together with `ts`, defines the batch
    /// window used for the boundary-concentration metric.
    pub batch_open_ts: u64,
    pub clearing_price: Option<u128>,
    pub demand_at_price: u128,
    pub supply_at_price: u128,
    pub traded_quantity: u128,
    /// Volume left unexecuted on the heavier side of the book this batch.
    pub unexecuted_quantity: u128,
    /// The best (most aggressive) unfilled buy/sell limit price remaining
    /// after clearing — the FBA counterpart of a CDA's best bid/ask, used
    /// for the "quoted spread" analogue and depth-at-best.
    pub best_unfilled_buy: Option<u128>,
    pub best_unfilled_sell: Option<u128>,
    /// Cumulative demand/supply at each of `DEPTH_BPS_THRESHOLDS` away from
    /// the clearing price, i.e. one (demand, supply) pair per threshold, in
    /// the same order as `DEPTH_BPS_THRESHOLDS`.
    pub depth_schedule: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    /// Wall-clock time the clearing computation itself took.
    pub compute_time: Duration,
}

/// A snapshot of the CDA book, taken after processing a single order.
#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub ts: u64,
    pub best_bid: Option<u128>,
    pub best_ask: Option<u128>,
    pub bid_depth: u128,
    pub ask_depth: u128,
    /// Cumulative bid/ask depth at each of `DEPTH_BPS_THRESHOLDS` away from
    /// the midpoint, same shape/order as `BatchClearedEvent::depth_schedule`.
    pub depth_schedule: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    /// Wall-clock time this single order's matching pass took — the CDA
    /// counterpart of `BatchClearedEvent::compute_time`.
    pub compute_time: Duration,
}

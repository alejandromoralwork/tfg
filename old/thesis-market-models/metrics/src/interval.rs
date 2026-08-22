//! One row of the metric time series: everything computable for a single
//! `[interval_start, interval_start + interval_width)` bucket, on the same
//! time grid for both engines (see docs/expose.tex, "Comparability Protocol").

use crate::events::DEPTH_BPS_THRESHOLDS;

/// A single time-bucketed row of metrics. Every field maps directly to one
/// row of the metric catalogue tables in docs/expose.tex (§ Metric
/// Catalogue). Fields are `Option<f64>` wherever the metric can genuinely be
/// undefined for a bucket (e.g. no trades occurred, or an external input the
/// collector wasn't given isn't available) — `None` here always means
/// "not computable from what was recorded," never a silent zero.
#[derive(Debug, Clone)]
pub struct IntervalMetrics {
    pub engine: &'static str,
    pub interval_start: u64,
    pub interval_width: u64,

    // ---- RQ2.1: liquidity and transaction cost ----
    /// (ask-bid)/mid in bps, time-weighted across snapshots in the bucket.
    /// FBA counterpart: implied spread between the best unfilled buy/sell.
    pub quoted_spread_bps: Option<f64>,
    /// Displayed/schedule volume at the best level, averaged across the bucket.
    pub depth_at_best: Option<f64>,
    /// Cumulative volume within each of `DEPTH_BPS_THRESHOLDS`, averaged
    /// across the bucket. Index i corresponds to `DEPTH_BPS_THRESHOLDS[i]`.
    pub depth_within_bps: [Option<f64>; DEPTH_BPS_THRESHOLDS.len()],
    /// (bid_vol - ask_vol) / (bid_vol + ask_vol) at the top of book, averaged
    /// across the bucket. Not meaningful for the FBA (no resting book) and
    /// left `None` there.
    pub book_imbalance: Option<f64>,
    /// Volume-weighted effective spread in bps: 2*D*(p-m)/m.
    pub effective_spread_bps: Option<f64>,
    /// Volume-weighted realized spread in bps, for markout horizons of
    /// 1s / 5s / 30s: 2*D*(p - m_{t+delta})/m.
    pub realized_spread_bps_1s: Option<f64>,
    pub realized_spread_bps_5s: Option<f64>,
    pub realized_spread_bps_30s: Option<f64>,
    /// effective_spread - realized_spread, per horizon.
    pub price_impact_bps_1s: Option<f64>,
    pub price_impact_bps_5s: Option<f64>,
    pub price_impact_bps_30s: Option<f64>,
    /// mean(|return| / volume) across the bucket.
    pub amihud_illiquidity: Option<f64>,

    // ---- RQ2.2: price discovery and market quality ----
    /// Std. deviation of returns of the reference price series (midpoint for
    /// CDA, clearing price for FBA) across the bucket.
    pub realized_volatility: Option<f64>,
    /// Std. deviation of trade execution prices within the bucket. Zero by
    /// construction for the FBA (uniform price), strictly positive for the
    /// CDA — this is a direct check of the uniform-price property.
    pub intra_interval_price_dispersion: Option<f64>,
    /// |p - p_ref| in bps against an external reference price. `None`
    /// unless the collector was supplied a reference price series — not
    /// wired up yet in this implementation (see MetricsCollector docs).
    pub pricing_error_bps: Option<f64>,

    // ---- RQ2.3: execution, allocation, and engine performance ----
    /// Total matched quantity and notional, and number of trades.
    pub executed_volume: f64,
    pub executed_notional: f64,
    pub trade_count: u64,
    /// Filled / submitted quantity, for orders first submitted in this
    /// bucket (including fills that land in later buckets).
    pub fill_rate: Option<f64>,
    /// Mean elapsed time (seconds) from submission to first fill, for
    /// orders first submitted in this bucket that received at least one fill.
    pub avg_time_to_execution_secs: Option<f64>,
    /// Sum over trades executed in this bucket of |limit - execution price| * qty
    /// for both counterparties.
    pub trader_surplus: f64,
    /// Mean, across participants active in this bucket, of submitted/filled
    /// size ratio — the empirical signature of order-size inflation.
    pub order_size_inflation: Option<f64>,
    /// Submitted messages (including rejections/cancellations) per executed
    /// trade in this bucket.
    pub order_to_trade_ratio: Option<f64>,
    /// Share of order arrivals in the final 10% of the enclosing batch
    /// window. FBA only; `None` for the CDA (no batch window to speak of).
    pub boundary_concentration: Option<f64>,
    /// Orders processed per second of wall-clock compute time in this bucket.
    pub throughput_orders_per_sec: Option<f64>,
    /// Mean wall-clock time (microseconds) per clearing/match computation.
    pub avg_clearing_latency_micros: Option<f64>,
    /// Share of the traded side's volume (demand+supply) that went
    /// unexecuted at the clearing price. FBA only.
    pub unexecuted_residual_share: Option<f64>,
}

impl IntervalMetrics {
    pub fn empty(engine: &'static str, interval_start: u64, interval_width: u64) -> Self {
        Self {
            engine,
            interval_start,
            interval_width,
            quoted_spread_bps: None,
            depth_at_best: None,
            depth_within_bps: [None; DEPTH_BPS_THRESHOLDS.len()],
            book_imbalance: None,
            effective_spread_bps: None,
            realized_spread_bps_1s: None,
            realized_spread_bps_5s: None,
            realized_spread_bps_30s: None,
            price_impact_bps_1s: None,
            price_impact_bps_5s: None,
            price_impact_bps_30s: None,
            amihud_illiquidity: None,
            realized_volatility: None,
            intra_interval_price_dispersion: None,
            pricing_error_bps: None,
            executed_volume: 0.0,
            executed_notional: 0.0,
            trade_count: 0,
            fill_rate: None,
            avg_time_to_execution_secs: None,
            trader_surplus: 0.0,
            order_size_inflation: None,
            order_to_trade_ratio: None,
            boundary_concentration: None,
            throughput_orders_per_sec: None,
            avg_clearing_latency_micros: None,
            unexecuted_residual_share: None,
        }
    }
}

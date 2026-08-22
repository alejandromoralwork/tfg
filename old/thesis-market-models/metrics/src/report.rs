//! Plain-text / CSV rendering of a metric time series — kept separate from
//! `IntervalMetrics` itself so the collector/interval modules stay free of
//! any formatting concerns.

use crate::events::DEPTH_BPS_THRESHOLDS;
use crate::interval::IntervalMetrics;

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{:.6}", x),
        None => "".to_string(),
    }
}

pub fn csv_header() -> String {
    let mut cols = vec![
        "engine",
        "interval_start_ns",
        "interval_width_ns",
        "quoted_spread_bps",
        "depth_at_best",
    ];
    for bps in DEPTH_BPS_THRESHOLDS {
        cols.push(match bps {
            10 => "depth_within_10bps",
            50 => "depth_within_50bps",
            100 => "depth_within_100bps",
            _ => "depth_within_bps",
        });
    }
    cols.extend([
        "book_imbalance",
        "effective_spread_bps",
        "realized_spread_bps_1s",
        "realized_spread_bps_5s",
        "realized_spread_bps_30s",
        "price_impact_bps_1s",
        "price_impact_bps_5s",
        "price_impact_bps_30s",
        "amihud_illiquidity",
        "realized_volatility",
        "intra_interval_price_dispersion",
        "pricing_error_bps",
        "executed_volume",
        "executed_notional",
        "trade_count",
        "fill_rate",
        "avg_time_to_execution_secs",
        "trader_surplus",
        "order_size_inflation",
        "order_to_trade_ratio",
        "boundary_concentration",
        "throughput_orders_per_sec",
        "avg_clearing_latency_micros",
        "unexecuted_residual_share",
    ]);
    cols.join(",")
}

pub fn csv_row(m: &IntervalMetrics) -> String {
    let mut fields = vec![
        m.engine.to_string(),
        m.interval_start.to_string(),
        m.interval_width.to_string(),
        fmt_opt(m.quoted_spread_bps),
        fmt_opt(m.depth_at_best),
    ];
    for v in m.depth_within_bps {
        fields.push(fmt_opt(v));
    }
    fields.extend([
        fmt_opt(m.book_imbalance),
        fmt_opt(m.effective_spread_bps),
        fmt_opt(m.realized_spread_bps_1s),
        fmt_opt(m.realized_spread_bps_5s),
        fmt_opt(m.realized_spread_bps_30s),
        fmt_opt(m.price_impact_bps_1s),
        fmt_opt(m.price_impact_bps_5s),
        fmt_opt(m.price_impact_bps_30s),
        fmt_opt(m.amihud_illiquidity),
        fmt_opt(m.realized_volatility),
        fmt_opt(m.intra_interval_price_dispersion),
        fmt_opt(m.pricing_error_bps),
        format!("{:.6}", m.executed_volume),
        format!("{:.6}", m.executed_notional),
        m.trade_count.to_string(),
        fmt_opt(m.fill_rate),
        fmt_opt(m.avg_time_to_execution_secs),
        format!("{:.6}", m.trader_surplus),
        fmt_opt(m.order_size_inflation),
        fmt_opt(m.order_to_trade_ratio),
        fmt_opt(m.boundary_concentration),
        fmt_opt(m.throughput_orders_per_sec),
        fmt_opt(m.avg_clearing_latency_micros),
        fmt_opt(m.unexecuted_residual_share),
    ]);
    fields.join(",")
}

/// Render a full time series as CSV text (header + one row per interval).
pub fn to_csv(series: &[IntervalMetrics]) -> String {
    let mut out = String::new();
    out.push_str(&csv_header());
    out.push('\n');
    for row in series {
        out.push_str(&csv_row(row));
        out.push('\n');
    }
    out
}

/// A compact human-readable summary table for terminal display: one line
/// per interval, showing the headline column from each of the three RQ
/// dimensions rather than the full 25-column CSV.
pub fn to_summary_table(series: &[IntervalMetrics]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<6} {:>16} {:>10} {:>6} {:>12} {:>12} {:>10} {:>10}\n",
        "engine", "interval_start", "spread_bp", "trades", "volume", "surplus", "fillrate", "unexec%"
    ));
    for m in series {
        out.push_str(&format!(
            "{:<6} {:>16} {:>10} {:>6} {:>12.2} {:>12.2} {:>10} {:>10}\n",
            m.engine,
            m.interval_start,
            m.quoted_spread_bps.or(m.effective_spread_bps).map(|v| format!("{:.2}", v)).unwrap_or_default(),
            m.trade_count,
            m.executed_volume,
            m.trader_surplus,
            m.fill_rate.map(|v| format!("{:.1}%", v * 100.0)).unwrap_or_default(),
            m.unexecuted_residual_share.map(|v| format!("{:.1}%", v * 100.0)).unwrap_or_default(),
        ));
    }
    out
}

//! Independent metrics-calculation layer for the CDA/FBA comparison.
//!
//! Deliberately kept as its own crate that `engines` never depends on: the
//! matching engines (`engines::cda`, `engines::fba`) know nothing about this
//! crate. The simulation harness is the only thing that depends on both —
//! it drives an engine, turns its output into the plain events in
//! `crate::events`, feeds them to a `MetricsCollector`, and calls
//! `finalize()` to get a time series it can print, compare, or export.

pub mod collector;
pub mod events;
pub mod interval;
pub mod report;

pub use collector::MetricsCollector;
pub use events::{
    BatchClearedEvent, BookSnapshot, EngineKind, OrderMessage, TradeEvent, DEPTH_BPS_THRESHOLDS,
};
pub use interval::IntervalMetrics;

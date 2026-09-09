//! Crash-safe progress state for the `simulate` command so a multi-day
//! replay can be stopped and resumed without redoing finished work.
//!
//! A run writes `output/<slug>/checkpoint.txt` after every input file, right
//! after the settled time-series rows for that file have been appended to
//! the CSVs. The checkpoint is the source of truth: on resume the CSVs are
//! trimmed back to the row counts it records (a crash between the CSV append
//! and the checkpoint rename leaves the CSV one file's worth of rows ahead),
//! then streaming continues from the next unprocessed file.
//!
//! Resume is deliberately approximate — the in-flight event window and the
//! engine books are NOT persisted, only the bucket grid, the emit cursor,
//! the cross-flush `amihud` carry, cumulative counters and the summary
//! accumulators. A resumed run therefore leaves a gap of empty interval
//! rows covering roughly the settle window that was still buffered when it
//! stopped; everything after the resume point is exact again.
//!
//! Format: plain `key<space>value` lines, hand-rolled (this crate has no
//! serialization dependency and doesn't want one). Written to a `.tmp`
//! sibling and `rename`d into place so a reader never sees a half-written
//! file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::metrics::timeseries::IntervalMetrics;

pub const CHECKPOINT_FILE: &str = "checkpoint.txt";
pub const FBA_CSV: &str = "fba_timeseries.csv";
pub const CDA_CSV: &str = "cda_timeseries.csv";
pub const SUMMARY_FILE: &str = "summary.txt";

// v2: added the `kyle_lambda` metric column to the time-series CSVs and the
// `{fba,cda}_prev_clearing` carry lines. A v1 checkpoint's on-disk CSV header
// has no `kyle_lambda` column, so appending v2 rows to it on resume would
// misalign the file — reject v1 and make the user delete + restart.
const VERSION: u32 = 2;

/// Directory name for a given `simulate` source: its last path component,
/// with anything outside `[A-Za-z0-9._-]` replaced by `_`. `sol` stays
/// `sol`; `../data/sample/order_statuses/20251201` becomes `20251201`.
/// Two different sources that share a last component would collide on the
/// same `output/<slug>/`; the `source` field in the checkpoint catches that
/// on the next run and refuses rather than mixing them.
pub fn slugify(source: &str) -> String {
    let last = Path::new(source).file_name().and_then(|s| s.to_str()).unwrap_or("run");
    let mut out: String = last
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    if out.is_empty() {
        out.push_str("run");
    }
    out
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub version: u32,
    /// The resolved source path this output directory belongs to — a
    /// mismatch on resume means the directory is for a different run.
    pub source: String,
    pub interval_ns: u64,
    pub settle_ns: u64,
    pub files_total: usize,
    pub files_done: usize,
    pub last_file: String,
    /// Bucket-grid origin shared by both recorders (`None` only before the
    /// very first record has been seen).
    pub anchor: Option<u64>,
    /// Exclusive bucket boundary already flushed to both CSVs.
    pub emitted_upto: u64,
    pub records_seen: usize,
    pub records_skipped: usize,
    pub files_processed: usize,
    pub files_skipped: usize,
    pub fba_rows_written: u64,
    pub cda_rows_written: u64,
    pub fba_prev_close: Option<f64>,
    pub cda_prev_close: Option<f64>,
    /// FBA `kyle_lambda` cross-flush carry (last in-range batch clearing
    /// price). `cda_prev_clearing` is always `None` — kept for symmetry.
    pub fba_prev_clearing: Option<f64>,
    pub cda_prev_clearing: Option<f64>,
    pub fba_late_dropped: u64,
    pub cda_late_dropped: u64,
    /// Cumulative wall-clock across every run that has contributed to this
    /// output directory.
    pub elapsed_secs: f64,
    pub fba_summary: SummaryAccumulator,
    pub cda_summary: SummaryAccumulator,
    pub complete: bool,
}

impl Checkpoint {
    pub fn fresh(source: String, interval_ns: u64, settle_ns: u64, files_total: usize) -> Self {
        Self {
            version: VERSION,
            source,
            interval_ns,
            settle_ns,
            files_total,
            files_done: 0,
            last_file: String::new(),
            anchor: None,
            emitted_upto: 0,
            records_seen: 0,
            records_skipped: 0,
            files_processed: 0,
            files_skipped: 0,
            fba_rows_written: 0,
            cda_rows_written: 0,
            fba_prev_close: None,
            cda_prev_close: None,
            fba_prev_clearing: None,
            cda_prev_clearing: None,
            fba_late_dropped: 0,
            cda_late_dropped: 0,
            elapsed_secs: 0.0,
            fba_summary: SummaryAccumulator::new(),
            cda_summary: SummaryAccumulator::new(),
            complete: false,
        }
    }

    /// Read `dir/checkpoint.txt` if it exists. `Ok(None)` means no prior
    /// run; `Err` is a genuine IO/parse problem worth surfacing.
    pub fn load(dir: &Path) -> io::Result<Option<Checkpoint>> {
        let path = dir.join(CHECKPOINT_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        parse(&text).map(Some).map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {msg}", path.display())))
    }

    /// Write atomically: a full `.tmp` sibling, then `rename` over the real
    /// file (same directory, so the rename stays on one filesystem).
    pub fn save_atomic(&self, dir: &Path) -> io::Result<()> {
        let final_path = dir.join(CHECKPOINT_FILE);
        let tmp_path = dir.join(format!("{CHECKPOINT_FILE}.tmp"));
        fs::write(&tmp_path, render(self))?;
        // Windows `rename` won't clobber an existing file; remove first.
        // The window between remove and rename is the price of no-deps
        // atomicity on Windows — a crash there just means the next run
        // starts from the previous checkpoint (or fresh), which the CSV
        // reconciliation already handles.
        if final_path.exists() {
            fs::remove_file(&final_path)?;
        }
        fs::rename(&tmp_path, &final_path)
    }
}

fn opt_f64_to_str(v: Option<f64>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "NONE".to_string(),
    }
}

fn str_to_opt_f64(s: &str) -> Result<Option<f64>, String> {
    if s == "NONE" {
        Ok(None)
    } else {
        s.parse::<f64>().map(Some).map_err(|_| format!("bad f64 '{s}'"))
    }
}

fn render(c: &Checkpoint) -> String {
    let mut out = String::new();
    let mut line = |k: &str, v: &str| {
        out.push_str(k);
        out.push(' ');
        out.push_str(v);
        out.push('\n');
    };
    line("version", &c.version.to_string());
    line("source", &c.source);
    line("interval_ns", &c.interval_ns.to_string());
    line("settle_ns", &c.settle_ns.to_string());
    line("files_total", &c.files_total.to_string());
    line("files_done", &c.files_done.to_string());
    line("last_file", &c.last_file);
    line("anchor", &c.anchor.map(|a| a.to_string()).unwrap_or_else(|| "NONE".to_string()));
    line("emitted_upto", &c.emitted_upto.to_string());
    line("records_seen", &c.records_seen.to_string());
    line("records_skipped", &c.records_skipped.to_string());
    line("files_processed", &c.files_processed.to_string());
    line("files_skipped", &c.files_skipped.to_string());
    line("fba_rows_written", &c.fba_rows_written.to_string());
    line("cda_rows_written", &c.cda_rows_written.to_string());
    line("fba_prev_close", &opt_f64_to_str(c.fba_prev_close));
    line("cda_prev_close", &opt_f64_to_str(c.cda_prev_close));
    line("fba_prev_clearing", &opt_f64_to_str(c.fba_prev_clearing));
    line("cda_prev_clearing", &opt_f64_to_str(c.cda_prev_clearing));
    line("fba_late_dropped", &c.fba_late_dropped.to_string());
    line("cda_late_dropped", &c.cda_late_dropped.to_string());
    line("elapsed_secs", &c.elapsed_secs.to_string());
    line("fba_summary", &c.fba_summary.serialize());
    line("cda_summary", &c.cda_summary.serialize());
    line("complete", if c.complete { "true" } else { "false" });
    out
}

fn parse(text: &str) -> Result<Checkpoint, String> {
    let mut c = Checkpoint::fresh(String::new(), 0, 0, 0);
    let mut seen_version = false;
    for raw in text.lines() {
        let raw = raw.trim_end_matches(['\r', '\n']);
        if raw.is_empty() {
            continue;
        }
        let (key, val) = raw.split_once(' ').unwrap_or((raw, ""));
        match key {
            "version" => {
                seen_version = true;
                let v: u32 = val.parse().map_err(|_| "bad version")?;
                if v != VERSION {
                    return Err(format!("unsupported checkpoint version {v} (this build writes {VERSION})"));
                }
                c.version = v;
            }
            "source" => c.source = val.to_string(),
            "interval_ns" => c.interval_ns = val.parse().map_err(|_| "bad interval_ns")?,
            "settle_ns" => c.settle_ns = val.parse().map_err(|_| "bad settle_ns")?,
            "files_total" => c.files_total = val.parse().map_err(|_| "bad files_total")?,
            "files_done" => c.files_done = val.parse().map_err(|_| "bad files_done")?,
            "last_file" => c.last_file = val.to_string(),
            "anchor" => c.anchor = if val == "NONE" { None } else { Some(val.parse().map_err(|_| "bad anchor")?) },
            "emitted_upto" => c.emitted_upto = val.parse().map_err(|_| "bad emitted_upto")?,
            "records_seen" => c.records_seen = val.parse().map_err(|_| "bad records_seen")?,
            "records_skipped" => c.records_skipped = val.parse().map_err(|_| "bad records_skipped")?,
            "files_processed" => c.files_processed = val.parse().map_err(|_| "bad files_processed")?,
            "files_skipped" => c.files_skipped = val.parse().map_err(|_| "bad files_skipped")?,
            "fba_rows_written" => c.fba_rows_written = val.parse().map_err(|_| "bad fba_rows_written")?,
            "cda_rows_written" => c.cda_rows_written = val.parse().map_err(|_| "bad cda_rows_written")?,
            "fba_prev_close" => c.fba_prev_close = str_to_opt_f64(val)?,
            "cda_prev_close" => c.cda_prev_close = str_to_opt_f64(val)?,
            "fba_prev_clearing" => c.fba_prev_clearing = str_to_opt_f64(val)?,
            "cda_prev_clearing" => c.cda_prev_clearing = str_to_opt_f64(val)?,
            "fba_late_dropped" => c.fba_late_dropped = val.parse().map_err(|_| "bad fba_late_dropped")?,
            "cda_late_dropped" => c.cda_late_dropped = val.parse().map_err(|_| "bad cda_late_dropped")?,
            "elapsed_secs" => c.elapsed_secs = val.parse().map_err(|_| "bad elapsed_secs")?,
            "fba_summary" => c.fba_summary = SummaryAccumulator::deserialize(val)?,
            "cda_summary" => c.cda_summary = SummaryAccumulator::deserialize(val)?,
            "complete" => c.complete = val == "true",
            _ => {} // forward-compatible: ignore unknown keys
        }
    }
    if !seen_version {
        return Err("missing version line".to_string());
    }
    Ok(c)
}

// ============================================================================
// SummaryAccumulator — folds emitted interval rows into the numbers the
// end-of-run summary.txt prints, so the whole series never has to be kept.
// ============================================================================

/// Why a metric can legitimately be `n/a` for *every* interval, distinct
/// from "just happened to have no trades this run". Mirrors the old
/// `inputs::simulate_cmd::Scope`.
#[derive(Clone, Copy)]
enum Scope {
    Universal,
    FbaOnly,
    CdaOnly,
    NeedsExternalData(&'static str),
}

struct MetricDesc {
    name: &'static str,
    get: fn(&IntervalMetrics) -> Option<f64>,
    scope: Scope,
}

/// The catalogue rows the summary reports on, in print order — every
/// non-structural field of `IntervalMetrics`. Kept as one list so `fold`
/// and `render_section` can't drift apart.
fn metrics() -> Vec<MetricDesc> {
    use Scope::*;
    vec![
        MetricDesc { name: "quoted_spread_bps", get: |m| m.quoted_spread_bps, scope: Universal },
        MetricDesc { name: "depth_at_best", get: |m| m.depth_at_best, scope: Universal },
        MetricDesc { name: "depth_within_10bps", get: |m| m.depth_within_bps[0], scope: Universal },
        MetricDesc { name: "depth_within_50bps", get: |m| m.depth_within_bps[1], scope: Universal },
        MetricDesc { name: "depth_within_100bps", get: |m| m.depth_within_bps[2], scope: Universal },
        MetricDesc { name: "book_imbalance", get: |m| m.book_imbalance, scope: CdaOnly },
        MetricDesc { name: "total_book_depth", get: |m| m.total_book_depth, scope: CdaOnly },
        MetricDesc { name: "effective_spread_bps", get: |m| m.effective_spread_bps, scope: Universal },
        MetricDesc { name: "realized_spread_bps_1s", get: |m| m.realized_spread_bps_1s, scope: Universal },
        MetricDesc { name: "realized_spread_bps_5s", get: |m| m.realized_spread_bps_5s, scope: Universal },
        MetricDesc { name: "realized_spread_bps_30s", get: |m| m.realized_spread_bps_30s, scope: Universal },
        MetricDesc { name: "price_impact_bps_1s", get: |m| m.price_impact_bps_1s, scope: Universal },
        MetricDesc { name: "price_impact_bps_5s", get: |m| m.price_impact_bps_5s, scope: Universal },
        MetricDesc { name: "price_impact_bps_30s", get: |m| m.price_impact_bps_30s, scope: Universal },
        MetricDesc { name: "amihud_illiquidity", get: |m| m.amihud_illiquidity, scope: Universal },
        MetricDesc { name: "kyle_lambda", get: |m| m.kyle_lambda, scope: Universal },
        MetricDesc { name: "realized_volatility", get: |m| m.realized_volatility, scope: Universal },
        MetricDesc { name: "intra_interval_price_dispersion", get: |m| m.intra_interval_price_dispersion, scope: Universal },
        MetricDesc {
            name: "pricing_error_bps",
            get: |m| m.pricing_error_bps,
            scope: NeedsExternalData("requires an external oracle/mark-price feed; not in this dataset (see IntervalMetrics::pricing_error_bps)"),
        },
        MetricDesc { name: "executed_volume", get: |m| Some(m.executed_volume), scope: Universal },
        MetricDesc { name: "executed_notional", get: |m| Some(m.executed_notional), scope: Universal },
        MetricDesc { name: "vwap", get: |m| m.vwap, scope: Universal },
        MetricDesc { name: "trader_surplus", get: |m| Some(m.trader_surplus), scope: Universal },
        MetricDesc { name: "fill_rate", get: |m| m.fill_rate, scope: Universal },
        MetricDesc { name: "avg_time_to_execution_secs", get: |m| m.avg_time_to_execution_secs, scope: Universal },
        MetricDesc { name: "order_size_inflation", get: |m| m.order_size_inflation, scope: Universal },
        MetricDesc { name: "order_to_trade_ratio", get: |m| m.order_to_trade_ratio, scope: Universal },
        MetricDesc { name: "boundary_concentration", get: |m| m.boundary_concentration, scope: FbaOnly },
        MetricDesc { name: "throughput_orders_per_sec", get: |m| m.throughput_orders_per_sec, scope: Universal },
        MetricDesc { name: "avg_clearing_latency_micros", get: |m| m.avg_clearing_latency_micros, scope: Universal },
        MetricDesc { name: "unexecuted_residual_share", get: |m| m.unexecuted_residual_share, scope: FbaOnly },
    ]
}

#[derive(Clone, Copy, Debug)]
struct MetricAgg {
    n: u64,
    sum: f64,
    min: f64,
    max: f64,
}

impl MetricAgg {
    fn zero() -> Self {
        Self { n: 0, sum: 0.0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
    fn push(&mut self, v: f64) {
        self.n += 1;
        self.sum += v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }
}

#[derive(Clone, Debug)]
pub struct SummaryAccumulator {
    intervals_total: u64,
    trade_count_sum: u64,
    executed_volume_sum: f64,
    executed_notional_sum: f64,
    aggs: Vec<MetricAgg>,
}

impl SummaryAccumulator {
    pub fn new() -> Self {
        Self {
            intervals_total: 0,
            trade_count_sum: 0,
            executed_volume_sum: 0.0,
            executed_notional_sum: 0.0,
            aggs: vec![MetricAgg::zero(); metrics().len()],
        }
    }

    /// Fold one emitted interval row. Rows are folded in ascending bucket
    /// order across the whole run (the streaming flusher never revisits an
    /// earlier bucket), so the running `sum` matches, term for term, the
    /// left fold the old whole-series summary computed.
    pub fn fold(&mut self, m: &IntervalMetrics) {
        self.intervals_total += 1;
        self.trade_count_sum += m.trade_count;
        self.executed_volume_sum += m.executed_volume;
        self.executed_notional_sum += m.executed_notional;
        for (i, desc) in metrics().iter().enumerate() {
            if let Some(v) = (desc.get)(m) {
                self.aggs[i].push(v);
            }
        }
    }

    pub fn intervals_total(&self) -> u64 {
        self.intervals_total
    }
    pub fn trade_count_sum(&self) -> u64 {
        self.trade_count_sum
    }
    pub fn executed_volume_sum(&self) -> f64 {
        self.executed_volume_sum
    }
    pub fn executed_notional_sum(&self) -> f64 {
        self.executed_notional_sum
    }

    fn serialize(&self) -> String {
        let mut parts = vec![
            self.intervals_total.to_string(),
            self.trade_count_sum.to_string(),
            self.executed_volume_sum.to_string(),
            self.executed_notional_sum.to_string(),
        ];
        for (i, desc) in metrics().iter().enumerate() {
            let a = &self.aggs[i];
            parts.push(format!("{}={}:{}:{}:{}", desc.name, a.n, a.sum, a.min, a.max));
        }
        parts.join(";")
    }

    fn deserialize(s: &str) -> Result<Self, String> {
        let mut acc = SummaryAccumulator::new();
        let mut it = s.split(';');
        let head_err = || "truncated summary blob".to_string();
        acc.intervals_total = it.next().ok_or_else(head_err)?.parse().map_err(|_| "bad intervals_total")?;
        acc.trade_count_sum = it.next().ok_or_else(head_err)?.parse().map_err(|_| "bad trade_count_sum")?;
        acc.executed_volume_sum = it.next().ok_or_else(head_err)?.parse().map_err(|_| "bad executed_volume_sum")?;
        acc.executed_notional_sum = it.next().ok_or_else(head_err)?.parse().map_err(|_| "bad executed_notional_sum")?;

        let mut by_name: std::collections::HashMap<&str, MetricAgg> = std::collections::HashMap::new();
        for chunk in it {
            if chunk.is_empty() {
                continue;
            }
            let (name, rest) = chunk.split_once('=').ok_or("summary chunk missing '='")?;
            let mut f = rest.split(':');
            let n: u64 = f.next().ok_or("summary chunk missing n")?.parse().map_err(|_| "bad agg n")?;
            let sum: f64 = f.next().ok_or("summary chunk missing sum")?.parse().map_err(|_| "bad agg sum")?;
            let min: f64 = f.next().ok_or("summary chunk missing min")?.parse().map_err(|_| "bad agg min")?;
            let max: f64 = f.next().ok_or("summary chunk missing max")?.parse().map_err(|_| "bad agg max")?;
            by_name.insert(name, MetricAgg { n, sum, min, max });
        }
        for (i, desc) in metrics().iter().enumerate() {
            if let Some(a) = by_name.get(desc.name) {
                acc.aggs[i] = *a;
            }
        }
        Ok(acc)
    }

    /// One engine's block of per-metric avg/min/max — byte-identical to the
    /// old `inputs::simulate_cmd::stats_section` output.
    pub fn render_section(&self, label: &str) -> String {
        if self.intervals_total == 0 {
            return format!("\n{label} stats: (no intervals)\n");
        }
        let total = self.intervals_total as usize;
        let mut out = format!("\n{label} stats (avg/min/max across intervals where computable; n = computable/total intervals):\n");
        for (i, desc) in metrics().iter().enumerate() {
            out.push_str(&fmt_stat_row(desc.name, &self.aggs[i], total, desc.scope, label));
        }
        out
    }
}

impl Default for SummaryAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

fn fmt_stat_row(label: &str, a: &MetricAgg, total: usize, scope: Scope, engine: &str) -> String {
    match scope {
        Scope::FbaOnly if engine != "FBA" => return format!("  {label:<32} n/a — FBA-only (no batch window in a CDA)\n"),
        Scope::CdaOnly if engine != "CDA" => return format!("  {label:<32} n/a — CDA-only (no resting book to measure in FBA's uniform-price batch)\n"),
        Scope::NeedsExternalData(reason) => return format!("  {label:<32} n/a — {reason}\n"),
        _ => {}
    }
    let (avg, min, max) = if a.n == 0 {
        (None, None, None)
    } else {
        (Some(a.sum / a.n as f64), Some(a.min), Some(a.max))
    };
    let fmt = |v: Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or_else(|| "n/a".to_string());
    format!("  {label:<32} avg={:<12} min={:<12} max={:<12} (n={}/{total})\n", fmt(avg), fmt(min), fmt(max), a.n)
}

/// Trim a time-series CSV back to its header plus the first `keep` data
/// rows — used on resume when a crash left the CSV ahead of the checkpoint.
/// A no-op when the file already has exactly `keep` data rows.
pub fn truncate_data_rows(path: &Path, keep: u64) -> io::Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut lines = content.lines();
    let Some(header) = lines.next() else {
        return Ok(()); // empty file — caller will (re)write the header
    };
    let data: Vec<&str> = lines.collect();
    if data.len() as u64 == keep {
        return Ok(());
    }
    let mut out = String::with_capacity(content.len());
    out.push_str(header);
    out.push('\n');
    for line in data.into_iter().take(keep as usize) {
        out.push_str(line);
        out.push('\n');
    }
    fs::write(path, out)
}

/// `output/<slug>` for a resolved source path.
pub fn output_dir(source: &str) -> PathBuf {
    Path::new("output").join(slugify(source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::timeseries::IntervalMetrics;

    #[test]
    fn slugify_takes_the_last_component_and_sanitizes() {
        assert_eq!(slugify("data/order_statuses/sol"), "sol");
        assert_eq!(slugify("../data/sample/order_statuses/20251201"), "20251201");
        assert_eq!(slugify("weird name/with spaces & stuff"), "with_spaces___stuff");
        assert_eq!(slugify(""), "run");
    }

    #[test]
    fn checkpoint_round_trips_through_text() {
        let mut c = Checkpoint::fresh("data/order_statuses/sol".to_string(), 1_000_000_000, 3_660_000_000_000, 744);
        c.files_done = 12;
        c.last_file = "data/order_statuses/sol/20251201/sol_11.data.gz".to_string();
        c.anchor = Some(1_764_590_400_000_000_000);
        c.emitted_upto = 1_764_590_460_000_000_000;
        c.records_seen = 4_000_000;
        c.fba_rows_written = 720;
        c.cda_rows_written = 720;
        c.fba_prev_close = Some(126.5);
        c.cda_prev_close = None;
        c.fba_prev_clearing = Some(1_045_000.0);
        c.cda_prev_clearing = None;
        c.cda_late_dropped = 3;
        c.elapsed_secs = 812.5;

        let mut m = IntervalMetrics::empty("FBA", 0, 1_000_000_000);
        m.quoted_spread_bps = Some(2.5);
        m.trade_count = 4;
        m.executed_volume = 10.0;
        m.executed_notional = 1_260_000_000.0;
        c.fba_summary.fold(&m);
        c.fba_summary.fold(&m);

        let text = render(&c);
        let back = parse(&text).expect("round-trips");

        assert_eq!(back.source, c.source);
        assert_eq!(back.interval_ns, c.interval_ns);
        assert_eq!(back.files_done, 12);
        assert_eq!(back.anchor, c.anchor);
        assert_eq!(back.emitted_upto, c.emitted_upto);
        assert_eq!(back.fba_rows_written, 720);
        assert_eq!(back.fba_prev_close, Some(126.5));
        assert_eq!(back.cda_prev_close, None);
        assert_eq!(back.fba_prev_clearing, Some(1_045_000.0));
        assert_eq!(back.cda_prev_clearing, None);
        assert_eq!(back.cda_late_dropped, 3);
        assert_eq!(back.elapsed_secs, 812.5);
        assert_eq!(back.fba_summary.intervals_total(), 2);
        assert_eq!(back.fba_summary.trade_count_sum(), 8);
        // The rendered section must survive the round trip verbatim.
        assert_eq!(back.fba_summary.render_section("FBA"), c.fba_summary.render_section("FBA"));
    }

    #[test]
    fn parse_rejects_a_foreign_version() {
        let text = "version 999\nsource x\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn truncate_trims_extra_rows_and_leaves_matching_files_alone() {
        let dir = std::env::temp_dir().join(format!("mkt_ckpt_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.csv");
        fs::write(&p, "header\nr0\nr1\nr2\nr3\n").unwrap();

        truncate_data_rows(&p, 2).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "header\nr0\nr1\n");

        // Already the right length -> untouched.
        truncate_data_rows(&p, 2).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "header\nr0\nr1\n");

        fs::remove_dir_all(&dir).ok();
    }
}

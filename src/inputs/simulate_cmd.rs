//! The `simulate` command: streams a real order-status archive (CSV or
//! the binary/gzip files under `data/order_statuses/`, see
//! `inputs::simulator::collect_input_files`/`stream_records`) through
//! fresh, isolated `FbaOrderBook`/`CdaOrderBook` instances, recording the
//! full time-series metric catalogue (`metrics::timeseries`) for both
//! along the way, and writes the result to `output/`.
//!
//! Deliberately separate from the live session's own `fba`/`cda` (owned by
//! `inputs::cli::run`) — a multi-million-record replay has no business
//! mutating what `add`/`load`/`metrics`/`orderbook` show the user
//! afterward.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use colored::Colorize;

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::inputs::progress;
use crate::inputs::simulator;
use crate::metrics::timeseries::{self, MetricsRecorder};
use crate::types::{EngineKind, Order, Side};

const DEFAULT_INTERVAL_SECS: u64 = 1;
const NS_PER_SEC: u64 = 1_000_000_000;

pub fn run(path_str: &str, interval_secs: Option<u64>) {
    // `simulate all` is shorthand for running `btc`, `eth`, then `sol` back
    // to back — same idea as `download all`/`extract all`, and just as
    // simple to implement: each coin gets its own complete, independent
    // run (files, engines, progress bar, output) rather than trying to
    // merge three unrelated datasets into one combined replay.
    if path_str.eq_ignore_ascii_case("all") {
        for coin in ["btc", "eth", "sol"] {
            run(coin, interval_secs);
        }
        return;
    }

    let interval_secs = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);
    let interval_width_ns = interval_secs * NS_PER_SEC;

    // `simulate btc|eth|sol` is shorthand for the directory `download
    // <coin>` populates — resolved here rather than in the CLI parser so
    // `CliCommand::Simulate` stays a plain free-form path, and any literal
    // path argument (e.g. the CSV sample used in tests) is unaffected: it
    // simply won't match one of these three keywords.
    let coin_shorthand = matches!(path_str.to_lowercase().as_str(), "btc" | "eth" | "sol");
    let resolved_path = if coin_shorthand { format!("data/order_statuses/{}", path_str.to_lowercase()) } else { path_str.to_string() };
    let root = Path::new(&resolved_path);

    let files = match simulator::collect_input_files(root) {
        Ok(files) if !files.is_empty() => files,
        Ok(_) if coin_shorthand => {
            println!("{}", format!("[ERROR] No .csv/.gz files found under '{resolved_path}'. Run 'download {path_str}' first?").red());
            return;
        }
        Ok(_) => {
            println!("{}", format!("[ERROR] No .csv/.gz files found under '{resolved_path}'.").red());
            return;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Failed to read '{resolved_path}': {err}").red());
            return;
        }
    };

    println!("{}", format!("==> Simulating {} file(s) from '{resolved_path}', interval={interval_secs}s ...", files.len()).cyan());
    std::io::stdout().flush().ok();
    let wall_clock_start = Instant::now();

    // Total on-disk size of every file about to be streamed — cheap (just
    // `stat`s, no decompression) and the whole basis for the live progress
    // bar below: `simulator::stream_records` counts raw bytes actually
    // read from disk (compressed size for `.gz`) as it goes, so comparing
    // that running count against this total gives a real percentage
    // through the run, not just an unbounded "N records so far".
    let total_bytes: u64 = files.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let bytes_read = Arc::new(AtomicU64::new(0));
    let records_seen = Arc::new(AtomicU64::new(0));

    let mut fba = FbaOrderBook::new();
    let mut cda = CdaOrderBook::new();
    let mut fba_recorder = MetricsRecorder::new(EngineKind::Fba, interval_width_ns);
    let mut cda_recorder = MetricsRecorder::new(EngineKind::Cda, interval_width_ns);

    // FBA has no continuous clock of its own — `simulate` triggers a
    // clear() whenever a record's own timestamp crosses the next
    // interval-width boundary in event-time (not wall-clock time).
    let mut next_fba_boundary: Option<u64> = None;
    let mut fba_batch_open_ts: Option<u64> = None;
    let mut last_seen_ts: Option<u64> = None;

    // Cache of the CDA book's depth-derived figures, updated only when a
    // record could actually have changed the book (see `is_actionable`
    // below) — recomputing these from scratch every record turns the whole
    // replay into O(records x book size), the dominant cost once the book
    // grows over a trading day. When a record is a no-op (rejection,
    // cancellation of an already-gone order, fill, un-triggered conditional
    // order), `cda.bids`/`cda.asks` are byte-for-byte unchanged from the
    // previous record, so the previous values are still exactly correct.
    let mut cached_bid_depth: u128 = 0;
    let mut cached_ask_depth: u128 = 0;
    let mut cached_depth_sched = [(0u128, 0u128); timeseries::DEPTH_BPS_THRESHOLDS.len()];

    let stream_result = progress::run_with_progress(
        (total_bytes > 0).then_some(total_bytes),
        {
            let bytes_read = Arc::clone(&bytes_read);
            move || bytes_read.load(Ordering::Relaxed)
        },
        progress::human_bytes,
        {
            let records_seen = Arc::clone(&records_seen);
            move || format!("  {} record(s)", records_seen.load(Ordering::Relaxed))
        },
        || {
            simulator::stream_records(&files, &bytes_read, |order: Order| {
                records_seen.fetch_add(1, Ordering::Relaxed);
                last_seen_ts = Some(order.ts);

                // Both recorders see the raw message stream before any gating —
                // same "sees rejections/cancellations too" principle the message
                // log has always used, needed for order-to-trade ratio etc.
                let msg = timeseries::OrderMessage {
                    ts: order.ts,
                    oid: order.oid,
                    user_id: order.user_id.clone(),
                    side: order.side(),
                    limit_price: order.limit_price(),
                    quantity: order.orig_sz,
                    accepted: order.is_new_live_order(),
                };
                fba_recorder.record_message(msg.clone());
                cda_recorder.record_message(msg);

                if next_fba_boundary.is_none() {
                    next_fba_boundary = Some(order.ts + interval_width_ns);
                    fba_batch_open_ts = Some(order.ts);
                }
                while order.ts >= next_fba_boundary.expect("just ensured Some above") {
                    let open_ts = fba_batch_open_ts.expect("set alongside next_fba_boundary");
                    let close_ts = next_fba_boundary.expect("checked by the while condition");
                    clear_fba_batch(&mut fba, &mut fba_recorder, open_ts, close_ts);
                    fba_batch_open_ts = Some(close_ts);
                    next_fba_boundary = Some(close_ts + interval_width_ns);
                }

                // Rejections, cancellations of already-gone orders, fills, and
                // un-triggered conditional orders are guaranteed no-ops in both
                // `FbaOrderBook::submit` and `CdaOrderBook::submit` (see their own
                // `is_new_live_order`/`is_cancellation` gating) — skip the clone +
                // call entirely for those rather than paying for a call that's
                // known in advance to do nothing.
                let is_actionable = order.is_new_live_order() || order.is_cancellation();

                if is_actionable {
                    fba.submit(order.clone());
                }

                let reference_price = midpoint(cda.best_bid(), cda.best_ask());
                let cda_start = Instant::now();
                let trades = if is_actionable { cda.submit(order.clone()) } else { Vec::new() };
                let compute_time = cda_start.elapsed();

                for trade in &trades {
                    cda_recorder.record_trade(timeseries::TradeEvent {
                        trade: trade.clone(),
                        reference_price,
                        aggressor_side: Some(order.side()),
                    });
                }

                let best_bid = cda.best_bid();
                let best_ask = cda.best_ask();
                // Bids/asks are kept sorted best-first (binary-search insertion —
                // see `CdaOrderBook::bid_sort_key`/`ask_sort_key`), so index 0 IS
                // the touch. Separate from `bid_depth`/`ask_depth` below, which sum
                // the whole book.
                let best_bid_qty: u128 = cda.bids.first().map(|o| o.remaining).unwrap_or(0);
                let best_ask_qty: u128 = cda.asks.first().map(|o| o.remaining).unwrap_or(0);

                // Only re-scan the whole book when this record could have changed
                // it — otherwise `cda.bids`/`cda.asks` (and therefore best_bid/
                // best_ask above) are identical to last time, so the cached values
                // are still exactly right.
                if is_actionable {
                    cached_bid_depth = cda.bids.iter().map(|o| o.remaining).sum();
                    cached_ask_depth = cda.asks.iter().map(|o| o.remaining).sum();
                    cached_depth_sched = match midpoint(best_bid, best_ask) {
                        Some(mid) => timeseries::depth_schedule(
                            mid,
                            cda.bids
                                .iter()
                                .map(|o| (o.limit_price().unwrap_or(mid), o.side(), o.remaining))
                                .chain(cda.asks.iter().map(|o| (o.limit_price().unwrap_or(mid), o.side(), o.remaining))),
                        ),
                        None => [(0, 0); timeseries::DEPTH_BPS_THRESHOLDS.len()],
                    };
                }
                cda_recorder.record_book_snapshot(timeseries::BookSnapshot {
                    ts: order.ts,
                    best_bid,
                    best_ask,
                    best_bid_qty,
                    best_ask_qty,
                    bid_depth: cached_bid_depth,
                    ask_depth: cached_ask_depth,
                    depth_schedule: cached_depth_sched,
                    compute_time,
                });
            })
        },
    );

    let stats = match stream_result {
        Ok(stats) => stats,
        Err(err) => {
            println!("{}", format!("[ERROR] Streaming failed: {err}").red());
            return;
        }
    };

    // Flush whatever's left in the final, still-open FBA batch.
    if !fba.pending_orders.is_empty() {
        let open_ts = fba_batch_open_ts.unwrap_or(0);
        let close_ts = last_seen_ts.unwrap_or(open_ts);
        clear_fba_batch(&mut fba, &mut fba_recorder, open_ts, close_ts.max(open_ts));
    }

    let elapsed = wall_clock_start.elapsed();
    println!(
        "{}",
        format!(
            "[OK] Streamed {} file(s), {} record(s) seen ({} skipped), in {:.1}s wall-clock.",
            stats.files_processed,
            stats.records_seen,
            stats.records_skipped,
            elapsed.as_secs_f64()
        )
        .green()
    );

    let fba_series = fba_recorder.finalize();
    let cda_series = cda_recorder.finalize();
    println!("FBA: {} interval(s)  |  CDA: {} interval(s)", fba_series.len(), cda_series.len());

    match write_output(&resolved_path, &stats, elapsed, &fba_series, &cda_series) {
        Ok(dir) => println!("{}", format!("[OK] Wrote time series + summary to {}", dir.display()).green()),
        Err(err) => println!("{}", format!("[ERROR] Failed to write output: {err}").red()),
    }
}

/// Clears FBA's current batch (if it has anything queued) and records a
/// `BatchClearedEvent` either way — a batch that finds no crossing price
/// is still worth knowing about (see `timeseries::BatchClearedEvent`'s
/// `clearing_price: None` case).
fn clear_fba_batch(fba: &mut FbaOrderBook, recorder: &mut MetricsRecorder, batch_open_ts: u64, batch_close_ts: u64) {
    if fba.pending_orders.is_empty() {
        return;
    }

    // Snapshot the batch's own order list BEFORE `clear()` consumes it
    // (needed for depth_schedule around the clearing price — clear() only
    // leaves the POST-clear residual behind), and capture the reference
    // price BEFORE clear() potentially updates `last_clearing_price`.
    let snapshot: Vec<(Option<u128>, Side, u128)> = fba.pending_orders.iter().map(|o| (o.limit_price(), o.side(), o.remaining)).collect();
    let reference_before = fba.last_clearing_price;

    let clear_start = Instant::now();
    let result = fba.clear();
    let compute_time = clear_start.elapsed();

    let Some(clearing) = result else {
        recorder.record_batch(timeseries::BatchClearedEvent {
            ts: batch_close_ts,
            batch_open_ts,
            clearing_price: None,
            demand_at_price: 0,
            supply_at_price: 0,
            traded_quantity: 0,
            unexecuted_quantity: 0,
            best_unfilled_buy: fba.best_unfilled_buy(),
            best_unfilled_sell: fba.best_unfilled_sell(),
            depth_schedule: [(0, 0); timeseries::DEPTH_BPS_THRESHOLDS.len()],
            compute_time,
        });
        return;
    };

    for trade in &clearing.trades {
        recorder.record_trade(timeseries::TradeEvent {
            trade: trade.clone(),
            reference_price: reference_before,
            aggressor_side: None, // no taker/maker distinction in a uniform-price batch
        });
    }

    let depth_sched = timeseries::depth_schedule(clearing.clearing_price, snapshot.into_iter().map(|(price, side, qty)| (price.unwrap_or(clearing.clearing_price), side, qty)));

    recorder.record_batch(timeseries::BatchClearedEvent {
        ts: batch_close_ts,
        batch_open_ts,
        clearing_price: Some(clearing.clearing_price),
        demand_at_price: clearing.demand_at_price,
        supply_at_price: clearing.supply_at_price,
        traded_quantity: clearing.traded_quantity,
        // The heavier ELIGIBLE side's leftover (`|demand - supply|`), not a
        // full-residual-orders sum — see `FbaOrderBook::unexecuted_residual_share`'s
        // doc comment for why the latter can push a share past 1. Same fix,
        // applied here too rather than reintroducing that bug in a new place.
        unexecuted_quantity: clearing.demand_at_price.abs_diff(clearing.supply_at_price),
        best_unfilled_buy: fba.best_unfilled_buy(),
        best_unfilled_sell: fba.best_unfilled_sell(),
        depth_schedule: depth_sched,
        compute_time,
    });
}

fn midpoint(best_bid: Option<u128>, best_ask: Option<u128>) -> Option<u128> {
    match (best_bid, best_ask) {
        (Some(b), Some(a)) => Some((b + a) / 2),
        (Some(b), None) => Some(b),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn write_output(
    source: &str,
    stats: &simulator::RunStats,
    elapsed: std::time::Duration,
    fba_series: &[timeseries::IntervalMetrics],
    cda_series: &[timeseries::IntervalMetrics],
) -> std::io::Result<std::path::PathBuf> {
    let out_dir = Path::new("output");
    std::fs::create_dir_all(out_dir)?;

    // Tag every file from this run with when it was produced, so repeated
    // `simulate` runs accumulate in output/ instead of clobbering each
    // other's results.
    let ts = run_timestamp();

    std::fs::write(out_dir.join(format!("fba_timeseries_{ts}.csv")), timeseries::to_csv(fba_series))?;
    std::fs::write(out_dir.join(format!("cda_timeseries_{ts}.csv")), timeseries::to_csv(cda_series))?;

    let summary = format!(
        "Simulation summary\n\
         ===================\n\
         Generated:               {ts} (UTC)\n\
         Source:                  {source}\n\
         Files processed:         {}\n\
         Records seen:            {}\n\
         Records skipped:         {}\n\
         Wall-clock duration:     {:.1}s\n\
         \n\
         FBA intervals:           {}\n\
         CDA intervals:           {}\n\
         \n\
         FBA totals:  trades={}  volume={:.2} SOL  notional=${:.2}\n\
         CDA totals:  trades={}  volume={:.2} SOL  notional=${:.2}\n\
         {}\
         {}\
         \n\
         Note: *_timeseries.csv's own `executed_notional`/price-bps columns\n\
         keep the raw PRICE_SCALE (1e6) fixed-point convention used\n\
         internally throughout this crate — divide price-denominated values\n\
         by 1,000,000 for real USD (already done for the dollar figures\n\
         above). Quantities (volume, depth) are already whole SOL units.\n\
         \n\
         \"avg\" below means the mean across only the intervals where that\n\
         metric was actually computable (see IntervalMetrics' doc comment —\n\
         `None` always means \"not computable from what was recorded,\" never\n\
         a silent zero, so intervals with no value for a given metric are\n\
         excluded from its average rather than pulling it toward zero).\n",
        stats.files_processed,
        stats.records_seen,
        stats.records_skipped,
        elapsed.as_secs_f64(),
        fba_series.len(),
        cda_series.len(),
        fba_series.iter().map(|m| m.trade_count).sum::<u64>(),
        fba_series.iter().map(|m| m.executed_volume).sum::<f64>(),
        fba_series.iter().map(|m| m.executed_notional).sum::<f64>() / crate::types::PRICE_SCALE as f64,
        cda_series.iter().map(|m| m.trade_count).sum::<u64>(),
        cda_series.iter().map(|m| m.executed_volume).sum::<f64>(),
        cda_series.iter().map(|m| m.executed_notional).sum::<f64>() / crate::types::PRICE_SCALE as f64,
        stats_section("FBA", fba_series),
        stats_section("CDA", cda_series),
    );
    std::fs::write(out_dir.join(format!("summary_{ts}.txt")), summary)?;

    Ok(out_dir.to_path_buf())
}

/// avg/min/max of `f(interval)` across only the intervals where it's `Some`
/// — `None` intervals are excluded entirely rather than counted as zero
/// (matching what `None` means throughout this catalogue). `n` is how many
/// intervals actually contributed, so a structurally-always-`None` metric
/// (e.g. `pricing_error_bps`) is visibly distinct from one that's merely
/// sparse.
struct Stat {
    avg: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
    n: usize,
}

fn stat_of(series: &[timeseries::IntervalMetrics], f: impl Fn(&timeseries::IntervalMetrics) -> Option<f64>) -> Stat {
    let values: Vec<f64> = series.iter().filter_map(&f).collect();
    if values.is_empty() {
        return Stat { avg: None, min: None, max: None, n: 0 };
    }
    Stat {
        avg: Some(values.iter().sum::<f64>() / values.len() as f64),
        min: Some(values.iter().cloned().fold(f64::INFINITY, f64::min)),
        max: Some(values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        n: values.len(),
    }
}

/// Same as `stat_of`, but for the catalogue's non-`Option` per-interval
/// fields (`executed_volume`, `executed_notional`, `trader_surplus`) —
/// every interval counts (there's no "not computable" case for these,
/// a quiet interval is legitimately `0.0`, not missing).
fn stat_of_f64(series: &[timeseries::IntervalMetrics], f: impl Fn(&timeseries::IntervalMetrics) -> f64) -> Stat {
    stat_of(series, |m| Some(f(m)))
}

/// Why a metric can legitimately be `n/a` for *every* interval, distinct
/// from "just happened to have no trades this run" — a bare "n/a" reads
/// the same in both cases, which is exactly what made the old summary
/// noisy. `Universal` metrics still show avg/min/max/(n=computable/total)
/// as before; the other three print a one-line reason instead of three
/// `n/a`s, and only for the engine(s) the reason actually applies to.
#[derive(Clone, Copy)]
enum Scope {
    Universal,
    FbaOnly,
    CdaOnly,
    /// Structurally uncomputable from this dataset alone, for either
    /// engine — e.g. needs an external oracle/mark-price feed.
    NeedsExternalData(&'static str),
}

fn fmt_stat_row(label: &str, s: &Stat, total: usize, scope: Scope, engine: &str) -> String {
    match scope {
        Scope::FbaOnly if engine != "FBA" => return format!("  {label:<32} n/a — FBA-only (no batch window in a CDA)\n"),
        Scope::CdaOnly if engine != "CDA" => return format!("  {label:<32} n/a — CDA-only (no resting book to measure in FBA's uniform-price batch)\n"),
        Scope::NeedsExternalData(reason) => return format!("  {label:<32} n/a — {reason}\n"),
        _ => {}
    }
    let fmt = |v: Option<f64>| v.map(|x| format!("{x:.4}")).unwrap_or_else(|| "n/a".to_string());
    format!("  {label:<32} avg={:<12} min={:<12} max={:<12} (n={}/{total})\n", fmt(s.avg), fmt(s.min), fmt(s.max), s.n)
}

/// One engine's block of per-metric avg/min/max for the detailed summary —
/// every field in `IntervalMetrics` that isn't purely structural
/// (`engine`/`interval_start`/`interval_width`), so this stays in sync with
/// the catalogue by construction rather than by remembering to add a line
/// here every time a metric is added there.
fn stats_section(label: &str, series: &[timeseries::IntervalMetrics]) -> String {
    if series.is_empty() {
        return format!("\n{label} stats: (no intervals)\n");
    }
    let total = series.len();

    let rows: Vec<(&str, Stat, Scope)> = vec![
        ("quoted_spread_bps", stat_of(series, |m| m.quoted_spread_bps), Scope::Universal),
        ("depth_at_best", stat_of(series, |m| m.depth_at_best), Scope::Universal),
        ("depth_within_10bps", stat_of(series, |m| m.depth_within_bps[0]), Scope::Universal),
        ("depth_within_50bps", stat_of(series, |m| m.depth_within_bps[1]), Scope::Universal),
        ("depth_within_100bps", stat_of(series, |m| m.depth_within_bps[2]), Scope::Universal),
        ("book_imbalance", stat_of(series, |m| m.book_imbalance), Scope::CdaOnly),
        ("total_book_depth", stat_of(series, |m| m.total_book_depth), Scope::CdaOnly),
        ("effective_spread_bps", stat_of(series, |m| m.effective_spread_bps), Scope::Universal),
        ("realized_spread_bps_1s", stat_of(series, |m| m.realized_spread_bps_1s), Scope::Universal),
        ("realized_spread_bps_5s", stat_of(series, |m| m.realized_spread_bps_5s), Scope::Universal),
        ("realized_spread_bps_30s", stat_of(series, |m| m.realized_spread_bps_30s), Scope::Universal),
        ("price_impact_bps_1s", stat_of(series, |m| m.price_impact_bps_1s), Scope::Universal),
        ("price_impact_bps_5s", stat_of(series, |m| m.price_impact_bps_5s), Scope::Universal),
        ("price_impact_bps_30s", stat_of(series, |m| m.price_impact_bps_30s), Scope::Universal),
        ("amihud_illiquidity", stat_of(series, |m| m.amihud_illiquidity), Scope::Universal),
        ("realized_volatility", stat_of(series, |m| m.realized_volatility), Scope::Universal),
        ("intra_interval_price_dispersion", stat_of(series, |m| m.intra_interval_price_dispersion), Scope::Universal),
        ("pricing_error_bps", stat_of(series, |m| m.pricing_error_bps), Scope::NeedsExternalData("requires an external oracle/mark-price feed; not in this dataset (see IntervalMetrics::pricing_error_bps)")),
        ("executed_volume", stat_of_f64(series, |m| m.executed_volume), Scope::Universal),
        ("executed_notional", stat_of_f64(series, |m| m.executed_notional), Scope::Universal),
        ("vwap", stat_of(series, |m| m.vwap), Scope::Universal),
        ("trader_surplus", stat_of_f64(series, |m| m.trader_surplus), Scope::Universal),
        ("fill_rate", stat_of(series, |m| m.fill_rate), Scope::Universal),
        ("avg_time_to_execution_secs", stat_of(series, |m| m.avg_time_to_execution_secs), Scope::Universal),
        ("order_size_inflation", stat_of(series, |m| m.order_size_inflation), Scope::Universal),
        ("order_to_trade_ratio", stat_of(series, |m| m.order_to_trade_ratio), Scope::Universal),
        ("boundary_concentration", stat_of(series, |m| m.boundary_concentration), Scope::FbaOnly),
        ("throughput_orders_per_sec", stat_of(series, |m| m.throughput_orders_per_sec), Scope::Universal),
        ("avg_clearing_latency_micros", stat_of(series, |m| m.avg_clearing_latency_micros), Scope::Universal),
        ("unexecuted_residual_share", stat_of(series, |m| m.unexecuted_residual_share), Scope::FbaOnly),
    ];

    let mut out = format!("\n{label} stats (avg/min/max across intervals where computable; n = computable/total intervals):\n");
    for (name, stat, scope) in &rows {
        out.push_str(&fmt_stat_row(name, stat, total, *scope, label));
    }
    out
}

/// Formats the current UTC time as `YYYYMMDD_HHMMSS`, for tagging this
/// run's output filenames. Pure integer arithmetic — no date/time crate
/// needed: `civil_from_days` below is the inverse of
/// `inputs::simulator::days_from_civil` (both Howard Hinnant's public-domain
/// algorithm), so this is just running that same date math backwards.
fn run_timestamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let total_secs = now.as_secs() as i64;
    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}{month:02}{day:02}_{hour:02}{minute:02}{second:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `civil_from_days` is the inverse of `inputs::simulator::days_from_civil`
    /// — same known-correct reference points that function's own tests use
    /// (`parses_dataset_timestamps` there), run backwards: days-since-epoch
    /// -> (year, month, day) instead of the other way around.
    #[test]
    fn civil_from_days_round_trips_known_dates() {
        assert_eq!(civil_from_days(20423), (2025, 12, 1)); // 2025-12-01
        assert_eq!(civil_from_days(20089), (2025, 1, 1)); // 2025-01-01 (epoch 1735689600)
        assert_eq!(civil_from_days(0), (1970, 1, 1)); // the epoch itself
    }

    #[test]
    fn run_timestamp_has_the_expected_shape() {
        let ts = run_timestamp();
        assert_eq!(ts.len(), 15, "expected YYYYMMDD_HHMMSS, got '{ts}'");
        assert_eq!(ts.as_bytes()[8], b'_');
        assert!(ts.chars().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit()));
    }

    #[test]
    fn stat_of_skips_none_and_computes_avg_min_max() {
        let mut a = timeseries::IntervalMetrics::empty("FBA", 0, 1_000_000_000);
        a.quoted_spread_bps = Some(2.0);
        let mut b = timeseries::IntervalMetrics::empty("FBA", 1, 1_000_000_000);
        b.quoted_spread_bps = None; // must be excluded, not counted as 0.0
        let mut c = timeseries::IntervalMetrics::empty("FBA", 2, 1_000_000_000);
        c.quoted_spread_bps = Some(4.0);

        let stat = stat_of(&[a, b, c], |m| m.quoted_spread_bps);
        assert_eq!(stat.avg, Some(3.0), "mean of [2.0, None, 4.0] should be 3.0, not 2.0 (which is what treating None as 0 would give)");
        assert_eq!(stat.min, Some(2.0));
        assert_eq!(stat.max, Some(4.0));
        assert_eq!(stat.n, 2, "only the 2 Some(..) intervals should count, out of 3 total");
    }
}

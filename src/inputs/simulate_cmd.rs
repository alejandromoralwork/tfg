//! The `simulate` command: streams a real order-status archive (CSV or the
//! binary/gzip files under `data/order_statuses/`, via
//! `inputs::simulator::collect_input_files` + a per-file `stream_file` loop
//! driven from here) through fresh,
//! isolated `FbaOrderBook`/`CdaOrderBook` instances, recording the full
//! time-series metric catalogue (`metrics::timeseries`) for both along the
//! way, and writing the result to `output/<slug>/`.
//!
//! Deliberately separate from the live session's own `fba`/`cda` (owned by
//! `inputs::cli::run`) — a multi-million-record replay has no business
//! mutating what `add`/`load`/`metrics`/`orderbook` show the user
//! afterward.
//!
//! ## Streaming, incremental output, resume
//!
//! A full-month archive is far too large to buffer every event and only
//! write once at the end, and a run that takes days must survive being
//! interrupted. So the replay is processed one input file at a time, and
//! after each file:
//!
//! - `MetricsRecorder::emit` flushes every interval bucket that neither this
//!   file's own tail nor the *next* file's first timestamp
//!   (`simulator::peek_first_ts`) can still be within `MARKOUT_GUARD_SECS`
//!   of — so no later file can add to it and its markout forward-mids
//!   already exist. Those rows are *appended* to
//!   `output/<slug>/{fba,cda}_timeseries.csv` and their events dropped, so
//!   the CSV grows once per input file and memory stays bounded to ~one
//!   file's span of recent activity.
//! - `output/<slug>/checkpoint.txt` is rewritten (atomically) with how many
//!   files are done, the cumulative counters, the bucket-grid cursor, and
//!   the running summary accumulators.
//!
//! Re-running `simulate` on the same source picks up from that checkpoint:
//! finished files are skipped and the CSVs are appended to (trimmed first
//! back to the checkpoint's row counts, in case a crash left them ahead).
//! Resume is approximate — the in-flight event window and the engine books
//! are not persisted, so a resumed run leaves a short gap of empty interval
//! rows (~one file's span, or `MARKOUT_GUARD_SECS` for the very last flush)
//! at the seam; everything after it is exact. `summary.txt` is written once,
//! at completion.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use colored::Colorize;

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::inputs::progress;
use crate::inputs::replay_checkpoint::{self, Checkpoint};
use crate::inputs::simulator;
use crate::metrics::timeseries::{self, Carry, IntervalMetrics, MetricsRecorder};
use crate::types::{EngineKind, Order, Side, PRICE_SCALE};

const DEFAULT_INTERVAL_SECS: u64 = 1;
const NS_PER_SEC: u64 = 1_000_000_000;

/// How far event-time must move past a metric bucket's end before that
/// bucket is flushed to CSV — just enough that its own forward-looking
/// markout mids are already observed. It only has to exceed the widest
/// forward horizon any metric uses: the 30s realized-spread markout and the
/// 5s `kyle_lambda` markout. It is NOT the flush cadence — a file boundary
/// is (see `flush_hi` and `simulator::peek_first_ts`): after each input file
/// `simulate` flushes every bucket that neither this file's own tail nor the
/// *next* file's start can still be within `MARKOUT_GUARD_SECS` of, so the
/// CSV grows once per file instead of once per ~hour.
const MARKOUT_GUARD_SECS: u64 = 35;

/// Exit codes: 0 ok, 1 a run-time failure (streaming / IO), 2 a bad request
/// (no data found, incompatible checkpoint). Returned so a non-interactive
/// caller (`cli::run_once`, GCP Batch) can retry or fail the task.
pub fn run(path_str: &str, interval_secs: Option<u64>) -> i32 {
    // `simulate all` is shorthand for running `btc`, `eth`, then `sol` back
    // to back — same idea as `download all`/`extract all`, and just as
    // simple to implement: each coin gets its own complete, independent
    // run (files, engines, progress bar, output) rather than trying to
    // merge three unrelated datasets into one combined replay.
    if path_str.eq_ignore_ascii_case("all") {
        let mut code = 0;
        for coin in ["btc", "eth", "sol"] {
            code = code.max(run(coin, interval_secs));
        }
        return code;
    }

    let interval_secs = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);
    let interval_width_ns = interval_secs * NS_PER_SEC;
    let markout_guard_ns = MARKOUT_GUARD_SECS.saturating_mul(NS_PER_SEC);

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
            return 2;
        }
        Ok(_) => {
            println!("{}", format!("[ERROR] No .csv/.gz files found under '{resolved_path}'.").red());
            return 2;
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Failed to read '{resolved_path}': {err}").red());
            return 2;
        }
    };

    let out_dir = replay_checkpoint::output_dir(&resolved_path);
    let fba_csv = out_dir.join(replay_checkpoint::FBA_CSV);
    let cda_csv = out_dir.join(replay_checkpoint::CDA_CSV);

    // ---- resume-or-fresh decision ----
    let prior = match Checkpoint::load(&out_dir) {
        Ok(p) => p,
        Err(err) => {
            println!("{}", format!("[ERROR] Couldn't read existing checkpoint in {}: {err}", out_dir.display()).red());
            return 2;
        }
    };
    let (mut ckpt, resuming) = match prior {
        Some(c) => {
            if c.source != resolved_path || c.interval_ns != interval_width_ns || c.files_total != files.len() {
                println!(
                    "{}",
                    format!(
                        "[ERROR] {} holds a different run (source / interval / file count changed). Delete that directory to start over.",
                        out_dir.display()
                    )
                    .red()
                );
                return 2;
            }
            if c.complete {
                println!("{}", format!("[OK] '{resolved_path}' is already fully processed — see {}. Delete that directory to re-run.", out_dir.display()).green());
                return 0;
            }
            (c, true)
        }
        None => {
            if let Err(err) = fs::create_dir_all(&out_dir) {
                println!("{}", format!("[ERROR] Couldn't create {}: {err}", out_dir.display()).red());
                return 1;
            }
            (Checkpoint::fresh(resolved_path.clone(), interval_width_ns, markout_guard_ns, files.len()), false)
        }
    };

    // A checkpoint with nothing flushed yet means the previous run was
    // interrupted before its first per-file flush — none of its events were
    // persisted, so there's nothing to resume onto. Restart the replay from
    // the top (no CSV rows exist to duplicate); only the cumulative
    // wall-clock carries over.
    let restart_from_scratch = resuming && ckpt.fba_rows_written == 0 && ckpt.cda_rows_written == 0;
    if restart_from_scratch {
        let kept_elapsed = ckpt.elapsed_secs;
        ckpt = Checkpoint::fresh(resolved_path.clone(), interval_width_ns, markout_guard_ns, files.len());
        ckpt.elapsed_secs = kept_elapsed;
    }
    let appending = resuming && !restart_from_scratch;

    let header_line = format!("{}\n", timeseries::csv_header());
    if appending {
        // A crash between appending rows and rewriting the checkpoint can
        // leave a CSV ahead of `*_rows_written` — trim it back so appends
        // resume exactly where the checkpoint says.
        if let Err(err) = replay_checkpoint::truncate_data_rows(&fba_csv, ckpt.fba_rows_written).and_then(|_| replay_checkpoint::truncate_data_rows(&cda_csv, ckpt.cda_rows_written)) {
            println!("{}", format!("[ERROR] Couldn't reconcile existing CSVs against the checkpoint: {err}").red());
            return 1;
        }
        for p in [&fba_csv, &cda_csv] {
            if !p.exists() {
                let _ = fs::write(p, &header_line); // deleted between runs — recreate the header
            }
        }
        println!(
            "{}",
            format!(
                "==> Resuming '{resolved_path}' (interval={interval_secs}s): {}/{} file(s) done, {} FBA + {} CDA row(s) already written.",
                ckpt.files_done,
                files.len(),
                ckpt.fba_rows_written,
                ckpt.cda_rows_written
            )
            .cyan()
        );
    } else {
        if let Err(err) = fs::write(&fba_csv, &header_line).and_then(|_| fs::write(&cda_csv, &header_line)) {
            println!("{}", format!("[ERROR] Couldn't create output CSVs in {}: {err}", out_dir.display()).red());
            return 1;
        }
        let lead = if restart_from_scratch { "Restarting (prior run stopped before its first flush)" } else { "Simulating" };
        println!("{}", format!("==> {lead} {} file(s) from '{resolved_path}', interval={interval_secs}s -> {} ...", files.len(), out_dir.display()).cyan());
    }
    io::stdout().flush().ok();

    let base_elapsed_secs = ckpt.elapsed_secs;
    let todo_files: Vec<PathBuf> = files[ckpt.files_done.min(files.len())..].to_vec();

    // ---- engines + recorders (books start fresh even on resume) ----
    let mut fba = FbaOrderBook::new();
    let mut cda = CdaOrderBook::new();
    let (mut fba_recorder, mut cda_recorder) = match ckpt.anchor {
        Some(a) => (
            MetricsRecorder::resume(
                EngineKind::Fba,
                interval_width_ns,
                a,
                ckpt.emitted_upto,
                Carry { prev_close: ckpt.fba_prev_close, prev_clearing: ckpt.fba_prev_clearing },
                ckpt.fba_late_dropped,
            ),
            MetricsRecorder::resume(
                EngineKind::Cda,
                interval_width_ns,
                a,
                ckpt.emitted_upto,
                Carry { prev_close: ckpt.cda_prev_close, prev_clearing: ckpt.cda_prev_clearing },
                ckpt.cda_late_dropped,
            ),
        ),
        None => (MetricsRecorder::new(EngineKind::Fba, interval_width_ns), MetricsRecorder::new(EngineKind::Cda, interval_width_ns)),
    };
    let mut fba_summary = ckpt.fba_summary.clone();
    let mut cda_summary = ckpt.cda_summary.clone();

    let mut anchor: Option<u64> = ckpt.anchor;
    let mut max_seen_ts: u64 = ckpt.emitted_upto;

    // Message log for the currently-retained window only (pruned after each
    // file to the same bucket boundary the recorders prune to). Shared by
    // reference into both recorders' `emit`.
    let mut messages: Vec<timeseries::OrderMessage> = Vec::new();

    // FBA has no continuous clock of its own — `simulate` triggers a
    // clear() whenever a record's own timestamp crosses the next
    // interval-width boundary in event-time (not wall-clock time).
    let mut next_fba_boundary: Option<u64> = None;
    let mut fba_batch_open_ts: Option<u64> = None;
    let mut last_seen_ts: Option<u64> = None;

    // `cda.bid_depth()`/`cda.ask_depth()` are O(1) running counters, read
    // fresh every record. `depth_schedule` (the bps-banded breakdown) is an
    // O(book size) scan whose membership can shift even for orders that
    // didn't change (the reference midpoint moved), so it's only recomputed
    // when a record could actually have changed the book (`is_actionable`).
    let mut cached_depth_sched = [(0u128, 0u128); timeseries::DEPTH_BPS_THRESHOLDS.len()];

    let total_bytes: u64 = todo_files.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let bytes_read = Arc::new(AtomicU64::new(0));
    let records_seen = Arc::new(AtomicU64::new(ckpt.records_seen as u64));

    let wall_clock_start = Instant::now();

    let stream_result: io::Result<()> = progress::run_with_progress(
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
            // Snapshot the resume offset once — `ckpt.files_done` is mutated
            // inside the loop below.
            let already_done = ckpt.files_done;
            for (i, path) in todo_files.iter().enumerate() {
                let file_index = already_done + i; // 0-based into `files`
                println!("{}", format!("-> [{}/{}] {}", file_index + 1, files.len(), path.display()).dimmed());
                io::stdout().flush().ok();

                let (seen, skipped, dropped) = simulator::stream_file(path, &bytes_read, &mut |order: Order| {
                    records_seen.fetch_add(1, Ordering::Relaxed);
                    last_seen_ts = Some(order.ts);
                    if anchor.is_none() {
                        anchor = Some(order.ts);
                        // Pin BOTH recorders to the same grid origin now, so
                        // FBA (which otherwise wouldn't anchor until its
                        // first batch clear) and CDA share bucket boundaries.
                        fba_recorder.set_anchor(order.ts);
                        cda_recorder.set_anchor(order.ts);
                    }

                    // Both recorders see the raw message stream before any gating —
                    // needed for order-to-trade ratio etc. One shared log, read by
                    // both recorders' `emit`.
                    messages.push(timeseries::OrderMessage {
                        ts: order.ts,
                        oid: order.oid,
                        user_id: order.user_id.clone(),
                        side: order.side(),
                        limit_price: order.limit_price(),
                        quantity: order.orig_sz,
                        accepted: order.is_new_live_order(),
                    });

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
                    // engines' `submit` — skip the clone + call for those.
                    let is_actionable = order.is_new_live_order() || order.is_cancellation();

                    let ts = order.ts;
                    let side = order.side();

                    if is_actionable {
                        fba.submit(order.clone());
                    }

                    let reference_price = midpoint(cda.best_bid(), cda.best_ask());
                    let cda_start = Instant::now();
                    let trades = if is_actionable { cda.submit(order) } else { Vec::new() };
                    let compute_time = cda_start.elapsed();

                    for trade in &trades {
                        cda_recorder.record_trade(timeseries::TradeEvent { trade: trade.clone(), reference_price, aggressor_side: Some(side) });
                    }

                    let best_bid = cda.best_bid();
                    let best_ask = cda.best_ask();
                    let best_bid_qty: u128 = cda.best_bid_order().map(|o| o.remaining).unwrap_or(0);
                    let best_ask_qty: u128 = cda.best_ask_order().map(|o| o.remaining).unwrap_or(0);

                    let cached_bid_depth = cda.bid_depth();
                    let cached_ask_depth = cda.ask_depth();

                    if is_actionable {
                        cached_depth_sched = match midpoint(best_bid, best_ask) {
                            Some(mid) => timeseries::depth_schedule(
                                mid,
                                cda.bids_iter()
                                    .map(|o| (o.limit_price().unwrap_or(mid), o.side(), o.remaining))
                                    .chain(cda.asks_iter().map(|o| (o.limit_price().unwrap_or(mid), o.side(), o.remaining))),
                            ),
                            None => [(0, 0); timeseries::DEPTH_BPS_THRESHOLDS.len()],
                        };
                    }
                    cda_recorder.record_book_snapshot(timeseries::BookSnapshot {
                        ts,
                        best_bid,
                        best_ask,
                        best_bid_qty,
                        best_ask_qty,
                        bid_depth: cached_bid_depth,
                        ask_depth: cached_ask_depth,
                        depth_schedule: cached_depth_sched,
                        compute_time,
                    });
                })?;

                ckpt.files_processed += 1;
                if dropped {
                    ckpt.files_skipped += 1;
                }
                ckpt.records_seen += seen;
                ckpt.records_skipped += skipped;
                ckpt.files_done = file_index + 1;
                ckpt.last_file = path.display().to_string();
                if let Some(w) = last_seen_ts {
                    max_seen_ts = max_seen_ts.max(w);
                }

                let is_last = ckpt.files_done == files.len();

                // On the final file, flush whatever's left in the still-open
                // FBA batch before the last emit.
                if is_last && !fba.pending_orders.is_empty() {
                    let open_ts = fba_batch_open_ts.unwrap_or(0);
                    let close_ts = last_seen_ts.unwrap_or(open_ts);
                    clear_fba_batch(&mut fba, &mut fba_recorder, open_ts, close_ts.max(open_ts));
                }

                let (fba_rows, cda_rows) = if is_last {
                    (fba_recorder.finish(&messages, max_seen_ts), cda_recorder.finish(&messages, max_seen_ts))
                } else {
                    // Flush every bucket that neither this file's tail nor the
                    // NEXT file's start is within `MARKOUT_GUARD_SECS` of — so
                    // the CSV grows once per input file. Peeking the next
                    // file's first timestamp is one small read.
                    let next_first_ts = files.get(file_index + 1).and_then(|p| simulator::peek_first_ts(p).ok().flatten());
                    let file_last_ts = last_seen_ts.unwrap_or(0);
                    let hi = flush_hi(anchor, next_first_ts, file_last_ts, markout_guard_ns, interval_width_ns);
                    (fba_recorder.emit(&messages, hi), cda_recorder.emit(&messages, hi))
                };
                debug_assert_eq!(fba_recorder.anchor(), cda_recorder.anchor(), "both recorders share the grid origin");
                debug_assert_eq!(fba_recorder.emitted_upto(), cda_recorder.emitted_upto(), "both recorders flush the same buckets");
                append_rows(&fba_csv, &fba_rows)?;
                append_rows(&cda_csv, &cda_rows)?;
                for r in &fba_rows {
                    fba_summary.fold(r);
                }
                for r in &cda_rows {
                    cda_summary.fold(r);
                }
                ckpt.fba_rows_written += fba_rows.len() as u64;
                ckpt.cda_rows_written += cda_rows.len() as u64;

                fba_recorder.prune();
                cda_recorder.prune();
                if let Some(a) = anchor {
                    let cutoff = fba_recorder.emitted_upto();
                    messages.retain(|m| timeseries::bucket_of(m.ts, a, interval_width_ns) >= cutoff);
                }

                ckpt.anchor = anchor;
                ckpt.emitted_upto = fba_recorder.emitted_upto();
                let fc = fba_recorder.carry();
                let cc = cda_recorder.carry();
                ckpt.fba_prev_close = fc.prev_close;
                ckpt.cda_prev_close = cc.prev_close;
                ckpt.fba_prev_clearing = fc.prev_clearing;
                ckpt.cda_prev_clearing = cc.prev_clearing; // always None
                ckpt.fba_late_dropped = fba_recorder.late_events_dropped();
                ckpt.cda_late_dropped = cda_recorder.late_events_dropped();
                ckpt.elapsed_secs = base_elapsed_secs + wall_clock_start.elapsed().as_secs_f64();
                ckpt.complete = is_last;
                ckpt.fba_summary = fba_summary.clone();
                ckpt.cda_summary = cda_summary.clone();
                ckpt.save_atomic(&out_dir)?;
            }
            Ok(())
        },
    );

    if let Err(err) = stream_result {
        println!("{}", format!("[ERROR] Streaming failed: {err}").red());
        println!("{}", format!("        Progress up to file {}/{} is checkpointed in {} — re-run the same command to resume.", ckpt.files_done, files.len(), out_dir.display()).yellow());
        return 1;
    }

    // Resume-after-a-crash-at-the-very-end: every file was already done, so
    // the loop body never ran. Mark complete and fall through to the summary.
    if todo_files.is_empty() && !ckpt.complete {
        ckpt.complete = true;
        ckpt.elapsed_secs = base_elapsed_secs + wall_clock_start.elapsed().as_secs_f64();
        let _ = ckpt.save_atomic(&out_dir);
    }

    println!(
        "{}",
        format!(
            "[OK] {}/{} file(s) done ({} record(s) seen, {} skipped, cumulative); {:.1}s wall-clock this run.",
            ckpt.files_done,
            files.len(),
            ckpt.records_seen,
            ckpt.records_skipped,
            wall_clock_start.elapsed().as_secs_f64()
        )
        .green()
    );
    if ckpt.files_skipped > 0 {
        println!(
            "{}",
            format!("[WARN] {} file(s) didn't look like order-status data and were skipped entirely.", ckpt.files_skipped).yellow()
        );
    }
    if ckpt.fba_late_dropped > 0 || ckpt.cda_late_dropped > 0 {
        println!(
            "{}",
            format!(
                "[WARN] {} FBA / {} CDA event(s) arrived for an interval that had already been flushed and were dropped (event-time went backwards past a file boundary). See summary.txt.",
                ckpt.fba_late_dropped, ckpt.cda_late_dropped
            )
            .yellow()
        );
    }
    println!("FBA: {} interval(s)  |  CDA: {} interval(s)", ckpt.fba_summary.intervals_total(), ckpt.cda_summary.intervals_total());

    match write_summary(&out_dir, &resolved_path, &ckpt) {
        Ok(()) => {
            println!("{}", format!("[OK] Time series + summary in {}", out_dir.display()).green());
            0
        }
        Err(err) => {
            println!("{}", format!("[ERROR] Failed to write summary: {err}").red());
            1
        }
    }
}

/// Exclusive upper bucket boundary to flush up to after a NON-last file.
/// Every bucket that starts before this is safe to emit: it is more than
/// `guard_ns` behind BOTH this file's own tail (`file_last_ts` — so its
/// markout forward-mids exist) and the next file's first record
/// (`next_first_ts` — so no later file, in `collect_input_files`'s sorted
/// order, can still add records to it). Returns `anchor` (a no-op for
/// `emit`) when nothing is safe yet; `0` before the grid has an origin.
/// The last file goes through `MetricsRecorder::finish` instead.
fn flush_hi(anchor: Option<u64>, next_first_ts: Option<u64>, file_last_ts: u64, guard_ns: u64, width: u64) -> u64 {
    let Some(a) = anchor else {
        return 0;
    };
    let cap = next_first_ts.map_or(file_last_ts, |nf| nf.min(file_last_ts));
    let safe = cap.saturating_sub(guard_ns);
    if safe > a {
        a + ((safe - a) / width) * width
    } else {
        a
    }
}

/// Append already-computed interval rows to a time-series CSV that already
/// carries its header line. Flushes to the OS so an interrupted run's
/// partial output is on disk.
fn append_rows(path: &Path, rows: &[IntervalMetrics]) -> io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(timeseries::csv_rows(rows).as_bytes())?;
    f.flush()
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

    // Net signed order flow into this batch, BEFORE price selection — the
    // regressor for FBA `kyle_lambda` (see `BatchClearedEvent::net_order_flow`).
    let net_order_flow: f64 = snapshot
        .iter()
        .map(|&(_, side, qty)| match side {
            Side::Buy => qty as f64,
            Side::Sell => -(qty as f64),
        })
        .sum();

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
            net_order_flow,
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
        net_order_flow,
        traded_quantity: clearing.traded_quantity,
        // The heavier ELIGIBLE side's leftover (`|demand - supply|`), not a
        // full-residual-orders sum — see `FbaOrderBook::unexecuted_residual_share`'s
        // doc comment for why the latter can push a share past 1.
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

fn write_summary(out_dir: &Path, source: &str, ckpt: &Checkpoint) -> io::Result<()> {
    let ts = run_timestamp();
    let fba = &ckpt.fba_summary;
    let cda = &ckpt.cda_summary;

    let summary = format!(
        "Simulation summary\n\
         ===================\n\
         Generated:               {ts} (UTC)\n\
         Source:                  {source}\n\
         Files processed:         {}\n\
         Files skipped (not order-status data): {}\n\
         Records seen:            {}\n\
         Records skipped:         {}\n\
         Wall-clock duration:     {:.1}s (cumulative across runs)\n\
         Markout guard:           {MARKOUT_GUARD_SECS}s (forward-mid lag before a bucket is flushed; flush cadence is per input file)\n\
         Late events dropped:     {} (FBA) / {} (CDA)\n\
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
         `kyle_lambda` is a slope in bps-of-mid-move per SOL of signed order\n\
         flow — NOT a price, so it is NOT PRICE_SCALE-denominated.\n\
         \n\
         \"avg\" below means the mean across only the intervals where that\n\
         metric was actually computable (see IntervalMetrics' doc comment —\n\
         `None` always means \"not computable from what was recorded,\" never\n\
         a silent zero, so intervals with no value for a given metric are\n\
         excluded from its average rather than pulling it toward zero).\n",
        ckpt.files_processed,
        ckpt.files_skipped,
        ckpt.records_seen,
        ckpt.records_skipped,
        ckpt.elapsed_secs,
        ckpt.fba_late_dropped,
        ckpt.cda_late_dropped,
        fba.intervals_total(),
        cda.intervals_total(),
        fba.trade_count_sum(),
        fba.executed_volume_sum(),
        fba.executed_notional_sum() / PRICE_SCALE as f64,
        cda.trade_count_sum(),
        cda.executed_volume_sum(),
        cda.executed_notional_sum() / PRICE_SCALE as f64,
        fba.render_section("FBA"),
        cda.render_section("CDA"),
    );
    fs::write(out_dir.join(replay_checkpoint::SUMMARY_FILE), summary)
}

/// Formats the current UTC time as `YYYYMMDD_HHMMSS`, for the summary's
/// "Generated" line. Pure integer arithmetic — no date/time crate needed:
/// `civil_from_days` is the inverse of `inputs::simulator::days_from_civil`
/// (both Howard Hinnant's public-domain algorithm).
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
    /// (`parses_dataset_timestamps` there), run backwards.
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
    fn flush_hi_releases_up_to_the_next_files_start_minus_the_markout_guard() {
        let w = 1_000_000_000u64;
        let guard = 10 * w;
        let anchor = Some(1_000u64);

        // No anchor -> nothing to emit.
        assert_eq!(flush_hi(None, Some(999_999), 999_999, guard, w), 0);

        // Next file starts only just past the anchor -> frontier below the
        // anchor -> returns the anchor (a no-op for `emit`).
        assert_eq!(flush_hi(anchor, Some(1_000 + 5 * w), 1_000 + 5 * w, guard, w), 1_000);

        // Next file is the binding cap: it starts at anchor+25s, this file's
        // tail is further out -> flush up to (25s - 10s) past anchor, grid-aligned.
        let hi = flush_hi(anchor, Some(1_000 + 25 * w), 1_000 + 40 * w, guard, w);
        assert_eq!(hi, 1_000 + 15 * w);
        assert_eq!((hi - 1_000) % w, 0, "grid-aligned");

        // This file's own tail is the binding cap (next file starts even later).
        let hi = flush_hi(anchor, Some(1_000 + 40 * w), 1_000 + 25 * w, guard, w);
        assert_eq!(hi, 1_000 + 15 * w);

        // No next file peeked -> fall back to this file's tail alone.
        assert_eq!(flush_hi(anchor, None, 1_000 + 25 * w, guard, w), 1_000 + 15 * w);
    }
}

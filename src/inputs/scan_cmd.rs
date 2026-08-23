//! The `scan` command: counts records/orders across real order-status data
//! (a file, folder, or `btc`/`eth`/`sol`/`all` shorthand — same targets
//! `simulate` accepts) without running either matching engine. It streams
//! through exactly the same `inputs::simulator::stream_records` path
//! `simulate` uses, just with a cheap tallying closure instead of feeding
//! `FbaOrderBook`/`CdaOrderBook` — no book-depth computation, no metrics
//! recording, no `output/` files. Meant for "how much data is even here,
//! and what does it look like" before committing to a full (much slower)
//! replay.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use colored::Colorize;

use crate::inputs::progress;
use crate::inputs::simulator;
use crate::types::Order;

/// Which of the three mutually-exclusive buckets a record's `status_id`
/// falls into — mirrors exactly what `FbaOrderBook::submit`/
/// `CdaOrderBook::submit` themselves branch on (`Order::is_new_live_order`/
/// `Order::is_cancellation`), so these counts describe what a `simulate`
/// run over the same data would actually do with each record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    /// Becomes a new resting order on the book (`open` non-trigger, or a
    /// conditional order's `triggered` event).
    NewLiveOrder,
    /// Removes a still-resting order (a `canceled`-type status).
    Cancellation,
    /// Everything else — rejections, `filled` events, a conditional
    /// order's own (not-yet-triggered) `open` record. Neither engine acts
    /// on these.
    Other,
}

fn classify(order: &Order) -> Category {
    if order.is_new_live_order() {
        Category::NewLiveOrder
    } else if order.is_cancellation() {
        Category::Cancellation
    } else {
        Category::Other
    }
}

pub fn run(path_str: &str) {
    // `scan all` mirrors `simulate all`/`download all`/`extract all` — each
    // coin gets its own independent scan and report rather than one
    // combined total across unrelated datasets.
    if path_str.eq_ignore_ascii_case("all") {
        for coin in ["btc", "eth", "sol"] {
            run(coin);
        }
        return;
    }

    // Same coin-shorthand resolution as `simulate_cmd::run` — kept as its
    // own copy rather than a shared helper since it's four lines and each
    // caller's error messages differ slightly (mentioning `scan`/`download`
    // by name).
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

    println!("{}", format!("==> Scanning {} file(s) from '{resolved_path}' ...", files.len()).cyan());

    let total_bytes: u64 = files.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let bytes_read = Arc::new(AtomicU64::new(0));
    let new_live_orders = AtomicU64::new(0);
    let cancellations = AtomicU64::new(0);
    let other_events = AtomicU64::new(0);
    let wall_clock_start = Instant::now();

    let stream_result = progress::run_with_progress(
        (total_bytes > 0).then_some(total_bytes),
        {
            let bytes_read = Arc::clone(&bytes_read);
            move || bytes_read.load(Ordering::Relaxed)
        },
        progress::human_bytes,
        || String::new(),
        || {
            simulator::stream_records(&files, &bytes_read, |order: Order| match classify(&order) {
                Category::NewLiveOrder => {
                    new_live_orders.fetch_add(1, Ordering::Relaxed);
                }
                Category::Cancellation => {
                    cancellations.fetch_add(1, Ordering::Relaxed);
                }
                Category::Other => {
                    other_events.fetch_add(1, Ordering::Relaxed);
                }
            })
        },
    );

    let stats = match stream_result {
        Ok(stats) => stats,
        Err(err) => {
            println!("{}", format!("[ERROR] Scan failed: {err}").red());
            return;
        }
    };

    let elapsed = wall_clock_start.elapsed();
    println!("{}", format!("[OK] Scanned {} file(s) in {:.1}s.", stats.files_processed, elapsed.as_secs_f64()).green());
    println!("  Total records:       {}", stats.records_seen);
    println!("  New live orders:     {}  (would rest on the book — is_new_live_order)", new_live_orders.load(Ordering::Relaxed));
    println!("  Cancellations:       {}  (would remove a resting order — is_cancellation)", cancellations.load(Ordering::Relaxed));
    println!("  Other events:        {}  (rejections, fills, non-triggered conditional opens, etc. — no-ops to both engines)", other_events.load(Ordering::Relaxed));
    println!("  Skipped (unparseable/zero-size): {}", stats.records_skipped);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    fn order_with_status(status_id: u8, is_trigger: bool, triggered: bool) -> Order {
        let mut o = Order::limit(1, "u", Side::Buy, 100, 1, 0);
        o.status_id = status_id;
        o.is_trigger = is_trigger;
        o.triggered = triggered;
        o
    }

    #[test]
    fn classify_matches_what_the_engines_themselves_would_do() {
        // open, non-trigger -> new live order (matches FbaOrderBook::submit/
        // CdaOrderBook::submit's own is_new_live_order gating).
        assert_eq!(classify(&order_with_status(1, false, false)), Category::NewLiveOrder);
        // open, but a not-yet-triggered conditional order -> neither engine
        // treats this as live.
        assert_eq!(classify(&order_with_status(1, true, false)), Category::Other);
        // triggered (status 9) -> live, regardless of is_trigger.
        assert_eq!(classify(&order_with_status(9, true, false)), Category::NewLiveOrder);
        // canceled -> cancellation.
        assert_eq!(classify(&order_with_status(2, false, false)), Category::Cancellation);
        // filled -> neither live nor a cancellation (see Order::is_cancellation's
        // doc for why `filled` is deliberately excluded).
        assert_eq!(classify(&order_with_status(5, false, false)), Category::Other);
        // an outright rejection -> other.
        assert_eq!(classify(&order_with_status(0, false, false)), Category::Other);
    }

    /// Cross-checked against the real numbers from an earlier `simulate`
    /// run over this exact same sample directory in this project's history
    /// (`data/sample/order_statuses`: `sol_12.data.gz` + its `_rejected`
    /// counterpart) — "Streamed 2 file(s), 3630216 record(s) seen (54179
    /// skipped)". `scan` walks the identical `collect_input_files`/
    /// `stream_records` path, just with a tallying closure instead of
    /// feeding the engines, so it must reproduce those same totals exactly.
    /// `#[ignore]`d by default since it depends on `data/sample/` being
    /// present at a fixed relative path — run explicitly with
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn scan_reproduces_known_totals_for_the_real_sample_data() {
        let root = Path::new("../data/sample/order_statuses");
        let files = simulator::collect_input_files(root).expect("should find the sample files");
        assert_eq!(files.len(), 2, "expected sol_12.data.gz + sol_12_rejected.data.gz");

        let bytes_read = Arc::new(AtomicU64::new(0));
        let new_live_orders = AtomicU64::new(0);
        let cancellations = AtomicU64::new(0);
        let other_events = AtomicU64::new(0);

        let stats = simulator::stream_records(&files, &bytes_read, |order: Order| match classify(&order) {
            Category::NewLiveOrder => {
                new_live_orders.fetch_add(1, Ordering::Relaxed);
            }
            Category::Cancellation => {
                cancellations.fetch_add(1, Ordering::Relaxed);
            }
            Category::Other => {
                other_events.fetch_add(1, Ordering::Relaxed);
            }
        })
        .expect("scanning the real sample files should succeed");

        assert_eq!(stats.files_processed, 2);
        assert_eq!(stats.records_seen, 3_630_216);
        assert_eq!(stats.records_skipped, 54_179);

        // The three buckets are mutually exclusive and cover every
        // successfully-parsed record (every record except the skipped
        // zero-size ones).
        let classified_total = new_live_orders.load(Ordering::Relaxed) + cancellations.load(Ordering::Relaxed) + other_events.load(Ordering::Relaxed);
        assert_eq!(classified_total as usize, stats.records_seen - stats.records_skipped);
    }
}

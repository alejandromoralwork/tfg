//! Shared live progress-bar machinery for this crate's long-running
//! commands — `download`/`extract`'s decompress-then-extract steps, and
//! `simulate`'s multi-GB replay. A single in-place-redrawn (`\r`, no
//! scrolling) line, so a multi-minute operation never looks hung, with a
//! real percentage whenever a total is known.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// `n` formatted as the largest whole binary unit that keeps it >= 1 (e.g.
/// `6.16 GiB`, `842.00 KiB`), 2 decimal places.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

/// Redraws a progress line in place. With a known `total`, shows a
/// filled/empty bar and a percentage; without one (e.g. the total couldn't
/// be determined up front), falls back to just the running count and
/// elapsed time so it's still obvious something is happening. `fmt` formats
/// a raw count for display (`human_bytes` for byte counts, `|n| n.to_string()`
/// for plain ones); `extra` is appended as-is for any additional context
/// (e.g. "  file 3/48") — pass `""` for none.
pub fn print_bar(current: u64, total: Option<u64>, elapsed: Duration, fmt: impl Fn(u64) -> String, extra: &str) {
    const WIDTH: usize = 30;
    let line = match total.filter(|&t| t > 0) {
        Some(total) => {
            let ratio = (current as f64 / total as f64).min(1.0);
            let filled = (ratio * WIDTH as f64).round() as usize;
            format!("[{}{}] {:>3}%  ({} / {}){extra}", "#".repeat(filled), "-".repeat(WIDTH - filled), (ratio * 100.0) as u32, fmt(current), fmt(total))
        }
        None => format!("   ... {} so far (elapsed {}s){extra}", fmt(current), elapsed.as_secs()),
    };
    // \r (no newline) + trailing spaces to blank out any leftover tail from
    // a longer previous line, then flush — this line is redrawn in place,
    // not scrolled.
    print!("\r{line}                    ");
    std::io::stdout().flush().ok();
}

/// Runs `work` on the calling thread while a background thread polls
/// `measure()` — "units done so far" — about ten times a second, redrawing
/// the progress line (via `print_bar`, `fmt`-formatted) at most once a
/// second. Returns `work`'s own result. Uses a scoped thread so `measure`/
/// `extra` (closures that typically borrow local state — a `Child`, a
/// shared counter, a destination path) don't need to be `'static`; the
/// scope itself blocks until the poll loop notices `done` and exits, which
/// is why the sleep is chopped into short steps rather than one long one.
pub fn run_with_progress<T>(total: Option<u64>, measure: impl Fn() -> u64 + Sync, fmt: impl Fn(u64) -> String + Sync, extra: impl Fn() -> String + Sync, work: impl FnOnce() -> T) -> T {
    let done = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        scope.spawn(|| {
            let start = Instant::now();
            while !done.load(Ordering::Relaxed) {
                print_bar(measure(), total, start.elapsed(), &fmt, &extra());
                for _ in 0..10 {
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let result = work();
        done.store(true, Ordering::Relaxed);
        result
    });
    // One last redraw at the real final measurement (the poll loop's last
    // frame can be a beat behind `work` actually finishing) so the bar
    // visibly reaches its end state rather than stopping short.
    print_bar(measure(), total, Duration::ZERO, &fmt, &extra());
    println!(); // leave the redrawn line in place instead of overwriting it next
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_picks_the_largest_unit_that_stays_above_one() {
        assert_eq!(human_bytes(0), "0.00 B");
        assert_eq!(human_bytes(512), "512.00 B");
        assert_eq!(human_bytes(1536), "1.50 KiB"); // 1.5 * 1024
        assert_eq!(human_bytes(6_615_982_080), "6.16 GiB"); // the real sol archive's uncompressed size
    }
}

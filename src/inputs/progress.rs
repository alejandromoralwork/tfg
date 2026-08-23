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
/// filled/empty bar, a percentage, and (once there's enough signal to
/// estimate one — see `eta_suffix`) an ETA; without a `total` (e.g. it
/// couldn't be determined up front), falls back to just the running count
/// and elapsed time so it's still obvious something is happening — no ETA
/// is possible without knowing the target. `fmt` formats a raw count for
/// display (`human_bytes` for byte counts, `|n| n.to_string()` for plain
/// ones); `extra` is appended as-is for any additional context (e.g.
/// "  file 3/48") — pass `""` for none.
pub fn print_bar(current: u64, total: Option<u64>, elapsed: Duration, fmt: impl Fn(u64) -> String, extra: &str) {
    const WIDTH: usize = 30;
    let line = match total.filter(|&t| t > 0) {
        Some(total) => {
            let ratio = (current as f64 / total as f64).min(1.0);
            let filled = (ratio * WIDTH as f64).round() as usize;
            let eta = eta_suffix(current, total, elapsed);
            format!("[{}{}] {:>3}%  ({} / {}){eta}{extra}", "#".repeat(filled), "-".repeat(WIDTH - filled), (ratio * 100.0) as u32, fmt(current), fmt(total))
        }
        None => format!("   ... {} so far (elapsed {}s){extra}", fmt(current), elapsed.as_secs()),
    };
    // \r (no newline) + trailing spaces to blank out any leftover tail from
    // a longer previous line, then flush — this line is redrawn in place,
    // not scrolled.
    print!("\r{line}                    ");
    std::io::stdout().flush().ok();
}

/// `"  ETA ~<duration>"` once there's been enough elapsed time to trust an
/// average-rate estimate, else `""` (nothing done yet, already done, or
/// still too early — a rate computed from a fraction of a second of data
/// would be more misleading than useful, especially right at startup when
/// the first file/chunk hasn't even finished yet).
fn eta_suffix(current: u64, total: u64, elapsed: Duration) -> String {
    let elapsed_secs = elapsed.as_secs_f64();
    if current == 0 || current >= total || elapsed_secs < 2.0 {
        return String::new();
    }
    let rate = current as f64 / elapsed_secs; // units/sec, averaged since start
    let remaining_secs = (total - current) as f64 / rate;
    format!("  ETA ~{}", format_duration(remaining_secs as u64))
}

/// `secs` formatted as the coarsest one or two units that convey it
/// (`"2d 4h"`, `"1h 23m"`, `"5m 09s"`, `"45s"`) — a multi-day estimate
/// doesn't need second-level precision, so lower units drop off once a
/// larger one is present.
pub fn format_duration(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
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

    #[test]
    fn format_duration_drops_lower_units_once_a_larger_one_is_present() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(69), "1m 09s");
        assert_eq!(format_duration(3661), "1h 01m");
        assert_eq!(format_duration(90_000), "1d 1h"); // 25h -> 1d 1h
    }

    #[test]
    fn eta_suffix_withholds_the_estimate_until_theres_enough_signal() {
        assert_eq!(eta_suffix(0, 100, Duration::from_secs(10)), "", "nothing done yet -> no rate to extrapolate from");
        assert_eq!(eta_suffix(100, 100, Duration::from_secs(10)), "", "already done -> no ETA needed");
        assert_eq!(eta_suffix(1, 100, Duration::from_millis(500)), "", "too little elapsed time to trust a rate yet");
        // 50/100 in 10s -> 5 units/s -> 50 remaining -> 10s left.
        assert_eq!(eta_suffix(50, 100, Duration::from_secs(10)), "  ETA ~10s");
    }
}

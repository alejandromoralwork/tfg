//! Reads Hyperliquid order-status data into `crate::types::Order` values
//! that can be fed straight into either engine's orderbook
//! (`FbaOrderBook::submit` / `CdaOrderBook::submit`). Two intake paths:
//!
//! - The small pre-decoded CSV PREVIEW files (`load_order_status_csv`,
//!   used by the `load` command) — materializes everything into a
//!   `Vec<Order>`, fine at PREVIEW scale (hundreds of rows).
//! - The real binary/gzip archive under `data/order_statuses/`
//!   (`collect_input_files` + `stream_records`, used by the `simulate`
//!   command) — streams record-by-record straight into a caller-supplied
//!   closure, never materializing a file's (let alone a whole run's)
//!   worth of orders at once. This path is what makes replaying a
//!   multi-day range (tens of millions of records) feasible at all.
//!
//! Deliberately builds `Order` directly (not through `Order::limit`, which
//! hardcodes `status_id = 1` / `is_trigger = false`) so a row's real
//! lifecycle status survives — a `canceled`/`filled`/rejected row still
//! produces an `Order`, it just won't pass `Order::is_new_live_order()`,
//! exactly like a live replay would gate it.
//!
//! `flate2` (this project's only dependency) handles gzip; everything else
//! — CSV splitting, price/timestamp decoding — is still pure integer
//! arithmetic and hand-rolled parsing, no floats, matching this project's
//! pricing discipline.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use colored::Colorize;
use flate2::read::MultiGzDecoder;

use crate::inputs::binary_format::{self, RECORD_SIZE};
use crate::types::Order;

/// Column count of the known PREVIEW header:
/// ts,userId,isBuilder,statusId,isAsk,limitPx,sz,oid,timestampDiff,
/// triggerCondition,triggered,isTrigger,hasChildren,isPositionTpsl,
/// reduceOnly,orderTypeId,tifId,triggerPx,origSz,status,orderType,tif
const MIN_COLUMNS: usize = 22;

pub fn load_order_status_csv(path: &str) -> io::Result<Vec<Order>> {
    let contents = fs::read_to_string(path)?;
    let mut orders = Vec::new();
    let mut lines = contents.lines();

    let Some(header) = lines.next() else {
        return Ok(orders); // empty file
    };
    // Same structural pre-check `simulator::stream_csv` uses — drop the
    // whole file with one clear warning rather than failing every single
    // row (and printing a warning for each) if it isn't actually
    // order-status data to begin with.
    if !looks_like_order_status_header(header) {
        eprintln!(
            "{}",
            format!("[WARN] Skipping file: header doesn't look like order-status data ({} column(s), expected at least {MIN_COLUMNS}): {header} ({path})", header.split(',').count()).yellow()
        );
        return Ok(orders);
    }

    for (line_no, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_row(line) {
            Ok(Some(order)) => orders.push(order),
            // Valid row, but its size rounds to nothing worth trading —
            // not an error, just not worth constructing an order for.
            Ok(None) => {}
            // +2: `lines` here starts counting from the first row AFTER
            // the header already consumed above (0-based), and this
            // message uses 1-based file line numbers.
            Err(()) => eprintln!("{}", format!("[WARN] Skipping malformed row {} in {path}", line_no + 2).yellow()),
        }
    }

    Ok(orders)
}

/// Cheap structural check applied to a CSV file's header line before any
/// row gets parsed — lets a wrong-schema file (e.g. one of the lookup
/// tables under `data/*/mapdir/`, which can run into hundreds of
/// thousands of rows — `users.csv` alone had 328,456) get dropped as ONE
/// clear warning instead of one `[WARN] Skipping malformed ... row N` per
/// row. Not a full schema check, just the same column-count bar every row
/// already has to clear (`MIN_COLUMNS`) — cheap, and already enough to
/// reject every known non-order-status CSV in this dataset (the lookup
/// tables all have 2-3 columns).
fn looks_like_order_status_header(header: &str) -> bool {
    header.trim().split(',').count() >= MIN_COLUMNS
}

/// `Err(())` = the row genuinely couldn't be parsed (bad/missing fields).
/// `Ok(None)` = the row parsed fine but isn't worth an `Order` for (its
/// size rounds to zero — see `round_to_unit`). `Ok(Some(order))` = success.
fn parse_row(line: &str) -> Result<Option<Order>, ()> {
    let cols: Vec<&str> = line.split(',').collect();
    if cols.len() < MIN_COLUMNS {
        return Err(());
    }

    let ts = parse_dataset_ts(cols[0]).ok_or(())?;
    let user_id = cols[1].trim().to_string();
    let status_id: u8 = cols[3].trim().parse().map_err(|_| ())?;
    let is_ask = parse_bool(cols[4]).ok_or(())?;
    let limit_px = parse_fixed_point(cols[5]).ok_or(())?;
    let oid: u64 = cols[7].trim().parse().map_err(|_| ())?;
    let is_trigger = parse_bool(cols[11]).ok_or(())?;
    let order_type_id: u8 = cols[15].trim().parse().map_err(|_| ())?;
    let tif_id: u8 = cols[16].trim().parse().map_err(|_| ())?;
    let orig_sz = round_to_unit(cols[18]).ok_or(())?;
    let status = non_empty(cols[19]);
    let order_type = non_empty(cols[20]);
    let tif = non_empty(cols[21]);

    // The engine's Amount/quantity type has no fixed-point convention (see
    // README notes), so fractional sizes are rounded to the nearest whole
    // unit; a row that rounds to nothing isn't worth constructing an order
    // for — but it parsed fine, so this isn't "malformed".
    if orig_sz == 0 {
        return Ok(None);
    }

    Ok(Some(Order {
        ts,
        user_id,
        is_builder: false,
        status_id,
        is_ask,
        limit_px,
        sz: orig_sz,
        oid,
        timestamp_diff: 0,
        trigger_condition: 0,
        triggered: false,
        is_trigger,
        has_children: false,
        is_position_tpsl: false,
        reduce_only: false,
        order_type_id,
        tif_id,
        trigger_px: 0,
        orig_sz,
        closed_ts: 0,
        status,
        order_type,
        tif,
        remaining: orig_sz,
    }))
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "True" | "true" => Some(true),
        "False" | "false" => Some(false),
        _ => None,
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

/// Decimal string (e.g. `"126.67"`) -> `PRICE_SCALE`-fixed-point `u128`
/// (e.g. `126_670_000`), matching `crate::types::PRICE_SCALE` (1e6). Pure
/// integer arithmetic: split on `.`, parse the integer part, and
/// pad/truncate the fractional part to exactly 6 digits. A plain integer
/// string (no `.`) works too — `"127"` -> `127_000_000`, same as before
/// this had decimal support at all.
///
/// `pub(crate)` rather than a CSV-loader implementation detail: `inputs::cli`'s
/// `add` command reuses this too, so a typed `add buy 127.06 5 Alice` and
/// a `load`-sourced row with `limitPx=127.06` parse identically instead of
/// the CLI only understanding whole-number prices.
pub(crate) fn parse_fixed_point(s: &str) -> Option<u128> {
    const DECIMALS: usize = 6;
    let s = s.trim();
    let (int_part, frac_part) = s.split_once('.').unwrap_or((s, ""));

    let int_val: u128 = int_part.parse().ok()?;
    if !frac_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let mut frac_val: u128 = 0;
    for i in 0..DECIMALS {
        let digit = frac_part.as_bytes().get(i).map(|b| (*b - b'0') as u128).unwrap_or(0);
        frac_val = frac_val * 10 + digit;
    }

    Some(int_val * 1_000_000 + frac_val)
}

/// Decimal string (e.g. `"39.35"`) -> nearest whole unit (`39`) via
/// round-half-up on the first fractional digit — exact for that decision
/// since a fractional part `>= 0.5` always has a first digit `>= 5` and
/// vice versa. No fixed-point quantity support in the engine yet, so this
/// is a lossy but honest simplification.
///
/// `pub(crate)`, same reasoning as `parse_fixed_point` above: `inputs::cli`'s
/// `add` command uses this for quantity too, so a typed decimal qty rounds
/// the exact same way a loaded CSV row's `origSz` does.
pub(crate) fn round_to_unit(s: &str) -> Option<u128> {
    let s = s.trim();
    let (int_part, frac_part) = s.split_once('.').unwrap_or((s, ""));
    let int_val: u128 = int_part.parse().ok()?;

    match frac_part.as_bytes().first() {
        None => Some(int_val),
        Some(b) if b.is_ascii_digit() && *b >= b'5' => Some(int_val + 1),
        Some(b) if b.is_ascii_digit() => Some(int_val),
        _ => None,
    }
}

/// Parses `"YYYY-MM-DD HH:MM:SS.fffffffff"` (the PREVIEW CSV's timestamp
/// format, 9-digit nanosecond fraction) into nanoseconds since the Unix
/// epoch. Pure integer arithmetic — no external date/time crate.
fn parse_dataset_ts(s: &str) -> Option<u64> {
    let s = s.trim();
    let (date_part, time_part) = s.split_once(' ')?;

    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: u32 = date_iter.next()?.parse().ok()?;
    let day: u32 = date_iter.next()?.parse().ok()?;

    let (hms_part, nanos_str) = time_part.split_once('.').unwrap_or((time_part, "0"));
    let mut hms_iter = hms_part.split(':');
    let hour: i64 = hms_iter.next()?.parse().ok()?;
    let minute: i64 = hms_iter.next()?.parse().ok()?;
    let second: i64 = hms_iter.next()?.parse().ok()?;

    if !nanos_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut nanos: i64 = 0;
    for i in 0..9 {
        let digit = nanos_str.as_bytes().get(i).map(|b| (*b - b'0') as i64).unwrap_or(0);
        nanos = nanos * 10 + digit;
    }

    let days = days_from_civil(year, month, day);
    let total_ns = days * 86_400_000_000_000i64
        + hour * 3_600_000_000_000
        + minute * 60_000_000_000
        + second * 1_000_000_000
        + nanos;

    if total_ns < 0 { None } else { Some(total_ns as u64) }
}

/// Howard Hinnant's `days_from_civil`: days since the Unix epoch
/// (1970-01-01) for a proleptic-Gregorian (year, month, day), pure integer
/// arithmetic. Public-domain algorithm —
/// <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month as i64 + 9) % 12; // [0, 11], Mar=0 .. Feb=11
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

// ============================================================================
// Streaming intake for the real binary/gzip archive (`simulate` command)
// ============================================================================

/// Total records seen/skipped/files processed across a `stream_records`
/// run — used for the `simulate` command's progress output and its
/// `output/summary.txt`.
#[derive(Default, Debug, Clone, Copy)]
pub struct RunStats {
    pub files_processed: usize,
    /// Of `files_processed`, how many were dropped ENTIRELY by the
    /// structural pre-check (`looks_like_order_status_header`/
    /// `binary_format::looks_like_order_status_record`) before any of
    /// their records were read — a wrong-schema file that got swept in by
    /// `collect_input_files` (e.g. a lookup table under `mapdir/`, or a
    /// foreign `.gz`), not counted toward `records_seen`/`records_skipped`
    /// at all since none of its rows/records were even looked at.
    pub files_skipped: usize,
    pub records_seen: usize,
    pub records_skipped: usize,
}

/// `path` may be a single file (`.csv` or `.gz`) or a directory — in which
/// case every `.csv`/`.gz` file anywhere underneath it (recursing through
/// date subfolders) is collected, sorted lexicographically by full path.
/// That sort already puts dates and zero-padded hours in chronological
/// order, and — because `.` (0x2E) sorts before `_` (0x5F) — puts
/// `sol_00.data.gz` before `sol_00_rejected.data.gz`: accepted before
/// rejected within the same hour.
pub fn collect_input_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_input_files_into(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_input_files_into(path: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_input_files_into(&entry?.path(), out)?;
        }
    } else if is_supported_extension(path) {
        out.push(path.to_path_buf());
    }
    Ok(())
}

fn is_supported_extension(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("csv") | Some("gz"))
}

/// A `Read` wrapper that adds every byte actually read to a shared atomic
/// counter — sits *below* the gzip decoder (wraps the raw file, not its
/// decompressed output), so it tracks physical bytes consumed from disk
/// (compressed size for `.gz`) rather than decoded record volume. That's
/// what makes it a meaningful progress signal: it can be compared directly
/// against each file's on-disk size (`fs::metadata`, no decompression
/// needed), which `simulate_cmd` sums upfront for its live progress bar.
struct CountingReader<R> {
    inner: R,
    count: Arc<AtomicU64>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Opens `path` as a buffered byte stream, transparently gzip-decompressing
/// through `flate2::read::MultiGzDecoder` if the extension is `.gz`.
/// Streams throughout — the decompressed content is never held in memory
/// as one big buffer, only whatever the caller reads at a time. Every byte
/// read from `path` itself (pre-decompression) is added to `bytes_read`.
fn open_reader(path: &Path, bytes_read: &Arc<AtomicU64>) -> io::Result<Box<dyn Read>> {
    let file = CountingReader { inner: BufReader::new(File::open(path)?), count: Arc::clone(bytes_read) };
    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        Ok(Box::new(MultiGzDecoder::new(file)))
    } else {
        Ok(Box::new(file))
    }
}

/// Streams every file in `files`, calling `on_record` immediately for each
/// decoded `Order` — never collects them into a `Vec`. Prints a short
/// progress line per file (there are at most 48 per day, so this doesn't
/// spam a multi-day run) so a long replay doesn't look hung. `bytes_read`
/// accumulates raw bytes consumed across every file — `simulate_cmd` polls
/// it from another thread to drive a live progress bar against the total
/// on-disk size of `files`, computed upfront.
pub fn stream_records(files: &[PathBuf], bytes_read: &Arc<AtomicU64>, mut on_record: impl FnMut(Order)) -> io::Result<RunStats> {
    let mut stats = RunStats::default();

    for (i, path) in files.iter().enumerate() {
        println!("{}", format!("-> [{}/{}] {}", i + 1, files.len(), path.display()).dimmed());
        // Explicit flush: stdout is fully (not line-)buffered when it's
        // not a terminal (piped/redirected), which is exactly how a long
        // `simulate` run's output is normally consumed — without this,
        // progress just silently accumulates in a buffer and never
        // appears until the process exits, making a multi-minute run look
        // hung even though it's working.
        io::stdout().flush().ok();
        let (seen, skipped, dropped) = stream_file(path, bytes_read, &mut on_record)?;
        stats.files_processed += 1;
        if dropped {
            stats.files_skipped += 1;
        }
        stats.records_seen += seen;
        stats.records_skipped += skipped;
    }

    Ok(stats)
}

/// Same job as `stream_records`, but spreads `files` across worker threads
/// instead of streaming them one at a time — for a caller like `scan` that
/// only tallies independent per-record counts and doesn't care what order
/// records arrive in relative to EACH OTHER (unlike `simulate`, which feeds
/// a single stateful `FbaOrderBook`/`CdaOrderBook` where price-time
/// priority and batch-window ordering depend on strict sequential replay —
/// that path must keep using `stream_records`, never this one).
///
/// `on_record` is called concurrently from multiple threads (hence `Fn` +
/// `Sync`, not `FnMut`) — it must be safe to invoke from more than one
/// thread at once, e.g. tallying into `Atomic*` counters the way `scan_cmd`
/// already does (those were `Sync`-safe for exactly this reason before this
/// function existed). `bytes_read` is unaffected by the parallelism: it's
/// already an `Arc<AtomicU64>`, so concurrent increments from multiple
/// threads are exactly as safe as the single-threaded case.
///
/// Each worker takes every `n`th file (round-robin, `n` = thread count) —
/// a simple split that balances reasonably well even when files vary
/// somewhat in size, without needing a real work-stealing scheduler for
/// what's typically at most a few dozen files per coin per day.
pub fn stream_records_parallel(files: &[PathBuf], bytes_read: &Arc<AtomicU64>, on_record: impl Fn(Order) + Sync) -> io::Result<RunStats> {
    if files.is_empty() {
        return Ok(RunStats::default());
    }

    let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(files.len());
    // Guards the per-file "-> path" announcement only — real progress comes
    // from the shared `bytes_read` counter (already safe under concurrent
    // increments), this is just to stop several threads' println!s from
    // interleaving mid-line into garbled output.
    let print_lock = std::sync::Mutex::new(());

    let per_thread_results: Vec<io::Result<RunStats>> = std::thread::scope(|scope| {
        let on_record = &on_record;
        let print_lock = &print_lock;
        let handles: Vec<_> = (0..n_threads)
            .map(|worker| {
                scope.spawn(move || {
                    let mut stats = RunStats::default();
                    let mut idx = worker;
                    while idx < files.len() {
                        let path = &files[idx];
                        {
                            let _guard = print_lock.lock().unwrap();
                            println!("{}", format!("-> {}", path.display()).dimmed());
                            io::stdout().flush().ok();
                        }
                        let (seen, skipped, dropped) = stream_file(path, bytes_read, &mut |order| on_record(order))?;
                        stats.files_processed += 1;
                        if dropped {
                            stats.files_skipped += 1;
                        }
                        stats.records_seen += seen;
                        stats.records_skipped += skipped;
                        idx += n_threads;
                    }
                    Ok(stats)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });

    let mut total = RunStats::default();
    for result in per_thread_results {
        let stats = result?; // surfaces the first error found, after every worker has finished its own share
        total.files_processed += stats.files_processed;
        total.files_skipped += stats.files_skipped;
        total.records_seen += stats.records_seen;
        total.records_skipped += stats.records_skipped;
    }
    Ok(total)
}

/// Returns `(records_seen, records_skipped, file_dropped)` — `file_dropped`
/// is true when the file failed its structural pre-check (see
/// `looks_like_order_status_header`/`binary_format::looks_like_order_status_record`)
/// and was skipped in its entirety, in which case `records_seen`/
/// `records_skipped` are both 0 (nothing in it was even looked at).
fn stream_file(path: &Path, bytes_read: &Arc<AtomicU64>, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize, bool)> {
    let is_csv = path.extension().and_then(|e| e.to_str()) == Some("csv");
    let reader = open_reader(path, bytes_read)?;

    if is_csv {
        stream_csv(reader, on_record)
    } else {
        stream_binary(reader, on_record)
    }
}

fn stream_csv(reader: Box<dyn Read>, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize, bool)> {
    let mut seen = 0usize;
    let mut skipped = 0usize;
    let mut lines = BufReader::new(reader).lines();

    let Some(header) = lines.next() else {
        return Ok((0, 0, false)); // empty file — nothing to check or process
    };
    let header = header?;

    // Structural pre-check, before any row gets parsed: a wrong-schema
    // file (e.g. one of the lookup tables under `data/*/mapdir/` — some,
    // like `users.csv`, run into hundreds of thousands of rows) would
    // otherwise fail every single row and print its own
    // `[WARN] Skipping malformed CSV row N` line per row. One clear
    // warning and dropping the whole file is both cheaper and far more
    // useful than that flood.
    if !looks_like_order_status_header(&header) {
        eprintln!(
            "{}",
            format!(
                "[WARN] Skipping file: header doesn't look like order-status data ({} column(s), expected at least {MIN_COLUMNS}): {header}",
                header.split(',').count()
            )
            .yellow()
        );
        return Ok((0, 0, true));
    }

    for (line_no, line) in lines.enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_row(line) {
            Ok(Some(order)) => {
                seen += 1;
                on_record(order);
            }
            Ok(None) => seen += 1, // parsed fine, size rounded to nothing
            Err(()) => {
                skipped += 1;
                // +2: `lines` here starts counting from the first row AFTER
                // the header already consumed above (0-based), and this
                // message uses 1-based file line numbers.
                eprintln!("{}", format!("[WARN] Skipping malformed CSV row {}", line_no + 2).yellow());
            }
        }
    }

    Ok((seen, skipped, false))
}

fn stream_binary(reader: Box<dyn Read>, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize, bool)> {
    // `reader` (the gzip decoder, for every real file — `open_reader`
    // returns a raw unbuffered `MultiGzDecoder` for `.gz`) gets read
    // `RECORD_SIZE` (54) bytes at a time by `read_one_record`, once per
    // record — millions of times on a real multi-million-record file.
    // Without this, each of those small reads can turn into its own
    // decompression call with no read-ahead of its own; wrapping it in a
    // `BufReader` here means the decoder is asked for a big chunk at a
    // time (256 KiB) and every record after the first in that chunk is
    // just a cheap memory copy out of it. `stream_csv` already gets this
    // for free via `BufReader::lines()` — this was the one binary-path gap.
    let mut reader = BufReader::with_capacity(256 * 1024, reader);
    let mut seen = 0usize;
    let mut skipped = 0usize;
    let mut buf = [0u8; RECORD_SIZE];

    if !read_one_record(&mut reader, &mut buf)? {
        return Ok((0, 0, false)); // empty file — nothing to check or process
    }

    // Structural pre-check on just the first record, before committing to
    // stream the rest of the file — cheap (54 bytes), and lets a wrong
    // file (e.g. a foreign `.gz` that got swept in by `collect_input_files`)
    // get dropped with one clear warning instead of silently decoding
    // garbage bytes into a stream of nonsense `Order`s with no signal at
    // all that anything's wrong (see `looks_like_order_status_record`'s
    // doc comment for why the binary format has no other way to detect
    // this on its own).
    if !binary_format::looks_like_order_status_record(&buf) {
        eprintln!("{}", "[WARN] Skipping file: doesn't look like order-status binary data (failed first-record sanity check)".yellow());
        return Ok((0, 0, true));
    }

    loop {
        seen += 1;
        match binary_format::parse_record(&buf) {
            Some(order) => on_record(order),
            None => skipped += 1,
        }

        if !read_one_record(&mut reader, &mut buf)? {
            break; // clean EOF
        }
    }

    Ok((seen, skipped, false))
}

/// Fills `buf` with exactly `RECORD_SIZE` bytes. Returns `Ok(true)` on a
/// full record, `Ok(false)` on a clean end-of-file (no bytes read at all —
/// the normal way this loop ends), and an error for a truncated trailing
/// record (some bytes read, then EOF before `RECORD_SIZE` was reached).
/// Plain `Read::read_exact` doesn't distinguish those last two cases
/// cleanly enough for this loop to know when to stop versus complain.
fn read_one_record(reader: &mut dyn Read, buf: &mut [u8; RECORD_SIZE]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < RECORD_SIZE {
        let n = reader.read(&mut buf[filled..])?;
        if n == 0 {
            return if filled == 0 {
                Ok(false)
            } else {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated record at end of file"))
            };
        }
        filled += n;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a throwaway directory tree mimicking `data/order_statuses/`'s
    /// real shape (date folders containing hourly accepted/rejected file
    /// pairs) and checks `collect_input_files` finds everything, in the
    /// right order, while ignoring non-`.csv`/`.gz` files. Uses tiny empty
    /// files — this is purely about the traversal/sort logic, not about
    /// decoding real content (that's covered by the `streams_the_real_sample_gz_file_correctly`
    /// integration test and `inputs::binary_format`'s tests).
    #[test]
    fn collect_input_files_walks_a_date_folder_tree_in_order() {
        let root = std::env::temp_dir().join(format!("market_sim_test_{}", std::process::id()));
        let day1 = root.join("20251201");
        let day2 = root.join("20251202");
        fs::create_dir_all(&day1).unwrap();
        fs::create_dir_all(&day2).unwrap();

        // Deliberately created out of chronological order, to prove the
        // result is genuinely sorted rather than reflecting creation order.
        for (dir, name) in [
            (&day2, "sol_00.data.gz"),
            (&day1, "sol_09_rejected.data.gz"),
            (&day1, "sol_09.data.gz"),
            (&day1, "sol_00.data.gz"),
            (&day1, "README.md"), // unsupported extension -> must be ignored
            (&day1, "sol_00_rejected.data.gz"),
        ] {
            fs::write(dir.join(name), b"").unwrap();
        }

        let files = collect_input_files(&root).expect("should walk the tree");
        let names: Vec<String> = files.iter().map(|p| format!("{}/{}", p.parent().unwrap().file_name().unwrap().to_string_lossy(), p.file_name().unwrap().to_string_lossy())).collect();

        assert_eq!(
            names,
            vec![
                "20251201/sol_00.data.gz",
                "20251201/sol_00_rejected.data.gz",
                "20251201/sol_09.data.gz",
                "20251201/sol_09_rejected.data.gz",
                "20251202/sol_00.data.gz",
            ],
            "expected chronological date order, accepted-before-rejected within an hour, and README.md excluded"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn collect_input_files_accepts_a_single_file_path_too() {
        let path = std::env::temp_dir().join(format!("market_sim_test_single_{}.csv", std::process::id()));
        fs::write(&path, b"").unwrap();

        let files = collect_input_files(&path).expect("should accept a lone file");
        assert_eq!(files, vec![path.clone()]);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn parses_fixed_point_prices() {
        assert_eq!(parse_fixed_point("126.67"), Some(126_670_000));
        assert_eq!(parse_fixed_point("127"), Some(127_000_000));
        assert_eq!(parse_fixed_point("0.000001"), Some(1));
    }

    #[test]
    fn rounds_quantities_to_whole_units() {
        assert_eq!(round_to_unit("39.35"), Some(39));
        assert_eq!(round_to_unit("39.5"), Some(40));
        assert_eq!(round_to_unit("5175.0"), Some(5175));
    }

    #[test]
    fn parses_dataset_timestamps() {
        assert_eq!(
            parse_dataset_ts("2025-12-01 11:59:59.897401610"),
            Some(1_764_590_399_897_401_610)
        );
        assert_eq!(
            parse_dataset_ts("2025-01-01 00:00:00.000000000"),
            Some(1_735_689_600_000_000_000)
        );
    }

    #[test]
    fn parses_a_real_preview_row_end_to_end() {
        let row = "2025-12-01 11:59:59.897401610,237,False,1,False,126.67,5175.0,254384947819,0,0.0,False,False,False,False,False,0,0,0.0,5175.0,open,Limit,Alo";
        let order = parse_row(row).expect("row should parse").expect("row should yield an order");
        assert_eq!(order.oid, 254384947819);
        assert_eq!(order.user_id, "237");
        assert_eq!(order.status_id, 1);
        assert!(!order.is_ask);
        assert_eq!(order.limit_px, 126_670_000);
        assert_eq!(order.remaining, 5175);
        assert!(order.is_new_live_order());
    }

    /// End-to-end integration check against the real sample archive:
    /// gzip decompression + binary record parsing + the streaming loop,
    /// all together, on an actual file. `#[ignore]`d by default since it
    /// depends on `data/sample/` being present at a fixed relative path on
    /// this machine — run explicitly with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn streams_the_real_sample_gz_file_correctly() {
        let path = Path::new("../data/sample/order_statuses/20251201/sol_12.data.gz");
        let mut first_order: Option<Order> = None;
        let mut count = 0usize;

        let bytes_read = Arc::new(AtomicU64::new(0));
        let stats = stream_records(&[path.to_path_buf()], &bytes_read, |order| {
            if first_order.is_none() {
                first_order = Some(order);
            }
            count += 1;
        })
        .expect("streaming the real sample file should succeed");

        // The counted bytes are physical (compressed) file bytes, so they
        // should land somewhere under the file's own on-disk size — not
        // zero (nothing read), not wildly more than the compressed size.
        let file_len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        assert!(bytes_read.load(Ordering::Relaxed) > 0, "should have counted some bytes read");
        assert!(bytes_read.load(Ordering::Relaxed) <= file_len, "counted bytes shouldn't exceed the compressed file's own size");

        assert_eq!(stats.files_processed, 1);
        assert_eq!(stats.records_seen, count + stats.records_skipped);
        assert!(count > 100_000, "expected at least six figures of records in one hour, got {count}");

        // Cross-checked against the known first row of
        // data/sample/order_statuses_accepted_PREVIEW.csv (same underlying
        // record — see inputs::binary_format's real-data test for the
        // byte-level version of this same check).
        let first = first_order.expect("file should have produced at least one order");
        assert_eq!(first.ts, 1_764_590_399_897_401_610);
        assert_eq!(first.user_id, "237");
        assert_eq!(first.oid, 254384947819);
        assert_eq!(first.limit_px, 126_670_000);
        assert_eq!(first.remaining, 5175);
    }
}

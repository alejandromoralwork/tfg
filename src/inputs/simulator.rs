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

    for (line_no, line) in contents.lines().enumerate() {
        if line_no == 0 {
            continue; // header row
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_row(line) {
            Ok(Some(order)) => orders.push(order),
            // Valid row, but its size rounds to nothing worth trading —
            // not an error, just not worth constructing an order for.
            Ok(None) => {}
            Err(()) => eprintln!("⚠️  Skipping malformed row {} in {path}", line_no + 1),
        }
    }

    Ok(orders)
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

/// Opens `path` as a buffered byte stream, transparently gzip-decompressing
/// through `flate2::read::MultiGzDecoder` if the extension is `.gz`.
/// Streams throughout — the decompressed content is never held in memory
/// as one big buffer, only whatever the caller reads at a time.
fn open_reader(path: &Path) -> io::Result<Box<dyn Read>> {
    let file = BufReader::new(File::open(path)?);
    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        Ok(Box::new(MultiGzDecoder::new(file)))
    } else {
        Ok(Box::new(file))
    }
}

/// Streams every file in `files`, calling `on_record` immediately for each
/// decoded `Order` — never collects them into a `Vec`. Prints a short
/// progress line per file (there are at most 48 per day, so this doesn't
/// spam a multi-day run) so a long replay doesn't look hung.
pub fn stream_records(files: &[PathBuf], mut on_record: impl FnMut(Order)) -> io::Result<RunStats> {
    let mut stats = RunStats::default();

    for (i, path) in files.iter().enumerate() {
        println!("📂 [{}/{}] {}", i + 1, files.len(), path.display());
        // Explicit flush: stdout is fully (not line-)buffered when it's
        // not a terminal (piped/redirected), which is exactly how a long
        // `simulate` run's output is normally consumed — without this,
        // progress just silently accumulates in a buffer and never
        // appears until the process exits, making a multi-minute run look
        // hung even though it's working.
        io::stdout().flush().ok();
        let (seen, skipped) = stream_file(path, &mut on_record)?;
        stats.files_processed += 1;
        stats.records_seen += seen;
        stats.records_skipped += skipped;
    }

    Ok(stats)
}

fn stream_file(path: &Path, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize)> {
    let is_csv = path.extension().and_then(|e| e.to_str()) == Some("csv");
    let reader = open_reader(path)?;

    if is_csv {
        stream_csv(reader, on_record)
    } else {
        stream_binary(reader, on_record)
    }
}

fn stream_csv(reader: Box<dyn Read>, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize)> {
    let mut seen = 0usize;
    let mut skipped = 0usize;

    for (line_no, line) in BufReader::new(reader).lines().enumerate() {
        let line = line?;
        if line_no == 0 {
            continue; // header row
        }
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
                eprintln!("⚠️  Skipping malformed CSV row {}", line_no + 1);
            }
        }
    }

    Ok((seen, skipped))
}

fn stream_binary(mut reader: Box<dyn Read>, on_record: &mut impl FnMut(Order)) -> io::Result<(usize, usize)> {
    let mut seen = 0usize;
    let mut skipped = 0usize;
    let mut buf = [0u8; RECORD_SIZE];

    loop {
        if !read_one_record(reader.as_mut(), &mut buf)? {
            break; // clean EOF
        }
        seen += 1;
        match binary_format::parse_record(&buf) {
            Some(order) => on_record(order),
            None => skipped += 1,
        }

        // A single hour's worth of real data is routinely ~1M records —
        // without an in-file progress indicator, one large file would give
        // no visibility at all between its start and end.
        if seen % 250_000 == 0 {
            println!("   ... {seen} records so far");
            io::stdout().flush().ok();
        }
    }

    Ok((seen, skipped))
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

        let stats = stream_records(&[path.to_path_buf()], |order| {
            if first_order.is_none() {
                first_order = Some(order);
            }
            count += 1;
        })
        .expect("streaming the real sample file should succeed");

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

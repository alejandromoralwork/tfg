//! Reads the Hyperliquid order-status PREVIEW CSVs from `data/sample` (see
//! `../data/SCHEMA.md`) into `engines::common::Order` values that can be fed
//! straight into either simulator via `FbaSimulator::ingest` /
//! `CdaSimulator::ingest`.
//!
//! Deliberately builds `Order` directly (not through `Order::limit`, which
//! hardcodes `status_id = 1` / `is_trigger = false`) so a row's real
//! lifecycle status survives — a `canceled`/`filled`/rejected row still
//! produces an `Order`, it just won't pass `Order::is_new_live_order()`,
//! exactly like a live replay would gate it.
//!
//! No external crates: the schema has no embedded commas, so a plain
//! `split(',')` is sufficient, and prices/timestamps are decoded with
//! pure-integer arithmetic (matching this project's "no floats for
//! pricing" rule) rather than parsing through `f64`.

use std::fs;
use std::io;

use engines::common::{AssetPair, Order};

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
            Some(order) => orders.push(order),
            None => eprintln!("⚠️  Skipping malformed row {} in {path}", line_no + 1),
        }
    }

    Ok(orders)
}

fn parse_row(line: &str) -> Option<Order> {
    let cols: Vec<&str> = line.split(',').collect();
    if cols.len() < MIN_COLUMNS {
        return None;
    }

    let ts = parse_dataset_ts(cols[0])?;
    let user_id = cols[1].trim().to_string();
    let status_id: u8 = cols[3].trim().parse().ok()?;
    let is_ask = parse_bool(cols[4])?;
    let limit_px = parse_fixed_point(cols[5])?;
    let oid: u64 = cols[7].trim().parse().ok()?;
    let is_trigger = parse_bool(cols[11])?;
    let order_type_id: u8 = cols[15].trim().parse().ok()?;
    let tif_id: u8 = cols[16].trim().parse().ok()?;
    let orig_sz = round_to_unit(cols[18])?;
    let status = non_empty(cols[19]);
    let order_type = non_empty(cols[20]);
    let tif = non_empty(cols[21]);

    // The engine's Amount/quantity type has no fixed-point convention (see
    // docs/SIMULATION_GUIDE.md "known limitations"), so fractional sizes are
    // rounded to the nearest whole unit; a row that rounds to nothing isn't
    // worth constructing an order for.
    if orig_sz == 0 {
        return None;
    }

    Some(Order {
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
        status,
        order_type,
        tif,
        pair: AssetPair::default(),
        remaining: orig_sz,
        wallet: None,
        client_order_id: None,
        chain_id: None,
    })
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
/// (e.g. `126_670_000`), matching `engines::common::PRICE_SCALE` (1e6).
/// Pure integer arithmetic: split on `.`, parse the integer part, and
/// pad/truncate the fractional part to exactly 6 digits.
fn parse_fixed_point(s: &str) -> Option<u128> {
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
/// is a lossy but honest simplification (see docs/SIMULATION_GUIDE.md).
fn round_to_unit(s: &str) -> Option<u128> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let order = parse_row(row).expect("row should parse");
        assert_eq!(order.oid, 254384947819);
        assert_eq!(order.user_id, "237");
        assert_eq!(order.status_id, 1);
        assert!(!order.is_ask);
        assert_eq!(order.limit_px, 126_670_000);
        assert_eq!(order.remaining, 5175);
        assert!(order.is_new_live_order());
    }
}

//! Decodes the real Hyperliquid L4 order-status binary format (see
//! `data/SCHEMA.md`, "Binary Record Format" + "Price Encoding") — the
//! 54-byte packed records `data/order_statuses/**/*.data.gz` are made of,
//! as opposed to `inputs/simulator.rs`'s pre-decoded CSV PREVIEW path.
//!
//! Pure integer arithmetic throughout (no floats), matching this project's
//! pricing discipline — same reasoning as `inputs::simulator`'s
//! `parse_fixed_point`/`round_to_unit`.

use crate::types::{Amount, Order, Price, PRICE_SCALE};

/// Bytes per record — file size / 54 gives the exact record count.
pub(crate) const RECORD_SIZE: usize = 54;

/// Shared by both the unsigned and signed price encodings: scales a raw
/// `value` known to carry `decimals` decimal places into `PRICE_SCALE`
/// (1e6) fixed-point. If `decimals <= 6`, this is an exact integer
/// multiplication (`10^(6-decimals)`, no rounding at all). Only
/// `decimals == 7` — one more decimal place than `PRICE_SCALE` can
/// represent — needs rounding, done round-half-up rather than truncating.
fn scale_to_price_units(value: u128, decimals: u32) -> u128 {
    if decimals <= 6 {
        value * 10u128.pow(6 - decimals)
    } else {
        let divisor = 10u128.pow(decimals - 6);
        (value + divisor / 2) / divisor
    }
}

/// Decodes `limitPx`/`triggerPx` (and `sz`/`origSz`, via
/// `decode_qty_to_whole_units` below): bits 31-29 are the decimal-place
/// count, bits 28-0 are the integer value. Worked example from
/// `SCHEMA.md`: `$96,543.21` encodes as `decimals=2, value=9654321` ->
/// `encoded = (2 << 29) | 9654321 = 0x40935031`.
pub(crate) fn decode_price(encoded: u32) -> Price {
    let decimals = encoded >> 29;
    let value = (encoded & 0x1FFF_FFFF) as u128;
    scale_to_price_units(value, decimals)
}

/// Decodes `triggerCondition`'s signed variant: bits 31-29 decimals, bit
/// 28 sign (1 = negative), bits 27-0 value. Per `SCHEMA.md`, the input is
/// the raw 4 bytes reinterpreted as `u32` (NOT parsed as `i32` first —
/// this is a custom bit-packed sign, not two's-complement).
pub(crate) fn decode_signed_price(encoded: u32) -> i128 {
    let decimals = encoded >> 29;
    let is_negative = (encoded & 0x1000_0000) != 0;
    let value = (encoded & 0x0FFF_FFFF) as u128;
    let magnitude = scale_to_price_units(value, decimals) as i128;
    if is_negative {
        -magnitude
    } else {
        magnitude
    }
}

/// Decodes `sz`/`origSz` down to a whole-unit `Amount` — first unpacking
/// the same fixed-point encoding as a price (sizes use the identical
/// scheme), then rounding to the nearest whole unit
/// (`(scaled + PRICE_SCALE/2) / PRICE_SCALE`, i.e. round-half-up). This is
/// the exact same convention `inputs::simulator::round_to_unit` applies to
/// CSV quantities, kept consistent here so an order's size means the same
/// thing regardless of which loader produced it — the engine's `Amount`
/// type has no fixed-point convention of its own, only `Price` does.
pub(crate) fn decode_qty_to_whole_units(encoded: u32) -> Amount {
    let scaled = decode_price(encoded);
    (scaled + PRICE_SCALE / 2) / PRICE_SCALE
}

/// Parses one 54-byte record into an `Order`, mirroring
/// `inputs::simulator::parse_row`'s CSV path field-for-field, but reading
/// fixed byte offsets instead of comma-separated columns. Returns `None`
/// only when the decoded size rounds to nothing worth an order for (same
/// rule the CSV path uses) — every 54-byte chunk is otherwise structurally
/// well-formed by construction, unlike a CSV row, so there's no separate
/// "malformed" case here.
///
/// `status`/`order_type`/`tif` (the human-readable label strings the CSV
/// path resolves) are deliberately left `None`: resolving them would mean
/// an extra allocation per record, and at the scale this path is meant for
/// (potentially tens of millions of records) that adds up fast for no
/// benefit — the engines only ever read the numeric `status_id`/
/// `order_type_id`/`tif_id` fields.
pub(crate) fn parse_record(bytes: &[u8; RECORD_SIZE]) -> Option<Order> {
    let ts = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let user_id = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let is_builder = bytes[12] != 0;
    let status_id = bytes[13];
    let is_ask = bytes[14] != 0;
    let limit_px = decode_price(u32::from_le_bytes(bytes[15..19].try_into().unwrap()));
    let sz = decode_qty_to_whole_units(u32::from_le_bytes(bytes[19..23].try_into().unwrap()));
    let oid = u64::from_le_bytes(bytes[23..31].try_into().unwrap());
    let timestamp_diff = u32::from_le_bytes(bytes[31..35].try_into().unwrap());
    let trigger_condition = decode_signed_price(u32::from_le_bytes(bytes[35..39].try_into().unwrap()));
    let triggered = bytes[39] != 0;
    let is_trigger = bytes[40] != 0;
    let has_children = bytes[41] != 0;
    let is_position_tpsl = bytes[42] != 0;
    let reduce_only = bytes[43] != 0;
    let order_type_id = bytes[44];
    let tif_id = bytes[45];
    let trigger_px = decode_price(u32::from_le_bytes(bytes[46..50].try_into().unwrap()));
    let orig_sz = decode_qty_to_whole_units(u32::from_le_bytes(bytes[50..54].try_into().unwrap()));

    if orig_sz == 0 {
        return None;
    }

    Some(Order {
        ts,
        user_id: user_id.to_string(),
        is_builder,
        status_id,
        is_ask,
        limit_px,
        sz,
        oid,
        timestamp_diff,
        trigger_condition,
        triggered,
        is_trigger,
        has_children,
        is_position_tpsl,
        reduce_only,
        order_type_id,
        tif_id,
        trigger_px,
        orig_sz,
        closed_ts: 0,
        status: None,
        order_type: None,
        tif: None,
        remaining: orig_sz,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_schema_worked_example() {
        // $96,543.21 -> decimals=2, value=9654321 (data/SCHEMA.md's own example).
        let encoded = (2u32 << 29) | 9654321;
        assert_eq!(decode_price(encoded), 96_543_210_000); // 96543.21 * PRICE_SCALE
    }

    #[test]
    fn decode_price_handles_every_decimal_count() {
        assert_eq!(decode_price((0u32 << 29) | 127), 127_000_000); // 127, 0 decimals
        assert_eq!(decode_price((6u32 << 29) | 1), 1); // smallest representable unit
        // 7 decimals: one more digit than PRICE_SCALE holds -> rounds.
        // value=15 at 7 decimals = 0.0000015 -> *1e6 = 1.5 -> rounds up to 2.
        assert_eq!(decode_price((7u32 << 29) | 15), 2);
        // value=14 at 7 decimals = 0.0000014 -> *1e6 = 1.4 -> rounds down to 1.
        assert_eq!(decode_price((7u32 << 29) | 14), 1);
    }

    #[test]
    fn decode_signed_price_respects_the_sign_bit() {
        let positive = (1u32 << 29) | 50; // +5.0
        let negative = (1u32 << 29) | 0x1000_0000 | 50; // -5.0
        assert_eq!(decode_signed_price(positive), 5_000_000);
        assert_eq!(decode_signed_price(negative), -5_000_000);
    }

    #[test]
    fn decode_qty_rounds_to_whole_units() {
        // 39.35 -> rounds down to 39 (matches round_to_unit's CSV behavior).
        assert_eq!(decode_qty_to_whole_units((2u32 << 29) | 3935), 39);
        // 39.5 -> rounds up to 40 (round-half-up).
        assert_eq!(decode_qty_to_whole_units((1u32 << 29) | 395), 40);
    }

    #[test]
    fn parse_record_decodes_a_realistic_record() {
        // Hand-assemble one 54-byte record: a live "open" buy limit order,
        // ts=1000, userId=237, statusId=1 (open), isAsk=false,
        // limitPx=126.67, sz=origSz=5175.0, oid=254384947819.
        let mut bytes = [0u8; RECORD_SIZE];
        bytes[0..8].copy_from_slice(&1000u64.to_le_bytes());
        bytes[8..12].copy_from_slice(&237u32.to_le_bytes());
        bytes[12] = 0; // isBuilder
        bytes[13] = 1; // statusId = open
        bytes[14] = 0; // isAsk = false (buy)
        bytes[15..19].copy_from_slice(&((2u32 << 29) | 12667).to_le_bytes()); // 126.67
        bytes[19..23].copy_from_slice(&((1u32 << 29) | 51750).to_le_bytes()); // sz=5175.0
        bytes[23..31].copy_from_slice(&254384947819u64.to_le_bytes());
        // bytes[31..54] (timestampDiff..origSz) default to 0 except origSz, set below.
        bytes[44] = 0; // orderTypeId = Limit
        bytes[45] = 0; // tifId = Alo
        bytes[50..54].copy_from_slice(&((1u32 << 29) | 51750).to_le_bytes()); // origSz=5175.0

        let order = parse_record(&bytes).expect("record should decode");
        assert_eq!(order.ts, 1000);
        assert_eq!(order.user_id, "237");
        assert_eq!(order.status_id, 1);
        assert!(!order.is_ask);
        assert_eq!(order.limit_px, 126_670_000);
        assert_eq!(order.oid, 254384947819);
        assert_eq!(order.remaining, 5175);
        assert!(order.is_new_live_order());
    }

    /// The literal first 54 bytes of the real
    /// `data/sample/order_statuses/20251201/sol_12.data.gz` file
    /// (`zcat sol_12.data.gz | head -c 54 | xxd`), decoded and
    /// cross-checked field-by-field against the known-correct CSV
    /// PREVIEW row for the same record (`data/sample/order_statuses_accepted_PREVIEW.csv`'s
    /// first row: `2025-12-01 11:59:59.897401610,237,False,1,False,126.67,
    /// 5175.0,254384947819,...`). Every field matched exactly when this
    /// was checked by hand — this test pins that down permanently against
    /// real production data, not just synthetic bytes.
    #[test]
    fn parse_record_matches_real_sample_data_exactly() {
        #[rustfmt::skip]
        let bytes: [u8; RECORD_SIZE] = [
            0x0a, 0xf9, 0xf2, 0x97, 0x9e, 0x15, 0x7d, 0x18, // ts
            0xed, 0x00, 0x00, 0x00,                         // userId = 237
            0x00,                                           // isBuilder = false
            0x01,                                           // statusId = open
            0x00,                                           // isAsk = false (buy)
            0x7b, 0x31, 0x00, 0x40,                         // limitPx = 126.67
            0x26, 0xca, 0x00, 0x20,                         // sz = 5175.0
            0x6b, 0x42, 0x86, 0x3a, 0x3b, 0x00, 0x00, 0x00, // oid = 254384947819
            0x00, 0x00, 0x00, 0x00,                         // timestampDiff
            0x00, 0x00, 0x00, 0x00,                         // triggerCondition
            0x00,                                           // triggered
            0x00,                                           // isTrigger
            0x00,                                           // hasChildren
            0x00,                                           // isPositionTpsl
            0x00,                                           // reduceOnly
            0x00,                                           // orderTypeId = Limit
            0x00,                                           // tifId = Alo
            0x00, 0x00, 0x00, 0x00,                         // triggerPx
            0x26, 0xca, 0x00, 0x20,                         // origSz = 5175.0
        ];

        let order = parse_record(&bytes).expect("real record should decode");
        assert_eq!(order.ts, 1_764_590_399_897_401_610); // 2025-12-01 11:59:59.897401610
        assert_eq!(order.user_id, "237");
        assert!(!order.is_builder);
        assert_eq!(order.status_id, 1);
        assert!(!order.is_ask);
        assert_eq!(order.limit_px, 126_670_000); // 126.67
        assert_eq!(order.oid, 254384947819);
        assert_eq!(order.remaining, 5175); // sz == origSz == 5175.0
        assert_eq!(order.order_type_id, 0); // Limit
        assert_eq!(order.tif_id, 0); // Alo
        assert!(order.is_new_live_order());
    }

    #[test]
    fn parse_record_skips_zero_size_after_rounding() {
        let mut bytes = [0u8; RECORD_SIZE];
        bytes[13] = 1; // statusId = open
        // origSz encoded as 0.2 (decimals=1, value=2) -> rounds down to 0.
        bytes[50..54].copy_from_slice(&((1u32 << 29) | 2).to_le_bytes());
        assert!(parse_record(&bytes).is_none());
    }
}

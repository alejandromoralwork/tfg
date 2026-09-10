//! Behavioral test suite for both engines, run at runtime from the CLI
//! (`test engine continuous` / `test engine batch`) rather than only via
//! `cargo test`, so the checklist is available without a Rust toolchain in
//! hand. Every case constructs a fresh, isolated orderbook and fully
//! deterministic orders (explicit ids/timestamps, never wall-clock), feeds
//! it a hand-designed scenario, and checks the result against an
//! independently hand-computed expectation — not just "whatever the code
//! currently does".

use std::time::Duration;

use colored::Colorize;

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::metrics::timeseries::{
    BatchClearedEvent, BookSnapshot, MetricsRecorder, OrderMessage, TradeEvent, DEPTH_BPS_THRESHOLDS,
};
use crate::types::{EngineKind, Order, Side, Trade, PRICE_SCALE};

pub struct TestCase {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

fn check(name: &'static str, passed: bool, detail: impl Into<String>) -> TestCase {
    TestCase { name, passed, detail: detail.into() }
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

// ---- Deterministic order builders (fixed ids/timestamps, no wall-clock) ----

fn limit(oid: u64, user: &str, side: Side, price: u128, qty: u128, ts: u64) -> Order {
    Order::limit(oid, user, side, price, qty, ts)
}

fn market(oid: u64, user: &str, side: Side, qty: u128, ts: u64) -> Order {
    Order::market(oid, user, side, qty, ts)
}

/// A canceled order (status_id = 2) — should never enter either engine as
/// a resting/pending order itself, though (since this session's
/// cancellation fix) it now also carries a real side effect: it cancels
/// any existing live order sharing its `oid`.
fn non_live(oid: u64, user: &str, side: Side, price: u128, qty: u128, ts: u64) -> Order {
    let mut o = Order::limit(oid, user, side, price, qty, ts);
    o.status_id = 2;
    o
}

/// A lifecycle event carrying `oid` with a cancellation-type status_id
/// (`canceled` = 2, one of the 8 codes `Order::is_cancellation` matches).
/// Other fields are irrelevant — only `oid` and `status_id` matter to
/// `FbaOrderBook::cancel`/`CdaOrderBook::cancel`.
fn cancel_event(oid: u64, ts: u64) -> Order {
    let mut o = Order::limit(oid, "cancel-src", Side::Buy, 0, 0, ts);
    o.status_id = 2;
    o
}

/// A lifecycle event carrying `oid` with the `filled` status_id (5) —
/// deliberately NOT a cancellation-type code, so it must NOT remove a
/// matching live order (see `Order::is_cancellation`'s doc for why).
fn filled_event(oid: u64, ts: u64) -> Order {
    let mut o = Order::limit(oid, "fill-src", Side::Buy, 0, 0, ts);
    o.status_id = 5;
    o
}

// ---- Time-series metric builders (feed MetricsRecorder directly) ----

const TS_W: u64 = 1_000_000_000; // 1 s bucket, `simulate`'s default
const TS_WIDE: u64 = 100_000_000_000; // 100 s bucket — "everything lands in bucket 0"

/// Zero sub-band depth schedule (the common case).
const NO_SCHED: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()] = [(0, 0); DEPTH_BPS_THRESHOLDS.len()];

/// Relative-tolerance compare, for metrics whose magnitude is a raw
/// PRICE_SCALE-scaled quantity (`executed_notional`, `vwap`, `trader_surplus`)
/// where `approx_eq`'s fixed `abs < 0.01` is far tighter than an f64 sum of
/// 1e8-scale terms can hold.
fn approx_rel(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn tmsg(ts: u64, oid: u64, user: &str, side: Side, limit_price: Option<u128>, qty: u128) -> OrderMessage {
    OrderMessage { ts, oid, user_id: user.to_string(), side, limit_price, quantity: qty, accepted: true }
}

fn ttrade(ts: u64, price: u128, qty: u128, buy_oid: u64, sell_oid: u64, engine: EngineKind) -> Trade {
    Trade {
        trade_id: buy_oid,
        price,
        quantity: qty,
        buyer_id: "b".to_string(),
        seller_id: "s".to_string(),
        buy_order_id: buy_oid,
        sell_order_id: sell_oid,
        engine_type: engine,
        ts,
        trade_tx_hash: None,
        chain_id: None,
    }
}

/// A CDA trade with an explicit pre-trade reference mid and aggressor side.
fn tcda_trade(ts: u64, price: u128, qty: u128, buy_oid: u64, sell_oid: u64, reference: Option<u128>, side: Option<Side>) -> TradeEvent {
    TradeEvent { trade: ttrade(ts, price, qty, buy_oid, sell_oid, EngineKind::Cda), reference_price: reference, aggressor_side: side }
}

/// An FBA batch trade — no taker/maker, so `aggressor_side` is always `None`.
fn tfba_trade(ts: u64, price: u128, qty: u128, buy_oid: u64, sell_oid: u64, reference: Option<u128>) -> TradeEvent {
    TradeEvent { trade: ttrade(ts, price, qty, buy_oid, sell_oid, EngineKind::Fba), reference_price: reference, aggressor_side: None }
}

#[allow(clippy::too_many_arguments)]
fn tbook(
    ts: u64,
    best_bid: Option<u128>,
    best_ask: Option<u128>,
    best_bid_qty: u128,
    best_ask_qty: u128,
    bid_depth: u128,
    ask_depth: u128,
    sched: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    compute_us: u64,
) -> BookSnapshot {
    BookSnapshot {
        ts,
        best_bid,
        best_ask,
        best_bid_qty,
        best_ask_qty,
        bid_depth,
        ask_depth,
        depth_schedule: sched,
        compute_time: Duration::from_micros(compute_us),
    }
}

/// A book snapshot that only carries a price point (both sides == `mid`),
/// used as a forward-markout reference for realized-spread / Kyle's lambda.
fn tbook_px(ts: u64, mid: u128) -> BookSnapshot {
    tbook(ts, Some(mid), Some(mid), 0, 0, 0, 0, NO_SCHED, 0)
}

#[allow(clippy::too_many_arguments)]
fn tbatch(
    ts: u64,
    batch_open_ts: u64,
    clearing_price: Option<u128>,
    demand: u128,
    supply: u128,
    net_order_flow: f64,
    unexecuted: u128,
    best_buy: Option<u128>,
    best_sell: Option<u128>,
    sched: [(u128, u128); DEPTH_BPS_THRESHOLDS.len()],
    compute_us: u64,
) -> BatchClearedEvent {
    BatchClearedEvent {
        ts,
        batch_open_ts,
        clearing_price,
        demand_at_price: demand,
        supply_at_price: supply,
        net_order_flow,
        traded_quantity: demand.min(supply),
        unexecuted_quantity: unexecuted,
        best_unfilled_buy: best_buy,
        best_unfilled_sell: best_sell,
        depth_schedule: sched,
        compute_time: Duration::from_micros(compute_us),
    }
}

/// A priced batch that only carries a clearing-price point.
fn tbatch_px(ts: u64, batch_open_ts: u64, clearing_price: u128) -> BatchClearedEvent {
    tbatch(ts, batch_open_ts, Some(clearing_price), 0, 0, 0.0, 0, None, None, NO_SCHED, 0)
}

/// Print a checklist section and return whether every case passed.
pub fn print_checklist(engine_label: &str, cases: &[TestCase]) -> bool {
    let rule = "==========================================================================".cyan();
    println!("\n{rule}");
    println!("{}", format!("                       {engine_label} TEST CHECKLIST                      ").cyan().bold());
    println!("{rule}");

    for c in cases {
        if c.passed {
            println!("  {} {}", "[PASS]".green(), c.name);
        } else {
            println!("  {} {}", "[FAIL]".red(), c.name);
            println!("{}", format!("      -> {}", c.detail).red());
        }
    }

    let passed = cases.iter().filter(|c| c.passed).count();
    let total = cases.len();
    println!("--------------------------------------------------------------------------");
    if passed == total {
        println!("{}", format!("  RESULT: {passed}/{total} passed — {engine_label} OK").green().bold());
    } else {
        println!("{}", format!("  RESULT: {passed}/{total} passed — {} case(s) FAILING", total - passed).red().bold());
    }
    println!("{rule}\n");

    passed == total
}

// ============================================================================
// CDA (Continuous Double Auction)
// ============================================================================

pub fn run_cda_tests() -> Vec<TestCase> {
    vec![
        cda_resting_order_no_cross(),
        cda_simple_cross_exact_qty(),
        cda_partial_fill_resting_larger(),
        cda_multi_fill_walks_book(),
        cda_price_priority_over_time(),
        cda_time_priority_same_price(),
        cda_market_crosses_at_maker_price(),
        cda_market_no_liquidity_no_rest(),
        cda_market_order_partial_liquidity_fill_rate(),
        cda_non_live_order_filtered(),
        cda_sell_crosses_bid_only_at_or_below_bid_price(),
        cda_cancellation_removes_resting_order(),
        cda_cancellation_of_unknown_oid_is_harmless(),
        cda_filled_status_does_not_touch_resting_order(),
        cda_metrics_known_scenario(),
    ]
}

fn cda_resting_order_no_cross() -> TestCase {
    let mut book = CdaOrderBook::new();
    let trades = book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1));

    let ok = trades.is_empty() && book.bid_count() == 1 && book.asks_is_empty() && book.best_bid_order().unwrap().remaining == 10;
    check(
        "cda_resting_order_no_cross",
        ok,
        format!("trades={} bids={} asks={}", trades.len(), book.bid_count(), book.ask_count()),
    )
}

fn cda_simple_cross_exact_qty() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Bob", Side::Sell, 100, 10, 1)); // rests
    let trades = book.submit(limit(2, "Alice", Side::Buy, 100, 10, 2)); // crosses fully

    let ok = trades.len() == 1
        && trades[0].quantity == 10
        && trades[0].price == 100
        && trades[0].engine_type == EngineKind::Cda
        && book.bids_is_empty()
        && book.asks_is_empty();
    check(
        "cda_simple_cross_exact_qty",
        ok,
        format!("trades={trades:?} bids_left={} asks_left={}", book.bid_count(), book.ask_count()),
    )
}

fn cda_partial_fill_resting_larger() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Bob", Side::Sell, 100, 10, 1)); // rests, qty 10
    let trades = book.submit(limit(2, "Alice", Side::Buy, 100, 4, 2)); // smaller taker

    let ok = trades.len() == 1
        && trades[0].quantity == 4
        && book.bids_is_empty() // taker fully filled, nothing rests
        && book.ask_count() == 1
        && book.best_ask_order().unwrap().remaining == 6; // maker partially filled, stays resting
    check(
        "cda_partial_fill_resting_larger",
        ok,
        format!("trades={trades:?} ask_remaining={:?}", book.best_ask_order().map(|o| o.remaining)),
    )
}

fn cda_multi_fill_walks_book() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "S1", Side::Sell, 100, 3, 1)); // ask A, earlier
    book.submit(limit(2, "S2", Side::Sell, 100, 4, 2)); // ask B, same price, later
    let trades = book.submit(limit(3, "Buyer", Side::Buy, 101, 10, 3)); // walks through both, rests remainder

    let ok = trades.len() == 2
        && trades[0].seller_id == "S1" && trades[0].quantity == 3
        && trades[1].seller_id == "S2" && trades[1].quantity == 4
        && book.asks_is_empty()
        && book.bid_count() == 1
        && book.best_bid_order().unwrap().remaining == 3 // 10 - 3 - 4
        && book.best_bid_order().unwrap().limit_px == 101;
    check(
        "cda_multi_fill_walks_book",
        ok,
        format!("trades={trades:?} bids={:?}", book.bids_iter().map(|o| (o.user_id.clone(), o.remaining)).collect::<Vec<_>>()),
    )
}

fn cda_price_priority_over_time() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "S_worse", Side::Sell, 102, 5, 1)); // submitted first, worse price
    book.submit(limit(2, "S_better", Side::Sell, 100, 5, 2)); // submitted second, better price
    let trades = book.submit(limit(3, "Buyer", Side::Buy, 105, 5, 3));

    // Better price must win even though it arrived later.
    let ok = trades.len() == 1
        && trades[0].seller_id == "S_better"
        && trades[0].price == 100
        && book.ask_count() == 1
        && book.best_ask_order().unwrap().user_id == "S_worse";
    check(
        "cda_price_priority_over_time",
        ok,
        format!("trades={trades:?} remaining_asks={:?}", book.asks_iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_time_priority_same_price() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "B_early", Side::Buy, 100, 5, 1));
    book.submit(limit(2, "B_late", Side::Buy, 100, 5, 2));
    let trades = book.submit(limit(3, "Seller", Side::Sell, 100, 5, 3));

    let ok = trades.len() == 1 && trades[0].buyer_id == "B_early" && book.bid_count() == 1 && book.best_bid_order().unwrap().user_id == "B_late";
    check(
        "cda_time_priority_same_price",
        ok,
        format!("trades={trades:?} remaining_bids={:?}", book.bids_iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_market_crosses_at_maker_price() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Seller", Side::Sell, 100, 10, 1)); // resting limit
    let trades = book.submit(market(2, "Buyer", Side::Buy, 10, 2)); // market order, no price of its own

    let ok = trades.len() == 1 && trades[0].price == 100 && trades[0].quantity == 10 && book.asks_is_empty() && book.bids_is_empty();
    check("cda_market_crosses_at_maker_price", ok, format!("trades={trades:?}"))
}

fn cda_market_no_liquidity_no_rest() -> TestCase {
    let mut book = CdaOrderBook::new();
    let trades = book.submit(market(1, "Buyer", Side::Buy, 10, 1)); // empty book

    // Market orders never rest, even unfilled. fill_rate must reflect that
    // this order was never actually filled (0.0), not silently drop out of
    // the accounting the way an inferred "submitted - still resting"
    // formula would (a market order that never rests would otherwise
    // vanish from both sides of that subtraction).
    let ok = trades.is_empty() && book.bids_is_empty() && book.asks_is_empty() && book.fill_rate() == Some(0.0);
    check(
        "cda_market_no_liquidity_no_rest",
        ok,
        format!("trades={} bids={} asks={} fill_rate={:?}", trades.len(), book.bid_count(), book.ask_count(), book.fill_rate()),
    )
}

/// Regression check for a real bug found via manual testing: a market
/// order that finds NO liquidity at all disappears (doesn't rest, doesn't
/// trade) — if `fill_rate` were inferred as "submitted - still resting" it
/// would wrongly count that vanished order as filled, since it's absent
/// from both the resting book AND the executed trades. Here Frank's ask
/// gets fully consumed by Eve's market buy, then Zed's market buy arrives
/// to an empty book and finds nothing.
fn cda_market_order_partial_liquidity_fill_rate() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Frank", Side::Sell, 130, 6, 1)); // rests
    book.submit(market(2, "Eve", Side::Buy, 6, 2)); // fully consumes Frank's ask
    book.submit(market(3, "Zed", Side::Buy, 3, 3)); // no liquidity left, vanishes unfilled

    // total_submitted = 6 + 6 + 3 = 15; genuinely filled = 6 (Frank) + 6
    // (Eve) = 12; Zed's 3 units never traded. fill_rate must be 12/15 =
    // 0.8, NOT 1.0 (which is what "submitted - still_resting" would give,
    // since still_resting is 0 here regardless of Zed's unfilled order).
    let ok = book.trade_count() == 1 && book.executed_volume() == 6 && book.fill_rate().is_some_and(|v| approx_eq(v, 0.8));
    check(
        "cda_market_order_partial_liquidity_fill_rate",
        ok,
        format!("trade_count={} volume={} fill_rate={:?}", book.trade_count(), book.executed_volume(), book.fill_rate()),
    )
}

fn cda_non_live_order_filtered() -> TestCase {
    let mut book = CdaOrderBook::new();
    let trades = book.submit(non_live(1, "X", Side::Buy, 100, 10, 1));

    let ok = trades.is_empty() && book.bids_is_empty() && book.asks_is_empty();
    check("cda_non_live_order_filtered", ok, format!("trades={} bids={} asks={}", trades.len(), book.bid_count(), book.ask_count()))
}

/// Regression check for a real matching-direction bug found while writing
/// the cancellation tests below: a sell only crosses a resting bid when
/// its ask price is AT OR BELOW the bid (seller willing to accept no more
/// than the buyer offers) — the previous `check_price_match` applied the
/// same `taker_px >= maker_px` comparison used for buys-crossing-asks to
/// this side too, which is backwards. Covers both directions: an
/// aggressive sell (well below the bid) must cross, and a passive sell
/// (above the bid) must NOT cross and should simply rest instead.
fn cda_sell_crosses_bid_only_at_or_below_bid_price() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Buyer", Side::Buy, 100, 10, 1)); // resting bid @100

    let aggressive_trades = book.submit(limit(2, "AggressiveSeller", Side::Sell, 90, 4, 2)); // 90 <= 100 -> must cross
    let aggressive_ok = aggressive_trades.len() == 1 && aggressive_trades[0].quantity == 4 && aggressive_trades[0].price == 100;

    let passive_trades = book.submit(limit(3, "PassiveSeller", Side::Sell, 105, 3, 3)); // 105 > 100 -> must NOT cross, must rest
    let passive_ok = passive_trades.is_empty() && book.ask_count() == 1 && book.best_ask_order().unwrap().user_id == "PassiveSeller";

    let ok = aggressive_ok && passive_ok;
    check(
        "cda_sell_crosses_bid_only_at_or_below_bid_price",
        ok,
        format!("aggressive_trades={aggressive_trades:?} passive_trades={passive_trades:?} asks={:?}", book.asks_iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_cancellation_removes_resting_order() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests, no counterparty
    book.submit(limit(2, "Bob", Side::Sell, 105, 5, 2)); // rests too, doesn't cross
    book.submit(cancel_event(1, 3)); // cancel Alice's resting bid by oid

    let ok = book.bids_is_empty() && book.ask_count() == 1 && book.best_ask_order().unwrap().user_id == "Bob";
    check(
        "cda_cancellation_removes_resting_order",
        ok,
        format!("bids={} asks={:?}", book.bid_count(), book.asks_iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_cancellation_of_unknown_oid_is_harmless() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests

    // Cancel an oid that was never submitted as live.
    let removed = book.cancel(999);

    let ok = !removed && book.bid_count() == 1 && book.best_bid_order().unwrap().oid == 1;
    check("cda_cancellation_of_unknown_oid_is_harmless", ok, format!("removed={removed} bids={}", book.bid_count()))
}

fn cda_filled_status_does_not_touch_resting_order() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests
    book.submit(filled_event(1, 2)); // an external "filled" event for the same oid

    // Per the design decision, fills are NOT replayed — Alice's order must
    // still be sitting there completely untouched.
    let ok = book.bid_count() == 1 && book.best_bid_order().unwrap().oid == 1 && book.best_bid_order().unwrap().remaining == 10;
    check(
        "cda_filled_status_does_not_touch_resting_order",
        ok,
        format!("bids={:?}", book.bids_iter().map(|o| (o.oid, o.remaining)).collect::<Vec<_>>()),
    )
}

fn cda_metrics_known_scenario() -> TestCase {
    // Prices here are PRICE_SCALE-scaled (like everywhere else in the
    // engine — CLI `add` and the CSV loader both do the same) so
    // `executed_notional`, which divides by PRICE_SCALE, produces a
    // meaningful number instead of truncating to 0 on tiny raw prices.
    let p = |raw: u128| raw * PRICE_SCALE;

    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "S1", Side::Sell, p(102), 20, 1)); // rests, ask depth 20 @102
    book.submit(limit(2, "S2", Side::Sell, p(105), 30, 2)); // rests, ask depth 30 @105
    book.submit(limit(3, "Buyer", Side::Buy, p(110), 15, 3)); // crosses S1 partially: 15 of 20
    book.submit(limit(4, "B1", Side::Buy, p(95), 10, 4)); // non-crossing, rests as bid

    // Hand-computed expectations:
    //   asks left: S1 remaining 5 @102, S2 remaining 30 @105 -> ask_depth 35
    //   bids left: B1 remaining 10 @95                        -> bid_depth 10
    //   best_bid=95, best_ask=102, mid=98.5 -> spread = (102-95)/98.5*10000 = 710.66bps
    //   depth_at_best = 10 + 35 = 45
    //   book_imbalance = (10-35)/(10+35) = -0.5556
    //   total_submitted = 20+30+15+10 = 75; still_resting = 45; filled = 30 -> fill_rate = 0.4
    //   executed_notional = 15 * 102 (price scale cancels out) = 1530
    let trade_count_ok = book.trade_count() == 1;
    let volume_ok = book.executed_volume() == 15;
    let notional_ok = book.executed_notional() == 15 * 102;
    let depth_ok = book.depth_at_best() == 45;
    let spread_ok = book.quoted_spread_bps().is_some_and(|v| approx_eq(v, 710.66));
    let imbalance_ok = book.book_imbalance().is_some_and(|v| approx_eq(v, -0.5556));
    let fill_rate_ok = book.fill_rate().is_some_and(|v| approx_eq(v, 0.4));

    let ok = trade_count_ok && volume_ok && notional_ok && depth_ok && spread_ok && imbalance_ok && fill_rate_ok;
    check(
        "cda_metrics_known_scenario",
        ok,
        format!(
            "trade_count={} volume={} notional={} depth={} spread={:?} imbalance={:?} fill_rate={:?}",
            book.trade_count(),
            book.executed_volume(),
            book.executed_notional(),
            book.depth_at_best(),
            book.quoted_spread_bps(),
            book.book_imbalance(),
            book.fill_rate()
        ),
    )
}

// ============================================================================
// FBA (Frequent Batch Auction)
// ============================================================================

pub fn run_fba_tests() -> Vec<TestCase> {
    vec![
        fba_empty_batch_no_clear(),
        fba_simple_full_match(),
        fba_rationing_price_time_priority(),
        fba_market_order_queues_then_clears_at_uniform_price(),
        fba_tie_no_history_picks_lower_price(),
        fba_all_market_no_history_preserves_orders(),
        fba_all_market_with_history_still_rolls_over(),
        fba_non_live_order_filtered(),
        fba_cancellation_removes_pending_order(),
        fba_cancellation_of_unknown_oid_is_harmless(),
        fba_filled_status_does_not_touch_pending_order(),
        fba_residual_rolls_into_pending(),
        fba_metrics_after_partial_clear(),
        fba_tie_with_history_picks_closest_price(),
    ]
}

fn fba_empty_batch_no_clear() -> TestCase {
    let mut book = FbaOrderBook::new();
    let result = book.clear();
    check("fba_empty_batch_no_clear", result.is_none(), "expected clear() on an empty book to return None")
}

fn fba_simple_full_match() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1));
    book.submit(limit(2, "Bob", Side::Sell, 100, 10, 1));
    let Some(result) = book.clear() else {
        return check("fba_simple_full_match", false, "clear() returned None, expected a full match at 100");
    };

    let ok = result.clearing_price == 100
        && result.traded_quantity == 10
        && result.trades.len() == 1
        && result.trades[0].engine_type == EngineKind::Fba
        && book.pending_orders.is_empty()
        && book.last_clearing_price == Some(100);
    check(
        "fba_simple_full_match",
        ok,
        format!("price={} qty={} trades={} pending_left={}", result.clearing_price, result.traded_quantity, result.trades.len(), book.pending_orders.len()),
    )
}

/// Same shape as `docs/ENGINE_DESIGN.md`'s worked example (B1/B2/B3/S1),
/// with S1's price moved from 90 to 100 so there's a UNIQUE
/// volume-maximizing candidate (see `fba_tie_no_history_picks_lower_price`
/// below for why the original 90 creates a genuine tie).
fn fba_rationing_price_time_priority() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "B1", Side::Buy, 105, 10, 1));
    book.submit(limit(2, "B2", Side::Buy, 100, 10, 2));
    book.submit(limit(3, "B3", Side::Buy, 100, 10, 3));
    book.submit(limit(4, "S1", Side::Sell, 100, 15, 1));
    let Some(result) = book.clear() else {
        return check("fba_rationing_price_time_priority", false, "clear() returned None, expected a match at 100");
    };

    // B1 (best price) fills fully (10). S1 has 5 left -> fills the earlier
    // of the two tied-at-100 buyers, B2, for 5. B3 gets 0 and rolls over
    // fully; B2's remaining 5 rolls over too.
    let ok = result.clearing_price == 100
        && result.traded_quantity == 15
        && result.trades.len() == 2
        && result.trades[0].buyer_id == "B1" && result.trades[0].quantity == 10
        && result.trades[1].buyer_id == "B2" && result.trades[1].quantity == 5
        && book.pending_orders.len() == 2
        && book.pending_orders.iter().any(|o| o.user_id == "B2" && o.remaining == 5)
        && book.pending_orders.iter().any(|o| o.user_id == "B3" && o.remaining == 10);
    check(
        "fba_rationing_price_time_priority",
        ok,
        format!(
            "price={} qty={} trades={:?} pending={:?}",
            result.clearing_price,
            result.traded_quantity,
            result.trades.iter().map(|t| (t.buyer_id.clone(), t.quantity)).collect::<Vec<_>>(),
            book.pending_orders.iter().map(|o| (o.user_id.clone(), o.remaining)).collect::<Vec<_>>()
        ),
    )
}

/// Regression test for a real point of user confusion: a market order
/// added to an FBA batch does NOT execute on `submit` — it only queues,
/// exactly like a limit order, and waits for `clear()` like everything
/// else in the batch. Once `clear()` runs, the market order gets top
/// matching priority (`order_priority` gives it `(0, 0)`, ahead of every
/// limit order) and walks the cheapest resting sells first.
///
/// Also pins down something easy to get wrong by eyeballing real data
/// alone (as happened live: the sample data's resting sells all happened
/// to share one price, so it wasn't visible from the CLI output alone
/// whether a trade prices at the uniform clearing price or at each
/// maker's own price — they looked identical by coincidence). Here S1@100
/// and S2@105 are deliberately DIFFERENT prices, so this test can prove
/// both trades print at the single uniform clearing price (105 — the
/// volume-maximizing candidate, since it lets S2's extra supply in), not
/// each maker's own resting price the way CDA would.
fn fba_market_order_queues_then_clears_at_uniform_price() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "S1", Side::Sell, 100, 8, 1));
    book.submit(limit(2, "S2", Side::Sell, 105, 20, 1));

    let trades_before_clear = book.executed_trades.len();
    book.submit(market(3, "Buyer", Side::Buy, 12, 2));
    let queued_without_executing = book.executed_trades.len() == trades_before_clear && book.pending_orders.len() == 3;

    let Some(result) = book.clear() else {
        return check("fba_market_order_queues_then_clears_at_uniform_price", false, "clear() returned None, expected a match at 105");
    };

    let ok = queued_without_executing
        && result.clearing_price == 105
        && result.traded_quantity == 12
        && result.trades.len() == 2
        && result.trades[0].seller_id == "S1" && result.trades[0].quantity == 8 && result.trades[0].price == 105
        && result.trades[1].seller_id == "S2" && result.trades[1].quantity == 4 && result.trades[1].price == 105
        && book.pending_orders.len() == 1
        && book.pending_orders[0].user_id == "S2"
        && book.pending_orders[0].remaining == 16;
    check(
        "fba_market_order_queues_then_clears_at_uniform_price",
        ok,
        format!(
            "queued_without_executing={queued_without_executing} price={} qty={} trades={:?} pending={:?}",
            result.clearing_price,
            result.traded_quantity,
            result.trades.iter().map(|t| (t.seller_id.clone(), t.quantity, t.price)).collect::<Vec<_>>(),
            book.pending_orders.iter().map(|o| (o.user_id.clone(), o.remaining)).collect::<Vec<_>>()
        ),
    )
}

/// Confirms a real tie: with Buyer@90 vs Seller@100 (no overlap), both
/// candidate prices give 0 matched volume and the same imbalance (10), so
/// with no `last_clearing_price` history yet the deterministic "prefer the
/// lower price" fallback picks 90 — and `clear()` still returns `Some`
/// even though nothing actually traded.
fn fba_tie_no_history_picks_lower_price() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Buyer", Side::Buy, 90, 10, 1));
    book.submit(limit(2, "Seller", Side::Sell, 100, 10, 1));
    let Some(result) = book.clear() else {
        return check("fba_tie_no_history_picks_lower_price", false, "clear() returned None, expected Some with 0 trades");
    };

    let ok = result.clearing_price == 90 && result.traded_quantity == 0 && result.trades.is_empty() && book.pending_orders.len() == 2;
    check(
        "fba_tie_no_history_picks_lower_price",
        ok,
        format!("price={} qty={} trades={} pending={}", result.clearing_price, result.traded_quantity, result.trades.len(), book.pending_orders.len()),
    )
}

/// Regression check for a real bug found while writing this suite: an
/// all-market-order batch with no price history has no candidate price at
/// all, so `clear()` must return `None` WITHOUT losing the orders — they
/// have to still be sitting in `pending_orders` afterward, ready for a
/// later batch once some price history exists.
fn fba_all_market_no_history_preserves_orders() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(market(1, "Buyer", Side::Buy, 10, 1));
    book.submit(market(2, "Seller", Side::Sell, 10, 1));
    let result = book.clear();

    let ok = result.is_none() && book.pending_orders.len() == 2;
    check(
        "fba_all_market_no_history_preserves_orders",
        ok,
        format!("result_is_some={} pending_left={}", result.is_some(), book.pending_orders.len()),
    )
}

/// An all-market-order batch never clears — not even when
/// `last_clearing_price` exists. `candidate_prices` deliberately doesn't
/// anchor on stale history to invent a price for market orders; instead
/// the whole batch rolls into `pending_orders` for the next one, same as
/// `fba_all_market_no_history_preserves_orders`. This test exists
/// specifically to prove that having history present doesn't change that
/// outcome — it used to (see git history), which was a bug: pricing
/// market orders off of whatever the market happened to be doing several
/// batches ago is exactly the kind of "guessing" clearing should never do.
fn fba_all_market_with_history_still_rolls_over() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "A", Side::Buy, 100, 5, 1));
    book.submit(limit(2, "B", Side::Sell, 100, 5, 1));
    book.clear(); // establishes last_clearing_price = Some(100)

    book.submit(market(3, "C", Side::Buy, 8, 2));
    book.submit(market(4, "D", Side::Sell, 8, 2));
    let result = book.clear();

    let ok = result.is_none() && book.last_clearing_price == Some(100) && book.pending_orders.len() == 2;
    check(
        "fba_all_market_with_history_still_rolls_over",
        ok,
        format!(
            "result_is_some={} last_clearing_price={:?} pending={}",
            result.is_some(),
            book.last_clearing_price,
            book.pending_orders.len()
        ),
    )
}

fn fba_non_live_order_filtered() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(non_live(1, "X", Side::Buy, 100, 10, 1));

    let ok = book.pending_orders.is_empty() && book.clear().is_none();
    check("fba_non_live_order_filtered", ok, format!("pending={}", book.pending_orders.len()))
}

fn fba_cancellation_removes_pending_order() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // queued, no counterparty yet
    book.submit(limit(2, "Bob", Side::Buy, 95, 5, 2)); // also queued, different oid
    book.submit(cancel_event(1, 3)); // cancel Alice's queued order by oid

    let ok = book.pending_orders.len() == 1 && book.pending_orders[0].user_id == "Bob";
    check(
        "fba_cancellation_removes_pending_order",
        ok,
        format!("pending={:?}", book.pending_orders.iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn fba_cancellation_of_unknown_oid_is_harmless() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // queued

    let removed = book.cancel(999); // never submitted

    let ok = !removed && book.pending_orders.len() == 1 && book.pending_orders[0].oid == 1;
    check("fba_cancellation_of_unknown_oid_is_harmless", ok, format!("removed={removed} pending={}", book.pending_orders.len()))
}

fn fba_filled_status_does_not_touch_pending_order() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // queued
    book.submit(filled_event(1, 2)); // an external "filled" event for the same oid

    // Per the design decision, fills are NOT replayed — Alice's order must
    // still be sitting there completely untouched.
    let ok = book.pending_orders.len() == 1 && book.pending_orders[0].oid == 1 && book.pending_orders[0].remaining == 10;
    check(
        "fba_filled_status_does_not_touch_pending_order",
        ok,
        format!("pending={:?}", book.pending_orders.iter().map(|o| (o.oid, o.remaining)).collect::<Vec<_>>()),
    )
}

fn fba_residual_rolls_into_pending() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Buyer", Side::Buy, 100, 20, 1));
    book.submit(limit(2, "Seller", Side::Sell, 100, 12, 1));
    let Some(result) = book.clear() else {
        return check("fba_residual_rolls_into_pending", false, "clear() returned None, expected a match at 100");
    };

    let ok = result.traded_quantity == 12
        && result.trades.len() == 1
        && book.pending_orders.len() == 1
        && book.pending_orders[0].user_id == "Buyer"
        && book.pending_orders[0].remaining == 8;
    check(
        "fba_residual_rolls_into_pending",
        ok,
        format!("qty={} pending={:?}", result.traded_quantity, book.pending_orders.iter().map(|o| (o.user_id.clone(), o.remaining)).collect::<Vec<_>>()),
    )
}

fn fba_metrics_after_partial_clear() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "Buyer", Side::Buy, 100, 20, 1));
    book.submit(limit(2, "Seller", Side::Sell, 100, 12, 1));
    book.clear();

    // Hand-computed: demand_at_price=20, supply_at_price=12, traded=12.
    //   unexecuted_residual_share = |20-12| / max(20,12) = 8/20 = 0.4
    //   total_submitted=32, still_pending=8 (Buyer's leftover), filled=24 -> fill_rate=0.75
    //   quoted_spread_bps: Seller fully filled and removed -> no best_unfilled_sell -> None
    let residual_ok = book.unexecuted_residual_share().is_some_and(|v| approx_eq(v, 0.4));
    let fill_rate_ok = book.fill_rate().is_some_and(|v| approx_eq(v, 0.75));
    let spread_ok = book.quoted_spread_bps().is_none();
    let depth_ok = book.depth_at_best() == 8;

    let ok = residual_ok && fill_rate_ok && spread_ok && depth_ok;
    check(
        "fba_metrics_after_partial_clear",
        ok,
        format!(
            "residual_share={:?} fill_rate={:?} spread={:?} depth={}",
            book.unexecuted_residual_share(),
            book.fill_rate(),
            book.quoted_spread_bps(),
            book.depth_at_best()
        ),
    )
}

/// Same ambiguous Buyer@90/Seller@100 setup as
/// `fba_tie_no_history_picks_lower_price`, but this time with
/// `last_clearing_price` seeded at 97 first (via a trivial exact clear).
/// Both candidates still tie on volume(0)/imbalance(10), but 100 is closer
/// to 97 than 90 is (3 vs 7) — so the winner flips from 90 to 100 purely
/// because of price-continuity history. Demonstrates both tie-break
/// sub-rules (no-history vs. with-history) against the same core scenario.
fn fba_tie_with_history_picks_closest_price() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "P1", Side::Buy, 97, 3, 1));
    book.submit(limit(2, "P2", Side::Sell, 97, 3, 1));
    book.clear(); // last_clearing_price = Some(97)

    book.submit(limit(3, "Buyer", Side::Buy, 90, 10, 2));
    book.submit(limit(4, "Seller", Side::Sell, 100, 10, 2));
    let Some(result) = book.clear() else {
        return check("fba_tie_with_history_picks_closest_price", false, "clear() returned None, expected Some with 0 trades");
    };

    let ok = result.clearing_price == 100 && result.traded_quantity == 0;
    check(
        "fba_tie_with_history_picks_closest_price",
        ok,
        format!("price={} (expected 100, vs. 90 with no history)", result.clearing_price),
    )
}

// ============================================================================
// Time-series metric catalogue (metrics::timeseries::MetricsRecorder)
// ============================================================================
//
// A separate code path from the engine getters above: these drive the
// streaming aggregator `simulate` uses, with hand-built events, and check each
// of the ~32 CSV columns against an independently hand-computed value. Uses
// `finish` (not the `#[cfg(test)]`-only `finalize`) so it runs in the release
// binary / the container.

pub fn run_timeseries_metric_tests() -> Vec<TestCase> {
    vec![
        ts_cda_liquidity_and_book_metrics(),
        ts_cda_effective_realized_impact(),
        ts_cda_volatility_and_amihud(),
        ts_cda_execution_allocation(),
        ts_cda_kyle_lambda_known_slope(),
        ts_fba_batch_liquidity_metrics(),
        ts_fba_effective_realized_impact(),
        ts_fba_kyle_lambda_net_order_flow(),
    ]
}

/// Book-derived liquidity columns: quoted spread, depth-at-best, depth bands,
/// imbalance, whole-book depth, plus the wall-clock instrumentation columns
/// (deterministic here because `compute_time` is hand-set).
fn ts_cda_liquidity_and_book_metrics() -> TestCase {
    let mut rec = MetricsRecorder::new(EngineKind::Cda, TS_W);
    rec.set_anchor(0);
    let messages = vec![
        tmsg(1, 1, "m", Side::Buy, None, 1),
        tmsg(2, 2, "m", Side::Buy, None, 1),
        tmsg(3, 3, "m", Side::Buy, None, 1),
    ];
    // S1: bid 100 / ask 102, touch 8/12, book 40/60, bands (5,7)/(20,25)/(40,60), 200us
    rec.record_book_snapshot(tbook(1, Some(100), Some(102), 8, 12, 40, 60, [(5, 7), (20, 25), (40, 60)], 200));
    // S2: bid 100 / ask 104, touch 10/10, book 50/50, bands (6,6)/(20,20)/(55,45), 400us
    rec.record_book_snapshot(tbook(2, Some(100), Some(104), 10, 10, 50, 50, [(6, 6), (20, 20), (55, 45)], 400));
    let series = rec.finish(&messages, 3);
    if series.len() != 1 {
        return check("ts_cda_liquidity_and_book_metrics", false, format!("expected 1 row, got {}", series.len()));
    }
    let r = &series[0];
    // spread: (2/101 + 4/102)/2 * 1e4 = 295.088332 bps
    // depth_at_best: ((8+12)/2 + (10+10)/2)/2 = 10 ; imbalance: (-0.2 + 0)/2 = -0.1
    // total_book_depth: (50 + 50)/2 = 50
    // d10 = (6 + 6)/2 = 6 ; d50 = (22.5 + 20)/2 = 21.25 ; d100 = (50 + 50)/2 = 50
    // latency = (200 + 400)/2 = 300 us ; throughput = 3 msgs / 0.0006 s = 5000
    // fill_rate = 0 filled / 3 orig = 0
    let ok = r.quoted_spread_bps.is_some_and(|v| approx_eq(v, 295.088332))
        && r.depth_at_best.is_some_and(|v| approx_eq(v, 10.0))
        && r.book_imbalance.is_some_and(|v| approx_eq(v, -0.1))
        && r.total_book_depth.is_some_and(|v| approx_eq(v, 50.0))
        && r.depth_within_bps[0].is_some_and(|v| approx_eq(v, 6.0))
        && r.depth_within_bps[1].is_some_and(|v| approx_eq(v, 21.25))
        && r.depth_within_bps[2].is_some_and(|v| approx_eq(v, 50.0))
        && r.avg_clearing_latency_micros.is_some_and(|v| approx_eq(v, 300.0))
        && r.throughput_orders_per_sec.is_some_and(|v| approx_eq(v, 5000.0))
        && r.fill_rate.is_some_and(|v| approx_eq(v, 0.0));
    check(
        "ts_cda_liquidity_and_book_metrics",
        ok,
        format!(
            "spread={:?} depth={:?} imb={:?} total={:?} d={:?}/{:?}/{:?} lat={:?} tput={:?} fill={:?}",
            r.quoted_spread_bps, r.depth_at_best, r.book_imbalance, r.total_book_depth,
            r.depth_within_bps[0], r.depth_within_bps[1], r.depth_within_bps[2],
            r.avg_clearing_latency_micros, r.throughput_orders_per_sec, r.fill_rate
        ),
    )
}

/// The spread-decomposition columns (effective / realized / price impact at
/// 1/5/30 s) plus volume, notional, VWAP, trade count, trader surplus,
/// relative price dispersion, order-to-trade ratio — all in one CDA bucket.
fn ts_cda_effective_realized_impact() -> TestCase {
    let p = |x: u128| x * PRICE_SCALE;
    let mut rec = MetricsRecorder::new(EngineKind::Cda, TS_WIDE);
    rec.set_anchor(0);
    let messages = vec![
        tmsg(500_000, 10, "u", Side::Buy, Some(p(102)), 4),
        tmsg(500_000, 20, "u", Side::Sell, Some(p(100)), 4),
        tmsg(500_000, 11, "u", Side::Buy, Some(p(104)), 6),
        tmsg(500_000, 21, "u", Side::Sell, Some(p(100)), 6),
    ];
    // Two buyer-initiated trades, pre-trade mid p(100).
    rec.record_trade(tcda_trade(1_000_000, p(101), 4, 10, 20, Some(p(100)), Some(Side::Buy)));
    rec.record_trade(tcda_trade(2_000_000, p(103), 6, 11, 21, Some(p(100)), Some(Side::Buy)));
    // Forward mids at +1 s / +5 s / +30 s (first series point at/after each target).
    rec.record_book_snapshot(tbook_px(1_500_000_000, p(100)));
    rec.record_book_snapshot(tbook_px(5_500_000_000, p(101)));
    rec.record_book_snapshot(tbook_px(30_500_000_000, p(98)));
    let series = rec.finish(&messages, 30_500_000_000);
    if series.len() != 1 {
        return check("ts_cda_effective_realized_impact", false, format!("expected 1 row, got {}", series.len()));
    }
    let r = &series[0];
    // effective: (200*4 + 600*6)/10 = 440
    // realized_1s (mid p100): (200*4 + 600*6)/10 = 440 -> impact_1s = 0
    // realized_5s (mid p101): (0*4 + 400*6)/10 = 240 -> impact_5s = 200
    // realized_30s (mid p98): (600*4 + 1000*6)/10 = 840 -> impact_30s = -400
    // executed_volume 10 ; executed_notional 4*p(101)+6*p(103) = 1_022_000_000
    // vwap = 102_200_000 ; trade_count 2
    // trader_surplus = 4e6 + 4e6 + 6e6 + 18e6 = 32_000_000
    // dispersion = stddev([p101,p103])/mean * 1e4 = 1e6 / 102e6 * 1e4 bps
    // order_to_trade_ratio = 4 msgs / 2 trades = 2
    let ok = r.effective_spread_bps.is_some_and(|v| approx_eq(v, 440.0))
        && r.realized_spread_bps_1s.is_some_and(|v| approx_eq(v, 440.0))
        && r.realized_spread_bps_5s.is_some_and(|v| approx_eq(v, 240.0))
        && r.realized_spread_bps_30s.is_some_and(|v| approx_eq(v, 840.0))
        && r.price_impact_bps_1s.is_some_and(|v| approx_eq(v, 0.0))
        && r.price_impact_bps_5s.is_some_and(|v| approx_eq(v, 200.0))
        && r.price_impact_bps_30s.is_some_and(|v| approx_eq(v, -400.0))
        && r.executed_volume == 10.0
        && approx_rel(r.executed_notional, 1_022_000_000.0)
        && r.vwap.is_some_and(|v| approx_rel(v, 102_200_000.0))
        && r.trade_count == 2
        && approx_rel(r.trader_surplus, 32_000_000.0)
        && r.intra_interval_price_dispersion.is_some_and(|v| approx_eq(v, 1.0 / 102.0 * 10_000.0))
        && r.order_to_trade_ratio.is_some_and(|v| approx_eq(v, 2.0));
    check(
        "ts_cda_effective_realized_impact",
        ok,
        format!(
            "eff={:?} rs={:?}/{:?}/{:?} pi={:?}/{:?}/{:?} vol={} notio={} vwap={:?} tc={} surplus={} disp={:?} otr={:?}",
            r.effective_spread_bps,
            r.realized_spread_bps_1s, r.realized_spread_bps_5s, r.realized_spread_bps_30s,
            r.price_impact_bps_1s, r.price_impact_bps_5s, r.price_impact_bps_30s,
            r.executed_volume, r.executed_notional, r.vwap, r.trade_count, r.trader_surplus,
            r.intra_interval_price_dispersion, r.order_to_trade_ratio
        ),
    )
}

/// `realized_volatility` (root-sum-of-squared returns) and
/// `amihud_illiquidity` (|return| per DOLLAR volume, carried across buckets).
fn ts_cda_volatility_and_amihud() -> TestCase {
    let p = |x: u128| x * PRICE_SCALE;
    let mut rec = MetricsRecorder::new(EngineKind::Cda, TS_W);
    rec.set_anchor(0);
    // Bucket 0: three mids -> returns +0.2 and -0.2 ; close p(96).
    rec.record_book_snapshot(tbook_px(1_000_000, p(100)));
    rec.record_book_snapshot(tbook_px(2_000_000, p(120)));
    rec.record_book_snapshot(tbook_px(3_000_000, p(96)));
    // Bucket 1: close p(120) and a trade of 5 @ p(200) -> dollar volume 1000.
    rec.record_book_snapshot(tbook_px(1_500_000_000, p(120)));
    rec.record_trade(tcda_trade(1_001_000_000, p(200), 5, 900, 901, None, None));
    let series = rec.finish(&[], 1_500_000_000);
    if series.len() != 2 {
        return check("ts_cda_volatility_and_amihud", false, format!("expected 2 rows, got {}", series.len()));
    }
    // row 0: rv = sqrt(0.2^2 + 0.2^2) = sqrt(0.08) ; amihud None (no prev close)
    // row 1: ret (p120 from p96) = 0.25 ; dollar vol = 5*200 = 1000
    //        amihud = 0.25 / 1000 * 1e6 = 250 (ILLIQ x 1e6)
    let ok = series[0].realized_volatility.is_some_and(|v| approx_eq(v, 0.08_f64.sqrt()))
        && series[0].amihud_illiquidity.is_none()
        && series[1].amihud_illiquidity.is_some_and(|v| approx_eq(v, 250.0))
        && series[1].executed_volume == 5.0
        && series[1].trade_count == 1;
    check(
        "ts_cda_volatility_and_amihud",
        ok,
        format!(
            "rv0={:?} amihud0={:?} amihud1={:?} vol1={} tc1={}",
            series[0].realized_volatility, series[0].amihud_illiquidity, series[1].amihud_illiquidity,
            series[1].executed_volume, series[1].trade_count
        ),
    )
}

/// Allocation columns bucketed by each order's own submission time:
/// `fill_rate`, `avg_time_to_execution_secs`, `order_size_inflation`.
fn ts_cda_execution_allocation() -> TestCase {
    let mut rec = MetricsRecorder::new(EngineKind::Cda, TS_WIDE);
    rec.set_anchor(0);
    let messages = vec![
        tmsg(0, 1, "Whale", Side::Buy, Some(100), 10),
        tmsg(0, 2, "Whale", Side::Buy, Some(100), 10), // never fills
        tmsg(1_000_000, 3, "Minnow", Side::Buy, Some(100), 4),
    ];
    rec.record_trade(tcda_trade(500_000_000, 100, 6, 1, 900, None, None)); // fills oid 1: 6 of 10
    rec.record_trade(tcda_trade(1_500_000_000, 100, 4, 3, 901, None, None)); // fills oid 3: 4 of 4
    let series = rec.finish(&messages, 1_500_000_000);
    if series.len() != 1 {
        return check("ts_cda_execution_allocation", false, format!("expected 1 row, got {}", series.len()));
    }
    let r = &series[0];
    // fill_rate = (6 + 0 + 4) / (10 + 10 + 4) = 10/24
    // ttf: oid1 0.5 s ; oid3 (1_500_000_000 - 1_000_000)/1e9 = 1.499 s ; mean 0.9995
    // inflation: Whale 20/6 ; Minnow 4/4 = 1 ; mean (20/6 + 1)/2
    // order_to_trade_ratio = 3 msgs / 2 trades = 1.5
    let ok = r.fill_rate.is_some_and(|v| approx_eq(v, 10.0 / 24.0))
        && r.avg_time_to_execution_secs.is_some_and(|v| approx_eq(v, 0.9995))
        && r.order_size_inflation.is_some_and(|v| approx_eq(v, (20.0 / 6.0 + 1.0) / 2.0))
        && r.order_to_trade_ratio.is_some_and(|v| approx_eq(v, 1.5));
    check(
        "ts_cda_execution_allocation",
        ok,
        format!(
            "fill_rate={:?} ttf={:?} inflation={:?} otr={:?}",
            r.fill_rate, r.avg_time_to_execution_secs, r.order_size_inflation, r.order_to_trade_ratio
        ),
    )
}

/// CDA `kyle_lambda`: OLS-through-origin slope of the +5 s relative mid move
/// (bps) on signed sweep flow (SOL). Mirrors the `timeseries.rs` unit test.
fn ts_cda_kyle_lambda_known_slope() -> TestCase {
    let pre = 1_000_000u128;
    let lambda = 2.0;
    let post = |x: i128| -> u128 { (pre as f64 * (1.0 + lambda * x as f64 / 10_000.0)).round() as u128 };
    let mut rec = MetricsRecorder::new(EngineKind::Cda, 10 * TS_W);
    rec.set_anchor(0);
    // Obs 1: net +5.
    rec.record_trade(tcda_trade(1_000_000, pre, 5, 1, 2, Some(pre), Some(Side::Buy)));
    rec.record_book_snapshot(tbook_px(1_000_000 + 5 * TS_W, post(5)));
    // Obs 2: net -3.
    rec.record_trade(tcda_trade(2_000_000, pre, 3, 3, 4, Some(pre), Some(Side::Sell)));
    rec.record_book_snapshot(tbook_px(2_000_000 + 5 * TS_W, post(-3)));
    // Obs 3: same-ts sweep, net +5.
    rec.record_trade(tcda_trade(3_000_000, pre, 2, 5, 6, Some(pre), Some(Side::Buy)));
    rec.record_trade(tcda_trade(3_000_000, pre, 3, 7, 8, Some(pre), Some(Side::Buy)));
    rec.record_book_snapshot(tbook_px(3_000_000 + 5 * TS_W, post(5)));
    // Obs 4: net-zero sweep, inert.
    rec.record_trade(tcda_trade(4_000_000, pre, 3, 9, 10, Some(pre), Some(Side::Buy)));
    rec.record_trade(tcda_trade(4_000_000, pre, 3, 11, 12, Some(pre), Some(Side::Sell)));
    let series = rec.finish(&[], 3_000_000 + 5 * TS_W);
    if series.len() != 1 {
        return check("ts_cda_kyle_lambda_known_slope", false, format!("expected 1 row, got {}", series.len()));
    }
    // Sxx = 25 + 9 + 25 = 59 ; Sxy = 50 + 18 + 50 = 118 ; lambda = 2.0
    let ok = series[0].kyle_lambda.is_some_and(|v| (v - 2.0).abs() < 1e-9) && series[0].pricing_error_bps.is_none();
    check(
        "ts_cda_kyle_lambda_known_slope",
        ok,
        format!("kyle_lambda={:?} pricing_error_bps={:?}", series[0].kyle_lambda, series[0].pricing_error_bps),
    )
}

/// FBA `BatchClearedEvent`-derived columns: implied quoted spread, depth,
/// depth bands, residual share, boundary concentration, latency, throughput.
fn ts_fba_batch_liquidity_metrics() -> TestCase {
    let mut rec = MetricsRecorder::new(EngineKind::Fba, TS_W);
    rec.set_anchor(0);
    let messages: Vec<OrderMessage> = [0u64, 50, 95, 100, 150, 195, 200]
        .iter()
        .enumerate()
        .map(|(i, &ts)| tmsg(ts, i as u64 + 1, "m", Side::Buy, None, 1))
        .collect();
    // B1: window [0,100], cleared 100, demand/supply 30/20, unexec 10, book 98/103.
    rec.record_batch(tbatch(100, 0, Some(100), 30, 20, 0.0, 10, Some(98), Some(103), [(4, 6), (10, 14), (30, 20)], 600));
    // B2: window [100,200], cleared 100, demand/supply 10/25, unexec 15, book 99/101.
    rec.record_batch(tbatch(200, 100, Some(100), 10, 25, 0.0, 15, Some(99), Some(101), [(6, 4), (12, 12), (10, 25)], 800));
    let series = rec.finish(&messages, 200);
    if series.len() != 1 {
        return check("ts_fba_batch_liquidity_metrics", false, format!("expected 1 row, got {}", series.len()));
    }
    let r = &series[0];
    // spread: ((103-98)/100 + (101-99)/100)/2 * 1e4 = (500 + 200)/2 = 350
    // depth_at_best: ((30+20)/2 + (10+25)/2)/2 = (25 + 17.5)/2 = 21.25
    // d10 = (5 + 5)/2 = 5 ; d50 = (12 + 12)/2 = 12 ; d100 = (25 + 17.5)/2 = 21.25
    // residual = (10 + 15) / (30 + 25) = 25/55
    // boundary_concentration: B1 {95,100}/{0,50,95,100} + B2 {195,200}/{100,150,195,200}
    //                         = (2 + 2) / (4 + 4) = 0.5
    // latency = (600 + 800)/2 = 700 us ; throughput = 7 msgs / 0.0014 s = 5000
    let ok = r.quoted_spread_bps.is_some_and(|v| approx_eq(v, 350.0))
        && r.depth_at_best.is_some_and(|v| approx_eq(v, 21.25))
        && r.depth_within_bps[0].is_some_and(|v| approx_eq(v, 5.0))
        && r.depth_within_bps[1].is_some_and(|v| approx_eq(v, 12.0))
        && r.depth_within_bps[2].is_some_and(|v| approx_eq(v, 21.25))
        && r.unexecuted_residual_share.is_some_and(|v| approx_eq(v, 25.0 / 55.0))
        && r.boundary_concentration.is_some_and(|v| approx_eq(v, 0.5))
        && r.avg_clearing_latency_micros.is_some_and(|v| approx_eq(v, 700.0))
        && r.throughput_orders_per_sec.is_some_and(|v| approx_eq(v, 5000.0));
    check(
        "ts_fba_batch_liquidity_metrics",
        ok,
        format!(
            "spread={:?} depth={:?} d={:?}/{:?}/{:?} residual={:?} boundary={:?} lat={:?} tput={:?}",
            r.quoted_spread_bps, r.depth_at_best,
            r.depth_within_bps[0], r.depth_within_bps[1], r.depth_within_bps[2],
            r.unexecuted_residual_share, r.boundary_concentration,
            r.avg_clearing_latency_micros, r.throughput_orders_per_sec
        ),
    )
}

/// FBA spread decomposition — exercises the unsigned (`aggressor_side: None`)
/// branch of `deviation_bps` / the realized-spread markout.
fn ts_fba_effective_realized_impact() -> TestCase {
    let mut rec = MetricsRecorder::new(EngineKind::Fba, TS_WIDE);
    rec.set_anchor(0);
    rec.record_trade(tfba_trade(1_000_000, 102, 10, 1, 2, Some(100)));
    // Forward clearing prices at +1 s / +5 s / +30 s.
    rec.record_batch(tbatch_px(1_500_000_000, 0, 100));
    rec.record_batch(tbatch_px(5_500_000_000, 0, 105));
    rec.record_batch(tbatch_px(30_500_000_000, 0, 97));
    let series = rec.finish(&[], 30_500_000_000);
    if series.len() != 1 {
        return check("ts_fba_effective_realized_impact", false, format!("expected 1 row, got {}", series.len()));
    }
    let r = &series[0];
    // unsigned: eff = 2*|102-100|/100 * 1e4 = 400
    // realized_1s (cp 100) = 400 -> impact_1s = 0
    // realized_5s (cp 105) = 2*|102-105|/100 * 1e4 = 600 -> impact_5s = -200
    // realized_30s (cp 97) = 2*|102-97|/100 * 1e4 = 1000 -> impact_30s = -600
    let ok = r.effective_spread_bps.is_some_and(|v| approx_eq(v, 400.0))
        && r.realized_spread_bps_1s.is_some_and(|v| approx_eq(v, 400.0))
        && r.realized_spread_bps_5s.is_some_and(|v| approx_eq(v, 600.0))
        && r.realized_spread_bps_30s.is_some_and(|v| approx_eq(v, 1000.0))
        && r.price_impact_bps_1s.is_some_and(|v| approx_eq(v, 0.0))
        && r.price_impact_bps_5s.is_some_and(|v| approx_eq(v, -200.0))
        && r.price_impact_bps_30s.is_some_and(|v| approx_eq(v, -600.0));
    check(
        "ts_fba_effective_realized_impact",
        ok,
        format!(
            "eff={:?} rs={:?}/{:?}/{:?} pi={:?}/{:?}/{:?}",
            r.effective_spread_bps,
            r.realized_spread_bps_1s, r.realized_spread_bps_5s, r.realized_spread_bps_30s,
            r.price_impact_bps_1s, r.price_impact_bps_5s, r.price_impact_bps_30s
        ),
    )
}

/// FBA `kyle_lambda`: slope of a priced batch's relative clearing-price move
/// (bps) on its own pre-selection net order flow (SOL). Mirrors the
/// `timeseries.rs` unit test; also exercises the unpriced-batch skip.
fn ts_fba_kyle_lambda_net_order_flow() -> TestCase {
    let mut rec = MetricsRecorder::new(EngineKind::Fba, TS_WIDE);
    rec.set_anchor(0);
    // b0 seeds prev_clearing.
    rec.record_batch(tbatch(1_000_000, 0, Some(1_000_000), 0, 0, 0.0, 0, None, None, NO_SCHED, 0));
    // b1: x=8,  y = (1_100_000-1_000_000)/1_000_000 * 1e4 = 1000
    rec.record_batch(tbatch(2_000_000, 0, Some(1_100_000), 0, 0, 8.0, 0, None, None, NO_SCHED, 0));
    // b2: x=-4, y = (1_045_000-1_100_000)/1_100_000 * 1e4 = -500
    rec.record_batch(tbatch(3_000_000, 0, Some(1_045_000), 0, 0, -4.0, 0, None, None, NO_SCHED, 0));
    // b3: unpriced -> inert, does not advance the carry.
    rec.record_batch(tbatch(4_000_000, 0, None, 0, 0, 99.0, 0, None, None, NO_SCHED, 0));
    // b4: x=2,  y = (1_071_125-1_045_000)/1_045_000 * 1e4 = 250
    rec.record_batch(tbatch(5_000_000, 0, Some(1_071_125), 0, 0, 2.0, 0, None, None, NO_SCHED, 0));
    let series = rec.finish(&[], 5_000_000);
    if series.len() != 1 {
        return check("ts_fba_kyle_lambda_net_order_flow", false, format!("expected 1 row, got {}", series.len()));
    }
    // Sxx = 64 + 16 + 4 = 84 ; Sxy = 8000 + 2000 + 500 = 10500 ; lambda = 125.0
    let ok = series[0].kyle_lambda.is_some_and(|v| (v - 125.0).abs() < 1e-6) && series[0].pricing_error_bps.is_none();
    check(
        "ts_fba_kyle_lambda_net_order_flow",
        ok,
        format!("kyle_lambda={:?} pricing_error_bps={:?}", series[0].kyle_lambda, series[0].pricing_error_bps),
    )
}

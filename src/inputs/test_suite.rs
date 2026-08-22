//! Behavioral test suite for both engines, run at runtime from the CLI
//! (`test engine continuous` / `test engine batch`) rather than only via
//! `cargo test`, so the checklist is available without a Rust toolchain in
//! hand. Every case constructs a fresh, isolated orderbook and fully
//! deterministic orders (explicit ids/timestamps, never wall-clock), feeds
//! it a hand-designed scenario, and checks the result against an
//! independently hand-computed expectation — not just "whatever the code
//! currently does".

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::types::{EngineKind, Order, Side, PRICE_SCALE};

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

pub fn print_checklist(engine_label: &str, cases: &[TestCase]) {
    println!("\n==========================================================================");
    println!("🧪                      {engine_label} ENGINE TEST CHECKLIST                      ");
    println!("==========================================================================");

    for c in cases {
        let mark = if c.passed { "✅" } else { "❌" };
        println!("  {mark} {}", c.name);
        if !c.passed {
            println!("      -> {}", c.detail);
        }
    }

    let passed = cases.iter().filter(|c| c.passed).count();
    let total = cases.len();
    println!("--------------------------------------------------------------------------");
    if passed == total {
        println!("  RESULT: {passed}/{total} passed — {engine_label} engine OK ✅");
    } else {
        println!("  RESULT: {passed}/{total} passed — {} case(s) FAILING ❌", total - passed);
    }
    println!("==========================================================================\n");
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

    let ok = trades.is_empty() && book.bids.len() == 1 && book.asks.is_empty() && book.bids[0].remaining == 10;
    check(
        "cda_resting_order_no_cross",
        ok,
        format!("trades={} bids={} asks={}", trades.len(), book.bids.len(), book.asks.len()),
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
        && book.bids.is_empty()
        && book.asks.is_empty();
    check(
        "cda_simple_cross_exact_qty",
        ok,
        format!("trades={trades:?} bids_left={} asks_left={}", book.bids.len(), book.asks.len()),
    )
}

fn cda_partial_fill_resting_larger() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Bob", Side::Sell, 100, 10, 1)); // rests, qty 10
    let trades = book.submit(limit(2, "Alice", Side::Buy, 100, 4, 2)); // smaller taker

    let ok = trades.len() == 1
        && trades[0].quantity == 4
        && book.bids.is_empty() // taker fully filled, nothing rests
        && book.asks.len() == 1
        && book.asks[0].remaining == 6; // maker partially filled, stays resting
    check(
        "cda_partial_fill_resting_larger",
        ok,
        format!("trades={trades:?} ask_remaining={:?}", book.asks.first().map(|o| o.remaining)),
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
        && book.asks.is_empty()
        && book.bids.len() == 1
        && book.bids[0].remaining == 3 // 10 - 3 - 4
        && book.bids[0].limit_px == 101;
    check(
        "cda_multi_fill_walks_book",
        ok,
        format!("trades={trades:?} bids={:?}", book.bids.iter().map(|o| (o.user_id.clone(), o.remaining)).collect::<Vec<_>>()),
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
        && book.asks.len() == 1
        && book.asks[0].user_id == "S_worse";
    check(
        "cda_price_priority_over_time",
        ok,
        format!("trades={trades:?} remaining_asks={:?}", book.asks.iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_time_priority_same_price() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "B_early", Side::Buy, 100, 5, 1));
    book.submit(limit(2, "B_late", Side::Buy, 100, 5, 2));
    let trades = book.submit(limit(3, "Seller", Side::Sell, 100, 5, 3));

    let ok = trades.len() == 1 && trades[0].buyer_id == "B_early" && book.bids.len() == 1 && book.bids[0].user_id == "B_late";
    check(
        "cda_time_priority_same_price",
        ok,
        format!("trades={trades:?} remaining_bids={:?}", book.bids.iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_market_crosses_at_maker_price() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Seller", Side::Sell, 100, 10, 1)); // resting limit
    let trades = book.submit(market(2, "Buyer", Side::Buy, 10, 2)); // market order, no price of its own

    let ok = trades.len() == 1 && trades[0].price == 100 && trades[0].quantity == 10 && book.asks.is_empty() && book.bids.is_empty();
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
    let ok = trades.is_empty() && book.bids.is_empty() && book.asks.is_empty() && book.fill_rate() == Some(0.0);
    check(
        "cda_market_no_liquidity_no_rest",
        ok,
        format!("trades={} bids={} asks={} fill_rate={:?}", trades.len(), book.bids.len(), book.asks.len(), book.fill_rate()),
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

    let ok = trades.is_empty() && book.bids.is_empty() && book.asks.is_empty();
    check("cda_non_live_order_filtered", ok, format!("trades={} bids={} asks={}", trades.len(), book.bids.len(), book.asks.len()))
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
    let passive_ok = passive_trades.is_empty() && book.asks.len() == 1 && book.asks[0].user_id == "PassiveSeller";

    let ok = aggressive_ok && passive_ok;
    check(
        "cda_sell_crosses_bid_only_at_or_below_bid_price",
        ok,
        format!("aggressive_trades={aggressive_trades:?} passive_trades={passive_trades:?} asks={:?}", book.asks.iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_cancellation_removes_resting_order() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests, no counterparty
    book.submit(limit(2, "Bob", Side::Sell, 105, 5, 2)); // rests too, doesn't cross
    book.submit(cancel_event(1, 3)); // cancel Alice's resting bid by oid

    let ok = book.bids.is_empty() && book.asks.len() == 1 && book.asks[0].user_id == "Bob";
    check(
        "cda_cancellation_removes_resting_order",
        ok,
        format!("bids={} asks={:?}", book.bids.len(), book.asks.iter().map(|o| o.user_id.clone()).collect::<Vec<_>>()),
    )
}

fn cda_cancellation_of_unknown_oid_is_harmless() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests

    // Cancel an oid that was never submitted as live.
    let removed = book.cancel(999);

    let ok = !removed && book.bids.len() == 1 && book.bids[0].oid == 1;
    check("cda_cancellation_of_unknown_oid_is_harmless", ok, format!("removed={removed} bids={}", book.bids.len()))
}

fn cda_filled_status_does_not_touch_resting_order() -> TestCase {
    let mut book = CdaOrderBook::new();
    book.submit(limit(1, "Alice", Side::Buy, 100, 10, 1)); // rests
    book.submit(filled_event(1, 2)); // an external "filled" event for the same oid

    // Per the design decision, fills are NOT replayed — Alice's order must
    // still be sitting there completely untouched.
    let ok = book.bids.len() == 1 && book.bids[0].oid == 1 && book.bids[0].remaining == 10;
    check(
        "cda_filled_status_does_not_touch_resting_order",
        ok,
        format!("bids={:?}", book.bids.iter().map(|o| (o.oid, o.remaining)).collect::<Vec<_>>()),
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

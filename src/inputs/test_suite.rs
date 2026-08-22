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

/// A canceled order (status_id = 2) — should never enter either engine.
fn non_live(oid: u64, user: &str, side: Side, price: u128, qty: u128, ts: u64) -> Order {
    let mut o = Order::limit(oid, user, side, price, qty, ts);
    o.status_id = 2;
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
        cda_non_live_order_filtered(),
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

    // Market orders never rest, even unfilled.
    let ok = trades.is_empty() && book.bids.is_empty() && book.asks.is_empty();
    check("cda_market_no_liquidity_no_rest", ok, format!("trades={} bids={} asks={}", trades.len(), book.bids.len(), book.asks.len()))
}

fn cda_non_live_order_filtered() -> TestCase {
    let mut book = CdaOrderBook::new();
    let trades = book.submit(non_live(1, "X", Side::Buy, 100, 10, 1));

    let ok = trades.is_empty() && book.bids.is_empty() && book.asks.is_empty();
    check("cda_non_live_order_filtered", ok, format!("trades={} bids={} asks={}", trades.len(), book.bids.len(), book.asks.len()))
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
        fba_all_market_with_history_anchors(),
        fba_non_live_order_filtered(),
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

fn fba_all_market_with_history_anchors() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(limit(1, "A", Side::Buy, 100, 5, 1));
    book.submit(limit(2, "B", Side::Sell, 100, 5, 1));
    book.clear(); // establishes last_clearing_price = Some(100)

    book.submit(market(3, "C", Side::Buy, 8, 2));
    book.submit(market(4, "D", Side::Sell, 8, 2));
    let Some(result) = book.clear() else {
        return check("fba_all_market_with_history_anchors", false, "clear() returned None, expected it to anchor on last_clearing_price");
    };

    let ok = result.clearing_price == 100 && result.traded_quantity == 8 && result.trades.len() == 1;
    check(
        "fba_all_market_with_history_anchors",
        ok,
        format!("price={} qty={} trades={}", result.clearing_price, result.traded_quantity, result.trades.len()),
    )
}

fn fba_non_live_order_filtered() -> TestCase {
    let mut book = FbaOrderBook::new();
    book.submit(non_live(1, "X", Side::Buy, 100, 10, 1));

    let ok = book.pending_orders.is_empty() && book.clear().is_none();
    check("fba_non_live_order_filtered", ok, format!("pending={}", book.pending_orders.len()))
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

//! Continuous Double Auction: every order is matched (or rested) the
//! instant it arrives, against a live resting book of bids/asks.
// Orderbook struct carries the live resting book, executed trades,
// and metrics so that module metrics can calculate the metrics at running time on each snapshot.

use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::types::{Amount, EngineKind, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

/// CDA's own orderbook: the live resting bids/asks, this engine's full
/// trade history, and the running state its own metric methods read from.
pub struct CdaOrderBook {
    // Resting orders, keyed by price, each level a FIFO queue ordered by
    // `(ts, oid)` (see `arrival_key`/`insert_sorted`). Both sides are
    // ascending `BTreeMap`s — "best" differs only in which END you read:
    // asks' best is the LOWEST price (`first_key_value`), bids' best is
    // the HIGHEST price (`last_key_value`).
    //
    // Private: this used to be a flat `pub bids: Vec<Order>`/`pub asks:
    // Vec<Order>`, sorted best-first and read directly by callers. That
    // made `submit`'s resting-insert/full-fill-removal O(book size) every
    // time (Vec::insert/remove shift the WHOLE side), even though the
    // insertion *position* was found in O(log n) — the shift dominated at
    // real-data scale, where the book can accumulate huge numbers of
    // resting orders over a trading day. Per-price-level storage turns
    // insert/remove into O(log L) (L = distinct price levels) + O(level
    // depth) (how many orders share exactly one price — typically far
    // smaller than the whole book). External read access now goes through
    // `best_bid_order`/`bids_iter`/`bid_count`/etc. below instead of
    // direct field access.
    //
    // Invariant, must hold after every mutation: a price key exists here
    // IFF its `VecDeque` is non-empty — no dangling empty levels.
    bids: BTreeMap<Price, VecDeque<Order>>,
    asks: BTreeMap<Price, VecDeque<Order>>,

    // oid -> (side, price) for every currently-resting order, so `cancel`
    // can find an order's price level in O(log L) instead of scanning the
    // whole book for it (the book is sorted by PRICE, not oid, so there's
    // no way to binary-search for a given oid without this). Kept in sync
    // at exactly 3 points: resting-insert (`submit`), full-fill removal
    // (`submit`'s matching loop), and explicit `cancel`.
    oid_index: HashMap<u64, (Side, Price)>,

    pub executed_trades: Vec<Trade>,
    next_trade_id: u64,

    // O(1) running counters, mirroring `bid_depth`/`ask_depth` below —
    // updated at the same 3 sync points as `oid_index`.
    bid_order_count: usize,
    ask_order_count: usize,

    // ---- Bookkeeping for fill_rate() = total_filled_qty / total_submitted_qty ----
    //
    // total_submitted_qty: every live order that ever entered this book
    // adds its OWN quantity here, regardless of side. A buy for 6 and a
    // sell for 6 add 6 + 6 = 12 to this total, not 6 — each order's
    // demand/supply is counted independently, the same way a trader would
    // count "how much volume has been thrown at this book in total."
    //
    // total_filled_qty: every time a trade of quantity `q` executes, it
    // fills `q` worth of the buy order's demand AND `q` worth of the sell
    // order's supply — two separate quantities got satisfied, matching
    // the two separate quantities total_submitted_qty counted for them.
    // So each trade adds `q` twice here (`fill_qty * 2` at the call
    // sites), once per side, to stay on the same footing as
    // total_submitted_qty. If it only added `q` once, a book where
    // *everything* submitted eventually got fully matched would still
    // only reach fill_rate = 0.5, not 1.0.
    //
    // Worked example (see `cda_market_order_partial_liquidity_fill_rate`
    // in inputs/test_suite.rs for this as a real, running test):
    //   1. Jesus submits: sell 6 SOL @130 (limit)         total_submitted_qty:  0 -> 6
    //      Nothing resting to match against -> it just rests, unfilled so far.
    //   2. Alejandro submits:   buy  6 SOL (market order)        total_submitted_qty:  6 -> 12
    //      Alejandro's order fully matches Jesus's resting sell -> ONE trade, qty 6.
    //      That trade filled 6 of Jesus's supply AND 6 of Alejandro's demand:
    //                                                       total_filled_qty:    0 -> 6 (Jesus's side) -> 12 (Alejandro's side)
    //   3. Alvaro submits:   buy  3 SOL (market order)        total_submitted_qty: 12 -> 15
    //      Jesus's ask is gone, nothing left to match -> Alvaro's order finds
    //      NO liquidity. Market orders never rest (see `submit` below), so
    //      it doesn't sit on the book either — it just vanishes.
    //                                                       total_filled_qty:    stays 12
    //
    //   fill_rate = total_filled_qty / total_submitted_qty = 12 / 15 = 0.8
    //   — correctly says Jesus and Alejandro's orders fully filled, but Alvaro's 3
    //   units never traded.
    //
    // Why not derive this instead of tracking it separately? The tempting
    // shortcut is `filled = total_submitted_qty - still_resting_qty`
    // (`depth_at_best()`). That's exactly what `FbaOrderBook` does, and
    // it's correct there because nothing in FBA ever disappears without
    // either matching or staying queued for the next batch. It's WRONG
    // here: in step 3 above, Alvaro's order never rests, so it would vanish
    // from both sides of that subtraction — not counted as "still
    // resting," but also never actually filled — silently inflating
    // fill_rate to 12/12 = 1.0 instead of the correct 0.8.
    // Tracking total_filled_qty directly, incremented at the
    // moment each trade happens, avoids that trap entirely.

    total_submitted_qty: Amount,
    total_filled_qty: Amount,

    // Running totals of resting quantity on each side, updated by exactly
    // ±qty at the moment an order rests / fills / gets canceled — O(1) per
    // event, instead of resumming the whole side from scratch.
    bid_depth: Amount,
    ask_depth: Amount,
}

/// Picks the price a trade executes at. The standard continuous-matching
/// convention: whoever posted liquidity first sets the price, and an
/// incoming order's own price only ever decides whether it's eligible to
/// cross — never what it pays. This is also
/// what makes CDA's pricing different from FBA's, here every trade can
/// print at a different price (whatever each maker posted), where FBA
/// forces every trade in a batch to the same uniform clearing price.
///
/// `OrderKind::Limit { price } => *price` — correct and the only path
/// that's actually reachable: the maker's own posted price.
///
/// `OrderKind::Market => PRICE_SCALE` — this branch exists only so the
/// match is exhaustive; it can never actually run. A market order is
/// never rested (see `submit` below: only `OrderKind::Limit` orders ever
/// get pushed into `self.bids`/`self.asks`), so `best_ask`/`best_bid` —
/// and therefore whatever gets passed into this function — can never BE a
/// `Market` order. `PRICE_SCALE` here isn't a real price (it's the raw
/// fixed-point value for "1.000000", with no relation to where the
/// market is actually trading); it's a placeholder that happens to type-
/// check. If some future change ever let a market order rest, this
/// function would silently start returning that meaningless placeholder
/// as a real trade price instead of failing loudly — worth hardening
/// (e.g. `unreachable!()`) if that invariant ever becomes less obviously
/// true than it is today.

fn get_price(kind: &OrderKind) -> Price {
    match kind {
        OrderKind::Limit { price } => *price,
        OrderKind::Market => PRICE_SCALE,
    }
}

/// Whether an incoming order (/taker) can cross a resting order (maker) at
/// the maker's price. A market order on either side always crosses. The
/// crossing direction depends on which side the taker is on: a buy
/// crosses a resting ask when it's willing to pay at least the ask's
/// price (`buy_px >= ask_px`); a sell crosses a resting bid when it's
/// willing to accept at most the bid's price (`sell_px <= bid_px`) — the
/// OPPOSITE comparison, so `taker_side` can't be dropped in favor of a
/// single symmetric check.
fn check_price_match(taker_kind: &OrderKind, maker_kind: &OrderKind, taker_side: Side) -> bool {
    match (taker_kind, maker_kind) {
        (OrderKind::Market, _) | (_, OrderKind::Market) => true,
        (OrderKind::Limit { price: taker_px }, OrderKind::Limit { price: maker_px }) => match taker_side {
            Side::Buy => *taker_px >= *maker_px,
            Side::Sell => *taker_px <= *maker_px,
        },
    }
}

/// The only ordering needed WITHIN one price level, now that price itself
/// is handled structurally by the map key — `(ts, oid)` ascending, same
/// tie-break the old flat-Vec `bid_sort_key`/`ask_sort_key` encoded
/// alongside price.
fn arrival_key(o: &Order) -> (u64, u64) {
    (o.ts, o.oid)
}

/// Push `order` into `level`, keeping it ordered by `arrival_key`
/// ascending.
///
/// Fast path: real order flow arrives close to time-ordered, so appending
/// to the back is already correct almost always — O(1).
///
/// Fallback (only when that's not true — e.g. two orders at the same
/// price with slightly out-of-order timestamps): `make_contiguous` then
/// binary-search-insert — O(level depth), bounded by how many orders
/// share exactly this one price, not the whole book.
fn insert_sorted(level: &mut VecDeque<Order>, order: Order) {
    let key = arrival_key(&order);
    if level.back().is_none_or(|last| arrival_key(last) <= key) {
        level.push_back(order);
    } else {
        let idx = level.make_contiguous().partition_point(|o| arrival_key(o) < key);
        level.insert(idx, order);
    }
}

impl CdaOrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            oid_index: HashMap::new(),
            executed_trades: Vec::new(),
            next_trade_id: 1,
            bid_order_count: 0,
            ask_order_count: 0,
            total_submitted_qty: 0,
            total_filled_qty: 0,
            bid_depth: 0,
            ask_depth: 0,
        }
    }

    /// Match the incoming order against the resting book (price-time
    /// priority), then rest whatever's left of a limit order. Returns the
    /// trades this single order produced (also appended to
    /// `self.executed_trades`).
    pub fn submit(&mut self, mut order: Order) -> Vec<Trade> {
        // A cancellation removes a still-resting order by oid (see
        // `Order::is_cancellation` but `filled` events are
        // deliberately NOT handled the same way — this simulation's CDA
        // matching decides fills independently of whatever Hyperliquid's
        // own engine did).
        if order.is_cancellation() {
            self.cancel(order.oid);
            return Vec::new();
        }

        // Rejections and un-triggered conditional orders never touch the book.
        if !order.is_new_live_order() || order.remaining == 0 {
            return Vec::new();
        }

        self.total_submitted_qty = self.total_submitted_qty.saturating_add(order.remaining);

        // Trades are pushed straight into `self.executed_trades` as they
        // happen, and the return value is sliced off the end — avoids
        // building a separate local `Vec<Trade>` and then `.clone()`-ing
        // the whole thing into `executed_trades` (each `Trade` owns 2
        // `String`s, so that used to double-allocate every single submit
        // that produced a fill).
        let trades_start = self.executed_trades.len();

        match order.side() {
            Side::Buy => {
                //loop until ask side is empty or order is fully filled unless no match then stop
                while order.remaining > 0 {
                    let Some(mut entry) = self.asks.first_entry() else { break }; // O(log L): best (lowest-priced) ask level
                    let level = entry.get_mut();
                    let best_ask = level.front_mut().expect("level invariant: never empty while keyed");

                    if !check_price_match(&order.kind(), &best_ask.kind(), Side::Buy) {
                        break;
                    } // if best ask and the incoming buy order do not match, break the loop

                    let execution_price = get_price(&best_ask.kind());
                    let fill_qty = order.remaining.min(best_ask.remaining);
                    if fill_qty == 0 {
                        break;
                    }

                    order.reduce(fill_qty);
                    best_ask.reduce(fill_qty);
                    let ask_fully_filled = best_ask.remaining == 0;
                    let ask_oid = best_ask.oid;
                    let ask_user_id = best_ask.user_id.clone();

                    self.ask_depth = self.ask_depth.saturating_sub(fill_qty);
                    self.total_filled_qty = self.total_filled_qty.saturating_add(fill_qty * 2);

                    self.executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: order.user_id.clone(),
                        seller_id: ask_user_id,
                        buy_order_id: order.oid,
                        sell_order_id: ask_oid,
                        engine_type: EngineKind::Cda,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if ask_fully_filled {
                        level.pop_front(); // O(1) — was `self.asks.remove(0)`, an O(book size) shift of the WHOLE side
                        self.oid_index.remove(&ask_oid);
                        self.ask_order_count -= 1;
                        if level.is_empty() {
                            entry.remove(); // O(log L) — keep the "no dangling empty level" invariant
                        }
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    let price = order.limit_price().expect("just matched Limit above");
                    self.bid_depth = self.bid_depth.saturating_add(order.remaining);
                    self.bid_order_count += 1;
                    self.oid_index.insert(order.oid, (Side::Buy, price));
                    let level = self.bids.entry(price).or_default(); // O(log L)
                    insert_sorted(level, order); // O(1) fast path, O(level depth) fallback
                }
            }
            Side::Sell => {
                while order.remaining > 0 {
                    let Some(mut entry) = self.bids.last_entry() else { break }; // O(log L): best (highest-priced) bid level
                    let level = entry.get_mut();
                    let best_bid = level.front_mut().expect("level invariant: never empty while keyed");

                    if !check_price_match(&order.kind(), &best_bid.kind(), Side::Sell) {
                        break;
                    }

                    let execution_price = get_price(&best_bid.kind());
                    let fill_qty = order.remaining.min(best_bid.remaining);
                    if fill_qty == 0 {
                        break;
                    }

                    order.reduce(fill_qty);
                    best_bid.reduce(fill_qty);
                    let bid_fully_filled = best_bid.remaining == 0;
                    let bid_oid = best_bid.oid;
                    let bid_user_id = best_bid.user_id.clone();

                    self.bid_depth = self.bid_depth.saturating_sub(fill_qty);
                    self.total_filled_qty = self.total_filled_qty.saturating_add(fill_qty * 2);

                    self.executed_trades.push(Trade {
                        trade_id: self.next_trade_id,
                        price: execution_price,
                        quantity: fill_qty,
                        buyer_id: bid_user_id,
                        seller_id: order.user_id.clone(),
                        buy_order_id: bid_oid,
                        sell_order_id: order.oid,
                        engine_type: EngineKind::Cda,
                        ts: order.ts,
                        trade_tx_hash: None,
                        chain_id: None,
                    });
                    self.next_trade_id += 1;

                    if bid_fully_filled {
                        level.pop_front(); // O(1) — was `self.bids.remove(0)`
                        self.oid_index.remove(&bid_oid);
                        self.bid_order_count -= 1;
                        if level.is_empty() {
                            entry.remove();
                        }
                    }
                }

                if order.remaining > 0 && matches!(order.kind(), OrderKind::Limit { .. }) {
                    let price = order.limit_price().expect("just matched Limit above");
                    self.ask_depth = self.ask_depth.saturating_add(order.remaining);
                    self.ask_order_count += 1;
                    self.oid_index.insert(order.oid, (Side::Sell, price));
                    let level = self.asks.entry(price).or_default();
                    insert_sorted(level, order);
                }
            }
        }

        self.executed_trades[trades_start..].to_vec()
    }

    /// Removes a still-resting order by `oid`, if present. Returns whether
    /// anything was actually removed — a cancel for an `oid` this book
    /// never saw as live, or that already matched away, is a harmless
    /// no-op.
    ///
    /// `oid_index` gives the order's side+price directly (O(1)), so this
    /// no longer needs to guess which side to scan or scan the whole book
    /// at all to find it — only O(log L) to reach the right level, plus
    /// O(level depth) to remove from within it (no O(1) arbitrary-position
    /// removal from a `VecDeque` without a heavier tombstone scheme, which
    /// isn't worth the added complexity here: this still eliminates the
    /// real cost center, the O(book size) full scan + shift).
    pub fn cancel(&mut self, oid: u64) -> bool {
        let Some(&(side, price)) = self.oid_index.get(&oid) else { return false };
        let book = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        // Defensive, not expected: `oid_index` should always stay in sync
        // with the book (see the 3 sync points documented on the field),
        // so a miss here would mean a bug elsewhere, not a legitimate
        // "wasn't there" case — treat it as a no-op rather than panicking.
        let Some(level) = book.get_mut(&price) else {
            self.oid_index.remove(&oid);
            return false;
        };
        let Some(pos) = level.iter().position(|o| o.oid == oid) else {
            self.oid_index.remove(&oid);
            return false;
        };
        let removed = level.remove(pos).expect("position() just found this index");
        if level.is_empty() {
            book.remove(&price);
        }
        self.oid_index.remove(&oid);

        match side {
            Side::Buy => {
                self.bid_depth = self.bid_depth.saturating_sub(removed.remaining);
                self.bid_order_count -= 1;
            }
            Side::Sell => {
                self.ask_depth = self.ask_depth.saturating_sub(removed.remaining);
                self.ask_order_count -= 1;
            }
        }
        true
    }

    // ---- Core metrics, computed on demand from this orderbook's own state ----

    pub fn best_bid(&self) -> Option<Price> {
        self.bids.last_key_value().map(|(&price, _)| price)
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.asks.first_key_value().map(|(&price, _)| price)
    }

    /// The single best-priced resting bid, if any — the order at the very
    /// touch, earliest-arrived at that price.
    pub fn best_bid_order(&self) -> Option<&Order> {
        self.bids.last_key_value().and_then(|(_, level)| level.front())
    }

    /// The single best-priced resting ask, if any.
    pub fn best_ask_order(&self) -> Option<&Order> {
        self.asks.first_key_value().and_then(|(_, level)| level.front())
    }

    /// Number of resting bid orders (not total quantity — see `bid_depth`
    /// for that) — O(1) running counter.
    pub fn bid_count(&self) -> usize {
        self.bid_order_count
    }

    /// Number of resting ask orders — O(1) running counter.
    pub fn ask_count(&self) -> usize {
        self.ask_order_count
    }

    pub fn bids_is_empty(&self) -> bool {
        self.bids.is_empty()
    }

    pub fn asks_is_empty(&self) -> bool {
        self.asks.is_empty()
    }

    /// All resting bids, best (highest price) first, then earliest-arrived
    /// first within a price level. O(book size) to fully consume — same
    /// cost as iterating the old flat `Vec` had, just reached via a
    /// zero-allocation flattening iterator over the price-level map
    /// instead of direct field access.
    pub fn bids_iter(&self) -> impl Iterator<Item = &Order> {
        self.bids.iter().rev().flat_map(|(_, level)| level.iter())
    }

    /// All resting asks, best (lowest price) first, then earliest-arrived
    /// first within a price level.
    pub fn asks_iter(&self) -> impl Iterator<Item = &Order> {
        self.asks.iter().flat_map(|(_, level)| level.iter())
    }

    pub fn trade_count(&self) -> usize {
        self.executed_trades.len()
    }

    pub fn executed_volume(&self) -> Amount {
        self.executed_trades.iter().map(|t| t.quantity).sum()
    }

    pub fn executed_notional(&self) -> Amount {
        self.executed_trades
            .iter()
            .map(|t| t.quantity.saturating_mul(t.price) / PRICE_SCALE)
            .sum()
    }

    pub fn quoted_spread_bps(&self) -> Option<f64> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        let mid = (bid as f64 + ask as f64) / 2.0;
        if mid == 0.0 {
            return None;
        }
        Some(((ask as f64 - bid as f64) / mid) * 10_000.0)
    }

    /// Total resting quantity on the bid side — O(1), a running counter
    /// maintained by `submit`/`cancel` rather than resummed here.
    pub fn bid_depth(&self) -> Amount {
        self.bid_depth
    }

    /// Total resting quantity on the ask side — O(1), same idea as
    /// `bid_depth`.
    pub fn ask_depth(&self) -> Amount {
        self.ask_depth
    }

    /// Total resting volume (bids + asks).
    pub fn depth_at_best(&self) -> Amount {
        self.bid_depth() + self.ask_depth()
    }

    /// `(bid_depth - ask_depth) / (bid_depth + ask_depth)` at the top of
    /// book — a CDA-only signal, since a batch auction has no resting book.
    pub fn book_imbalance(&self) -> Option<f64> {
        let bid = self.bid_depth();
        let ask = self.ask_depth();
        let total = bid + ask;
        if total == 0 {
            return None;
        }
        Some((bid as f64 - ask as f64) / total as f64)
    }

    /// Filled / submitted, across everything ever submitted to this book.
    pub fn fill_rate(&self) -> Option<f64> {
        if self.total_submitted_qty == 0 {
            return None;
        }
        Some(self.total_filled_qty as f64 / self.total_submitted_qty as f64)
    }
}

impl Default for CdaOrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Small deterministic LCG (no `rand` dependency in this crate) — same
    // constants as glibc's `rand()`, good enough for a reproducible fuzz
    // test, not for anything security-sensitive.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn range(&mut self, n: u64) -> u64 {
            if n == 0 { 0 } else { self.next() % n }
        }
    }

    // ---- "Naive" references: recompute directly from the book's own
    // private fields via a full scan, independent of the O(1)/O(log n)
    // accessors under test. Same-file-only (private field access). ----

    fn naive_bid_depth(book: &CdaOrderBook) -> Amount {
        book.bids.values().flat_map(|q| q.iter()).map(|o| o.remaining).sum()
    }
    fn naive_ask_depth(book: &CdaOrderBook) -> Amount {
        book.asks.values().flat_map(|q| q.iter()).map(|o| o.remaining).sum()
    }
    fn naive_bid_count(book: &CdaOrderBook) -> usize {
        book.bids.values().map(|q| q.len()).sum()
    }
    fn naive_ask_count(book: &CdaOrderBook) -> usize {
        book.asks.values().map(|q| q.len()).sum()
    }
    fn naive_best_bid(book: &CdaOrderBook) -> Option<Price> {
        book.bids.keys().next_back().copied()
    }
    fn naive_best_ask(book: &CdaOrderBook) -> Option<Price> {
        book.asks.keys().next().copied()
    }
    fn assert_no_dangling_levels(book: &CdaOrderBook, ctx: &str) {
        assert!(book.bids.values().all(|q| !q.is_empty()), "dangling empty bid level {ctx}");
        assert!(book.asks.values().all(|q| !q.is_empty()), "dangling empty ask level {ctx}");
    }

    #[test]
    fn bid_ask_depth_matches_hand_computed_scenario() {
        let mut book = CdaOrderBook::new();

        // Two resting bids, no crossing asks yet.
        book.submit(Order::limit(1, "a", Side::Buy, 100, 10, 0));
        book.submit(Order::limit(2, "a", Side::Buy, 99, 5, 1));
        assert_eq!(book.bid_depth(), 15);
        assert_eq!(book.ask_depth(), 0);

        // A sell that partially crosses the best bid (100 @ 10), leaving 4
        // resting on the bid side and nothing resting on the ask side.
        book.submit(Order::limit(3, "b", Side::Sell, 100, 6, 2));
        assert_eq!(book.bid_depth(), 9); // 10-6 + 5
        assert_eq!(book.ask_depth(), 0);

        // A resting ask that doesn't cross anything.
        book.submit(Order::limit(4, "b", Side::Sell, 105, 7, 3));
        assert_eq!(book.bid_depth(), 9);
        assert_eq!(book.ask_depth(), 7);

        // Cancel the remaining resting bid (oid 1, now at 4).
        assert!(book.cancel(1));
        assert_eq!(book.bid_depth(), 5); // just oid 2's 5 left
        assert_eq!(book.ask_depth(), 7);

        // Cancel of an oid that never existed / already gone — harmless no-op.
        assert!(!book.cancel(999));
        assert_eq!(book.bid_depth(), 5);
        assert_eq!(book.ask_depth(), 7);

        assert_eq!(book.bid_depth(), naive_bid_depth(&book));
        assert_eq!(book.ask_depth(), naive_ask_depth(&book));
    }

    #[test]
    fn matching_respects_price_then_time_priority_across_and_within_levels() {
        let mut book = CdaOrderBook::new();
        // Two ask levels: 100 (two orders, earliest-first) and 101 (one order).
        book.submit(Order::limit(1, "a1", Side::Sell, 100, 5, 0));
        book.submit(Order::limit(2, "a2", Side::Sell, 100, 5, 1)); // same price, later ts -> FIFO after oid 1
        book.submit(Order::limit(3, "a3", Side::Sell, 101, 5, 2)); // worse price -> after both 100-level orders

        // A buy market order sweeping 12 units should match: 5 from oid1, 5 from oid2, 2 from oid3.
        let trades = book.submit(Order::market(4, "buyer", Side::Buy, 12, 3));
        assert_eq!(trades.len(), 3, "trades={trades:?}");
        assert_eq!((trades[0].sell_order_id, trades[0].quantity), (1, 5));
        assert_eq!((trades[1].sell_order_id, trades[1].quantity), (2, 5));
        assert_eq!((trades[2].sell_order_id, trades[2].quantity), (3, 2));
        assert_no_dangling_levels(&book, "after sweep");
    }

    #[test]
    fn out_of_order_arrival_at_same_price_still_sorts_by_ts_not_submission_order() {
        // Exercises `insert_sorted`'s fallback (binary-search-insert) path:
        // the second order submitted has an EARLIER ts than the first, at
        // the same price, so a naive "just push_back on arrival" would get
        // fill order wrong.
        let mut book = CdaOrderBook::new();
        book.submit(Order::limit(1, "later", Side::Sell, 100, 5, 100));
        book.submit(Order::limit(2, "earlier", Side::Sell, 100, 5, 50));

        let trades = book.submit(Order::market(3, "buyer", Side::Buy, 5, 200));
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].sell_order_id, 2, "the ts=50 order should fill first despite arriving second: trades={trades:?}");
    }

    #[test]
    fn cancel_of_an_oid_that_already_fully_filled_is_a_harmless_no_op() {
        let mut book = CdaOrderBook::new();
        book.submit(Order::limit(1, "s", Side::Sell, 100, 5, 0));
        let trades = book.submit(Order::market(2, "b", Side::Buy, 5, 1));
        assert_eq!(trades.len(), 1); // oid 1 fully filled -> already removed from oid_index by submit() itself

        assert!(!book.cancel(1), "must be false, not panic, for an oid that already fully filled");
        assert_eq!(book.ask_depth(), 0);
        assert_eq!(book.bid_depth(), 0);
    }

    #[test]
    fn fully_filled_or_canceled_price_level_leaves_no_dangling_entry() {
        let mut book = CdaOrderBook::new();
        book.submit(Order::limit(1, "s", Side::Sell, 100, 5, 0));
        assert!(!book.asks_is_empty());
        book.submit(Order::market(2, "b", Side::Buy, 5, 1)); // fully consumes the only order at 100
        assert!(book.asks_is_empty());
        assert_eq!(book.asks.len(), 0, "no dangling empty level should remain after a full fill");

        book.submit(Order::limit(3, "s2", Side::Buy, 90, 4, 2));
        assert!(!book.bids_is_empty());
        assert!(book.cancel(3));
        assert!(book.bids_is_empty());
        assert_eq!(book.bids.len(), 0, "no dangling empty level should remain after a cancel");
    }

    /// Differential test: the O(1)/O(log n) accessors must agree exactly
    /// with a full-scan recomputation from the book's own actual state
    /// after every single operation in a long, randomized sequence of
    /// submits/cancels — the semantics `bid_depth()`/`ask_depth()`/
    /// `bid_count()`/`ask_count()`/`best_bid()`/`best_ask()` are all
    /// supposed to expose, just computed two different ways. Also asserts
    /// the "no dangling empty level" invariant holds at every step.
    #[test]
    fn accessors_match_naive_scan_after_random_submit_cancel_sequence() {
        let mut rng = Lcg(0xB00C_5EED);
        let mut book = CdaOrderBook::new();
        let mut live_oids: Vec<u64> = Vec::new();
        let mut next_oid = 1u64;

        for step in 0..500u64 {
            let action = rng.range(4);
            if action < 3 || live_oids.is_empty() {
                // Submit a new order (limit most of the time, occasional market).
                let side = if rng.range(2) == 0 { Side::Buy } else { Side::Sell };
                let qty = (rng.range(20) + 1) as Amount;
                let oid = next_oid;
                next_oid += 1;
                if rng.range(6) == 0 {
                    book.submit(Order::market(oid, "u", side, qty, step));
                } else {
                    // Narrow price range + a tick-jittered ts (occasionally
                    // going backwards) deliberately makes same-price,
                    // out-of-order arrivals common, to actually exercise
                    // `insert_sorted`'s fallback path, not just the fast one.
                    let price = (rng.range(10) + 80) as Price; // 80..90
                    let ts = step.saturating_sub(rng.range(5)); // occasional small backward jitter
                    book.submit(Order::limit(oid, "u", side, price, qty, ts));
                    live_oids.push(oid); // may or may not still be resting, cancel() below is a no-op either way
                }
            } else {
                // Cancel a random previously-submitted (possibly already
                // filled/canceled) order — exercises both the found and
                // not-found paths of `cancel`.
                let idx = rng.range(live_oids.len() as u64) as usize;
                let oid = live_oids.remove(idx);
                book.cancel(oid);
            }

            assert_eq!(book.bid_depth(), naive_bid_depth(&book), "bid_depth mismatch at step {step}");
            assert_eq!(book.ask_depth(), naive_ask_depth(&book), "ask_depth mismatch at step {step}");
            assert_eq!(book.bid_count(), naive_bid_count(&book), "bid_count mismatch at step {step}");
            assert_eq!(book.ask_count(), naive_ask_count(&book), "ask_count mismatch at step {step}");
            assert_eq!(book.best_bid(), naive_best_bid(&book), "best_bid mismatch at step {step}");
            assert_eq!(book.best_ask(), naive_best_ask(&book), "best_ask mismatch at step {step}");
            assert_no_dangling_levels(&book, &format!("at step {step}"));
        }
    }
}

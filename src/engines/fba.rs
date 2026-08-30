//! Frequent Batch Auction engine collects orders into `pending_orders` and clears
//! them all at once at a single uniform price, chosen to maximize matched
//! volume.  See ENGINE_DESIGN.md for a good description of the FBA algorithm and its
//! implementation details.

use std::collections::{BTreeSet, HashMap};

use crate::types::{Amount, EngineKind, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

/// Output of a single batch clearing.
#[derive(Clone, Debug)]
pub struct ClearingResult {
    pub clearing_price: Price,
    pub traded_quantity: Amount,
    pub demand_at_price: Amount,
    pub supply_at_price: Amount,
    pub trades: Vec<Trade>,
}

/// FBA orderbook: the pending-order buffer waiting for the next
/// `clear()`, this engine's full trade history, and the running keep track of its
/// own metric methods. Self-contained — a caller just calls
/// `submit`/`clear`/the metric getters, nothing else needs to reach inside.



pub struct FbaOrderBook {
    pub pending_orders: Vec<Order>,
    pub executed_trades: Vec<Trade>,
    pub last_clearing_price: Option<Price>,
    next_trade_id: u64,

    // Cumulative quantity ever submitted as live demand combined with
    // whatever's still sitting in `pending_orders` right now, this is all
    // `fill_rate` needs (no separate order-state tracking method required).
    total_submitted_qty: Amount,

    // Snapshot of the most recent clear()'s outcome and
    // `unexecuted_residual_share`.
    last_demand_at_price: Amount,
    last_supply_at_price: Amount,
    last_unexecuted_quantity: Amount,
}

impl FbaOrderBook {
    pub fn new() -> Self {
        Self {
            pending_orders: Vec::new(),
            executed_trades: Vec::new(),
            last_clearing_price: None,
            next_trade_id: 1,
            total_submitted_qty: 0,
            last_demand_at_price: 0,
            last_supply_at_price: 0,
            last_unexecuted_quantity: 0,
        }
    }

    /// Rejections and untriggered conditional orders never enter the
    /// batch. When a new live order is submitted, it is added to the
    /// `pending_orders` list. When a cancellation for an
    /// `oid` still sitting in `pending_orders` removes it (see
    /// `Order::is_cancellation` for why `filled` events are deliberately
    /// NOT handled the same way — this simulation's FBA clearing decides
    /// fills independently of whatever Hyperliquid's own engine did).
    /// this would complicate the project and make computing more expensive with no gain
    /// to replicate hyperliquid engine is not the objective
    pub fn submit(&mut self, order: Order) {
        if order.is_new_live_order() {
            self.total_submitted_qty = self.total_submitted_qty.saturating_add(order.remaining);
            self.pending_orders.push(order);
        } else if order.is_cancellation() {
            self.cancel(order.oid);
        }
    }

    /// Removes a still pending order by `oid`, if one is sitting in
    /// `pending_orders` (i.e. it was submitted as live and hasn't cleared
    /// yet). Returns whether anything was actually removed — a cancel for
    /// an `oid` this book never saw as live, or that already cleared, is a
    /// harmless operations nothing happens.
    pub fn cancel(&mut self, oid: u64) -> bool {
        let before = self.pending_orders.len();
        self.pending_orders.retain(|o| o.oid != oid);
        self.pending_orders.len() != before
    }

    /// Clear the current batch at a single uniform price, rationing by
    /// price-time priority, and roll any unfilled residual straight back
    /// into `pending_orders` for the next batch. Fully self-contained.
    pub fn clear(&mut self) -> Option<ClearingResult> {
        let orders = std::mem::take(&mut self.pending_orders);

        ///self.pending contains the pointer, len and capacity of the vector
        ///If Rust allowed you to move self.pending_orders into orders, 
        // self.pending_orders would become uninitialized (empty garbage data). 
        // If clear_batch were to crash or panic mid-execution, 
        // Rust would attempt to run clean-up logic on self, but self would be broken
        // Rust strictly forbids structs from remaining 
        // in an invalid/uninitialized state, even for a microsecond.


// BEFORE std::mem::take:
//   self.pending_orders  ───► [Heap: 5,000 incoming orders]
//   orders               ───► (Uninitialized local variable)

// DURING std::mem::take:
//   1. Creates Vec::default() on Stack -> Pointer: NULL, Capacity: 0, Length: 0 (0 bytes allocated on Heap).
//   2. Swaps the 24-byte Stack header of `self.pending_orders` with `Vec::default()`.

// AFTER std::mem::take:
//   self.pending_orders  ───► [Heap: Empty Vec with 0 allocation]
// orders               ───► [Heap: 5,000 incoming orders]  <-- Local variable now OWNS the orders

// Zero Heap Reallocation: No memory is allocated or freed during the swap. The existing heap memory holding your orders stays untouched in RAM.

// Borrow Checker Compliance: self.pending_orders is instantly replaced with a valid, empty vector Vec::default(). self remains complete and valid.

// Unlocks the Lock: Because orders is moved into a standalone variable, you no longer hold a active reference to self.pending_orders. You can freely call methods on self without triggering borrow-checker errors.

// Extract: std::mem::take safely extracts the pending batch without cloning data.
// Evaluate: The engine attempts to find a single market-clearing price.
// Restore on Failure: If the price discovery algorithm fails (e.g., bid-ask spread does not cross),
//  self.pending_orders = orders reassigns ownership back to self. 
// Unexecuted orders roll over into the next batch window without dropping data.




        if orders.is_empty() {
            return None;
        }

        let candidates = self.candidate_prices(&orders);
        let Some((clearing_price, demand_at_price, supply_at_price)) = self.select_price(&orders, candidates) else {
            // No candidate price at all (e.g. an all-market-order batch —
            // see `candidate_prices`) — nothing executes this round.
            // Put the batch back rather than silently dropping it: `orders`
            // was already drained out of `self.pending_orders` above via
            // `mem::take`, so without this it would just vanish.
            self.pending_orders = orders;
            return None;
        };

        let mut buys = self.eligible_orders(&orders, Side::Buy, clearing_price);
        let mut sells = self.eligible_orders(&orders, Side::Sell, clearing_price);
        let mut trades = Vec::new();

        let batch_ts = orders.iter().map(|o| o.ts).max().unwrap_or(0);
        let mut buy_index = 0usize;
        let mut sell_index = 0usize;


        //while there is orders on buy and sell loop through the orders and match them
        while buy_index < buys.len() && sell_index < sells.len() {

            // the min between quantity of best buy and best sell
            let fill = buys[buy_index].remaining.min(sells[sell_index].remaining);
            // note that the orders we have here are already eligible for the clearing price; 
            // if mkt orders or buy price is equal or less than clearing price; 
            // at this stage we only match quantities untils there is some residuals that cant be matched
            // given the quantities in the batch; note that orders are sorted by price-time priority,
            // so we always match the best buy with the best sell first
           
            if fill == 0 {
                // If the fill is 0, it means one of the orders is fully filled, remaining is zero.
                // We need to move to the next order in that side.
                if buys[buy_index].remaining == 0 { buy_index += 1; }//skip to the next best buy
                if sells[sell_index].remaining == 0 { sell_index += 1; } //skip to the next best sell
                continue;
            }

            let mut buy_order = buys[buy_index].clone();
            let mut sell_order = sells[sell_index].clone();
            buy_order.reduce(fill);
            sell_order.reduce(fill);
            buys[buy_index] = buy_order.clone();
            sells[sell_index] = sell_order.clone();

            trades.push(Trade {
                trade_id: self.next_trade_id,
                price: clearing_price,
                quantity: fill,
                buyer_id: buy_order.user_id.clone(),
                seller_id: sell_order.user_id.clone(),
                buy_order_id: buy_order.oid,
                sell_order_id: sell_order.oid,
                engine_type: EngineKind::Fba,
                ts: batch_ts,
                trade_tx_hash: None,
                chain_id: None,
            });
            self.next_trade_id += 1;

            if buys[buy_index].remaining == 0 { buy_index += 1; }
            if sells[sell_index].remaining == 0 { sell_index += 1; }
        }

        let traded_quantity: Amount = trades.iter().map(|t| t.quantity).sum();
        if traded_quantity > 0 {
            self.last_clearing_price = Some(clearing_price);
        }

        // Residual: whatever's left unfilled on the heavier side, computed
        // against the ORIGINAL batch (not the eligible-only clones above),
        // so orders that weren't even eligible at this price stay fully
        // intact instead of silently vanishing.
        let mut fills: HashMap<u64, Amount> = HashMap::new();
        for trade in &trades {
            *fills.entry(trade.buy_order_id).or_insert(0) += trade.quantity;
            *fills.entry(trade.sell_order_id).or_insert(0) += trade.quantity;
        }

        let mut residual_orders = Vec::new();
        for mut order in orders {
            let filled = fills.get(&order.oid).copied().unwrap_or(0);
            order.remaining = order.remaining.saturating_sub(filled);
            if order.remaining > 0 {
                residual_orders.push(order);
            }
        }
        self.last_demand_at_price = demand_at_price;
        self.last_supply_at_price = supply_at_price;
        // Deliberately NOT `residual_orders`' total remaining: that sum
        // includes orders that were never even eligible at this clearing
        // price (e.g. a buy limit below it), which can vastly outweigh
        // demand_at_price/supply_at_price (both eligible-only) and would
        // push the resulting share past 1. The heavier *eligible* side's
        // leftover is exactly `|demand - supply|` (since traded_quantity
        // is `min(demand, supply)`) — always within [0, max(demand, supply)],
        // so `unexecuted_residual_share` stays a genuine share.
        self.last_unexecuted_quantity = demand_at_price.abs_diff(supply_at_price);
        self.pending_orders = residual_orders;
        self.executed_trades.extend(trades.clone());

        Some(ClearingResult {
            clearing_price,
            traded_quantity,
            demand_at_price,
            supply_at_price,
            trades,
        })
    }

    /// Candidate clearing prices are exactly the submitted limit prices: the
    /// demand/supply step functions only change value at those points, so
    /// the volume-maximizing price is always achievable at one of them.
    ///
    /// A batch with no limit orders at all (every order in it happens to be
    /// a market order) has no price information of its own to clear at —
    /// this deliberately does NOT anchor on `last_clearing_price` to
    /// invent one. Better to do nothing this round: an empty candidate set
    /// makes `select_price` find nothing, so `clear()` rolls the whole
    /// batch into `pending_orders` for the next batch instead of pricing
    /// market orders off of what may be stale history.
    fn candidate_prices(&self, orders: &[Order]) -> BTreeSet<Price> {
        let mut candidates = BTreeSet::new();
        for order in orders {
            if let Some(price) = order.limit_price() {
                candidates.insert(price);
            }
        }
        candidates
    }

    /// Picks the uniform clearing price out of `candidates` (the submitted
    /// limit prices — see `candidate_prices`).
    ///
    /// `demand(p)`: every buy order willing to pay at least `p` (limit >=
    /// p, market orders always count). It is monotonically non-increasing
    /// in `p` — raise the price and only fewer buyers still qualify.
    /// Symmetrically `supply(p)`: every seller willing to accept at most
    /// `p` (limit <= p), monotonically non-decreasing in `p`.
    ///
    /// The quantity that can actually trade at `p` is `min(demand(p),
    /// supply(p))` — a rising-then-falling ("unimodal") curve in `p`, since
    /// demand only falls and supply only rises. Its maximum therefore sits
    /// at the crossing point of the two curves. Both curves are step
    /// functions that only move at the exact limit prices submitted, so the
    /// max is always achievable at one of `candidates` — no need to search
    /// any price in between.
    ///
    /// Selection is a 4-way lexicographic comparison, evaluated top to
    /// bottom, each one only a tie-breaker for the one above it:
    ///   1. Maximize matched volume `min(demand, supply)` — the primary
    ///      objective, this is what actually executes.
    ///   2. Minimize the imbalance `|demand - supply|` — among prices that
    ///      tie on volume, prefer the one closer to true equilibrium, since
    ///      it leaves the smallest residual on the heavier side.
    ///   3. Minimize distance to `last_clearing_price` — among prices still
    ///      tied, prefer continuity with where the market last traded
    ///      rather than jumping around on noise.
    ///   4. Lowest price wins — final deterministic tie-break so the choice
    ///      never depends on `BTreeSet`'s (or any) iteration order.
    fn select_price(&self, orders: &[Order], candidates: BTreeSet<Price>) -> Option<(Price, Amount, Amount)> {
        // demand(p)/supply(p) evaluators, precomputed ONCE for the whole
        // batch rather than rescanned per candidate price. The old version
        // called `aggregate_volume` (a full O(batch) filter+sum) twice for
        // every candidate — up to O(batch) candidates, so O(batch^2)
        // overall per clear(). This batch of ~1-second FBA windows adds up
        // fast on a large replay (the same class of bug `CdaOrderBook::
        // submit` already had fixed — see its own comment on binary-search
        // insertion vs. push+full-sort).
        //
        // Market orders count on both sides unconditionally, so they're
        // just summed up front. Limit orders are sorted once by price with
        // a cumulative-quantity prefix sum, turning "how much qualifies at
        // price p" into one `partition_point` binary search per candidate
        // instead of a full linear rescan.
        let (demand_at, supply_at) = self.demand_supply_evaluators(orders);

        let mut best: Option<(Price, Amount, Amount)> = None;

        for price in candidates {
            let demand = demand_at(price);
            let supply = supply_at(price);
            let volume = demand.min(supply); //the min between the two is the volume that can be executed at this price
            let imbalance = demand.abs_diff(supply); //what is left

            let better = match best {
                None => true, // first candidate seen, nothing to beat yet
                Some((best_price, best_demand, best_supply)) => {
                    let best_volume = best_demand.min(best_supply);
                    let best_imbalance = best_demand.abs_diff(best_supply);

                    if volume != best_volume { //if volume is diff from best one, enter the > comparison; 
                        // if lower goest out; if more then true ; if tie it goes directly to else
                        // Rule 1: strictly more matched volume wins outright.
                        volume > best_volume
                    } else if imbalance != best_imbalance {
                        // Rule 2: same volume, but less leftover on the
                        // heavier side — closer to true equilibrium.
                        imbalance < best_imbalance
                    } else {
                        // Rules 3 & 4: fully tied on volume and imbalance.
                        // Final tie-break: prefer continuity with the last
                        // executed clearing price.
                        match self.last_clearing_price {
                            Some(reference) => {
                                let diff_new = price.abs_diff(reference);
                                let diff_best = best_price.abs_diff(reference);
                                // Closer to the reference price wins; if
                                // still tied on distance too, fall back to
                                // the lowest price (rule 4) for a total order.
                                diff_new < diff_best || (diff_new == diff_best && price < best_price)
                            }
                            // No trading history yet to anchor to — go
                            // straight to rule 4.
                            None => price < best_price,
                        }
                    }
                }
            };

            if better {
                best = Some((price, demand, supply));
            }
        }

        best
    }

    /// Precomputes `demand(p)`/`supply(p)` evaluators for one batch of
    /// `orders`, each callable at O(log n) per candidate price — replaces
    /// what used to be a fresh O(n) `orders.iter().filter(..).sum()` scan
    /// per call (see `select_price`'s comment on why that mattered).
    /// Semantics match the old per-price filter exactly: market orders
    /// always count; a buy limit order counts once `price <= limit`
    /// (`limit >= price`), a sell limit order counts once `limit <= price`.
    fn demand_supply_evaluators(&self, orders: &[Order]) -> (impl Fn(Price) -> Amount, impl Fn(Price) -> Amount) {
        let mut buy_market_qty: Amount = 0;
        let mut buy_limits: Vec<(Price, Amount)> = Vec::new();
        let mut sell_market_qty: Amount = 0;
        let mut sell_limits: Vec<(Price, Amount)> = Vec::new();

        for order in orders {
            match (order.side(), order.kind()) {
                (Side::Buy, OrderKind::Market) => buy_market_qty += order.remaining,
                (Side::Buy, OrderKind::Limit { price }) => buy_limits.push((price, order.remaining)),
                (Side::Sell, OrderKind::Market) => sell_market_qty += order.remaining,
                (Side::Sell, OrderKind::Limit { price }) => sell_limits.push((price, order.remaining)),
            }
        }

        // Ascending by price: demand(p) = market qty + everything from the
        // first buy-limit order priced >= p through the end — that's
        // exactly the old `Side::Buy => limit_price >= price` filter,
        // just read off a prefix-sum instead of rescanned.
        buy_limits.sort_unstable_by_key(|&(price, _)| price);
        let buy_prices: Vec<Price> = buy_limits.iter().map(|&(p, _)| p).collect();
        let buy_cum = cumulative_quantities(&buy_limits);
        let buy_total = *buy_cum.last().unwrap_or(&0);

        // Ascending by price: supply(p) = market qty + everything up to
        // (and including) the last sell-limit order priced <= p — the old
        // `Side::Sell => limit_price <= price` filter, same idea mirrored.
        sell_limits.sort_unstable_by_key(|&(price, _)| price);
        let sell_prices: Vec<Price> = sell_limits.iter().map(|&(p, _)| p).collect();
        let sell_cum = cumulative_quantities(&sell_limits);

        let demand_at = move |price: Price| -> Amount {
            let idx = buy_prices.partition_point(|&p| p < price);
            buy_market_qty + (buy_total - buy_cum[idx])
        };
        let supply_at = move |price: Price| -> Amount {
            let idx = sell_prices.partition_point(|&p| p <= price);
            sell_market_qty + sell_cum[idx]
        };

        (demand_at, supply_at)
    }

    /// Orders eligible to trade at `price`, sorted by price-time priority:
    /// most aggressive price first (market orders ahead of all limit
    /// orders), and among orders at the same price, earliest submission
    /// time first.
    fn eligible_orders(&self, orders: &[Order], side: Side, price: Price) -> Vec<Order> {
        let mut eligible: Vec<Order> = orders
            .iter()
            .filter(|order| order.side() == side)
            .filter(|order| match order.kind() {
                OrderKind::Market => true,
                OrderKind::Limit { price: limit_price } => match side {
                    Side::Buy => limit_price >= price,
                    Side::Sell => limit_price <= price,
                },
            })
            .cloned()
            .collect();

        eligible.sort_by_key(|order| {
            let aggressiveness = self.order_priority(order);
            (aggressiveness, order.ts, order.oid)
        });

        eligible
    }

    fn order_priority(&self, order: &Order) -> (u8, Price) {
        match (order.side(), order.kind()) {
            (Side::Buy, OrderKind::Market) | (Side::Sell, OrderKind::Market) => (0, 0),
            (Side::Buy, OrderKind::Limit { price }) => (1, u128::MAX - price),
            (Side::Sell, OrderKind::Limit { price }) => (1, price),
        }
    }

    // ---- Core metrics, computed on demand from this orderbook's own state ----

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

    pub fn best_unfilled_buy(&self) -> Option<Price> {
        self.pending_orders
            .iter()
            .filter(|o| o.side() == Side::Buy)
            .filter_map(|o| o.limit_price())
            .max()
    }

    pub fn best_unfilled_sell(&self) -> Option<Price> {
        self.pending_orders
            .iter()
            .filter(|o| o.side() == Side::Sell)
            .filter_map(|o| o.limit_price())
            .min()
    }

    /// Implied spread between the best unfilled buy/sell in the pending
    /// buffer (the FBA counterpart of a resting book's quoted spread).
    pub fn quoted_spread_bps(&self) -> Option<f64> {
        let buy = self.best_unfilled_buy()?;
        let sell = self.best_unfilled_sell()?;
        let reference = self.last_clearing_price.unwrap_or((buy + sell) / 2);
        if reference == 0 {
            return None;
        }
        Some(((sell as f64 - buy as f64) / reference as f64) * 10_000.0)
    }

    /// Total remaining volume currently sitting in the pending buffer.
    pub fn depth_at_best(&self) -> Amount {
        self.pending_orders.iter().map(|o| o.remaining).sum()
    }

    /// Filled / submitted, across everything ever submitted to this book.
    pub fn fill_rate(&self) -> Option<f64> {
        if self.total_submitted_qty == 0 {
            return None;
        }
        let still_pending: Amount = self.pending_orders.iter().map(|o| o.remaining).sum();
        let filled = self.total_submitted_qty.saturating_sub(still_pending);
        Some(filled as f64 / self.total_submitted_qty as f64)
    }

    /// Share of the heavier side's volume that went unexecuted at the last
    /// batch's clearing price.
    pub fn unexecuted_residual_share(&self) -> Option<f64> {
        let total_side = self.last_demand_at_price.max(self.last_supply_at_price);
        if total_side == 0 {
            return None;
        }
        Some(self.last_unexecuted_quantity as f64 / total_side as f64)
    }
}

impl Default for FbaOrderBook {
    fn default() -> Self {
        Self::new()
    }
}

/// `cumulative_quantities(sorted)[i]` = sum of `remaining` for the first
/// `i` entries of `sorted` (already sorted ascending by price) — index
/// `sorted.len()` (the last entry) is the grand total. Used by
/// `FbaOrderBook::demand_supply_evaluators` to turn "how much volume
/// qualifies at price p" into an O(log n) binary search + array lookup
/// instead of an O(n) rescan per candidate price.
fn cumulative_quantities(sorted: &[(Price, Amount)]) -> Vec<Amount> {
    let mut cum = Vec::with_capacity(sorted.len() + 1);
    let mut running: Amount = 0;
    cum.push(running);
    for &(_, qty) in sorted {
        running += qty;
        cum.push(running);
    }
    cum
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

    /// The exact semantics `demand_supply_evaluators` replaced — kept here
    /// only as an independent differential-test reference, not in the
    /// production path anymore.
    fn naive_volume(orders: &[Order], side: Side, price: Price) -> Amount {
        orders
            .iter()
            .filter(|o| o.side() == side)
            .filter(|o| match o.kind() {
                OrderKind::Market => true,
                OrderKind::Limit { price: limit_price } => match side {
                    Side::Buy => limit_price >= price,
                    Side::Sell => limit_price <= price,
                },
            })
            .map(|o| o.remaining)
            .sum()
    }

    #[test]
    fn demand_supply_evaluators_boundary_prices_match_filter_semantics() {
        let orders = vec![
            Order::limit(1, "a", Side::Buy, 100, 5, 0),  // exactly at price -> counts (limit >= price)
            Order::limit(2, "a", Side::Buy, 99, 5, 0),   // below price -> excluded
            Order::limit(3, "a", Side::Buy, 101, 5, 0),  // above price -> counts
            Order::market(4, "a", Side::Buy, 7, 0),      // market -> always counts
            Order::limit(5, "a", Side::Sell, 100, 3, 0), // exactly at price -> counts (limit <= price)
            Order::limit(6, "a", Side::Sell, 101, 3, 0), // above price -> excluded
            Order::limit(7, "a", Side::Sell, 99, 3, 0),  // below price -> counts
            Order::market(8, "a", Side::Sell, 2, 0),     // market -> always counts
        ];
        let book = FbaOrderBook::new();
        let (demand_at, supply_at) = book.demand_supply_evaluators(&orders);

        assert_eq!(demand_at(100), 5 + 5 + 7);
        assert_eq!(supply_at(100), 3 + 3 + 2);
        // Cross-check against the naive reference too.
        assert_eq!(demand_at(100), naive_volume(&orders, Side::Buy, 100));
        assert_eq!(supply_at(100), naive_volume(&orders, Side::Sell, 100));
    }

    #[test]
    fn demand_supply_evaluators_handle_empty_and_all_market_batches() {
        let book = FbaOrderBook::new();

        let (demand_at, supply_at) = book.demand_supply_evaluators(&[]);
        assert_eq!(demand_at(50), 0);
        assert_eq!(supply_at(50), 0);

        let orders = vec![Order::market(1, "a", Side::Buy, 10, 0), Order::market(2, "a", Side::Sell, 4, 0)];
        let (demand_at, supply_at) = book.demand_supply_evaluators(&orders);
        // Market orders count at every price, however extreme.
        assert_eq!(demand_at(0), 10);
        assert_eq!(demand_at(u128::MAX), 10);
        assert_eq!(supply_at(0), 4);
        assert_eq!(supply_at(u128::MAX), 4);
    }

    /// Differential test: `demand_supply_evaluators`'s prefix-sum + binary
    /// search must agree exactly (integer amounts, no floats) with the old
    /// per-price O(n) filter+sum, across many random batches and prices.
    #[test]
    fn demand_supply_evaluators_match_naive_scan_across_random_batches() {
        let mut rng = Lcg(0x5EED_1234);
        let book = FbaOrderBook::new();

        for _ in 0..200 {
            let n = rng.range(40) as usize;
            let mut orders = Vec::with_capacity(n);
            for i in 0..n {
                let side = if rng.range(2) == 0 { Side::Buy } else { Side::Sell };
                let qty = (rng.range(100) + 1) as Amount;
                if rng.range(5) == 0 {
                    orders.push(Order::market(i as u64, "u", side, qty, i as u64));
                } else {
                    let price = rng.range(50) as Price;
                    orders.push(Order::limit(i as u64, "u", side, price, qty, i as u64));
                }
            }

            let (demand_at, supply_at) = book.demand_supply_evaluators(&orders);
            for _ in 0..20 {
                let p = rng.range(60) as Price;
                assert_eq!(demand_at(p), naive_volume(&orders, Side::Buy, p), "demand mismatch at p={p}, orders={orders:?}");
                assert_eq!(supply_at(p), naive_volume(&orders, Side::Sell, p), "supply mismatch at p={p}, orders={orders:?}");
            }
        }
    }
}

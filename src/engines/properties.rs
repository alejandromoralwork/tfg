//! Property tests for both matching engines (test-only module, no engine
//! code is touched).
//!
//! Every scenario is a seeded pseudo-random order flow. After EVERY event the
//! engine is compared with a deliberately naive oracle written here from the
//! written rules alone (full scans, no maps, no prefix sums), and a list of
//! invariants is asserted. Each check bumps a counter, and the totals are
//! printed (run with `cargo test properties -- --nocapture`) so the thesis can
//! quote exactly how much was checked.

use std::cmp::Reverse;
use std::collections::HashMap;

use crate::engines::cda::CdaOrderBook;
use crate::engines::fba::FbaOrderBook;
use crate::types::{Amount, Order, OrderKind, Price, Side, Trade};

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

/// Counts every assertion so the totals can be reported.
#[derive(Default)]
struct Checks {
    n: u64,
}
impl Checks {
    fn ok(&mut self, cond: bool, msg: impl FnOnce() -> String) {
        self.n += 1;
        assert!(cond, "{}", msg());
    }
}

const USERS: [&str; 4] = ["u0", "u1", "u2", "u3"]; // few users so self-matching occurs
const PX_LO: u64 = 95;
const PX_HI: u64 = 105;

fn random_order(rng: &mut Lcg, oid: u64, ts: u64) -> Order {
    let user = USERS[rng.range(4) as usize];
    let side = if rng.range(2) == 0 { Side::Buy } else { Side::Sell };
    let qty = 1 + rng.range(20) as Amount;
    if rng.range(100) < 18 {
        Order::market(oid, user, side, qty, ts)
    } else {
        let px = (PX_LO + rng.range(PX_HI - PX_LO + 1)) as Price;
        Order::limit(oid, user, side, px, qty, ts)
    }
}

fn limit_of(o: &Order) -> Option<Price> {
    match o.kind() {
        OrderKind::Limit { price } => Some(price),
        OrderKind::Market => None,
    }
}

// =====================================================================
// CDA: differential test against a naive full-scan book
// =====================================================================

/// Naive continuous book: two flat vectors, best order found by scanning
/// every resting order.
#[derive(Default)]
struct NaiveCda {
    bids: Vec<Order>,
    asks: Vec<Order>,
}

/// (buy oid, sell oid, price, qty) of one fill.
type Fill = (u64, u64, Price, Amount);

impl NaiveCda {
    fn submit(&mut self, mut o: Order) -> Vec<Fill> {
        let mut fills = Vec::new();
        if o.remaining == 0 {
            return fills;
        }
        let is_buy = o.side() == Side::Buy;
        while o.remaining > 0 {
            let book = if is_buy { &mut self.asks } else { &mut self.bids };
            // best maker = best price, then earliest (ts, oid); scan everything
            let mut best: Option<usize> = None;
            for (i, m) in book.iter().enumerate() {
                let mp = limit_of(m).unwrap();
                let crosses = match limit_of(&o) {
                    None => true,
                    Some(tp) => if is_buy { tp >= mp } else { tp <= mp },
                };
                if !crosses {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some(b) => {
                        let bp = limit_of(&book[b]).unwrap();
                        let key_new = (if is_buy { mp } else { u128::MAX - mp }, m.ts, m.oid);
                        let key_old = (if is_buy { bp } else { u128::MAX - bp }, book[b].ts, book[b].oid);
                        key_new < key_old
                    }
                };
                if better {
                    best = Some(i);
                }
            }
            let Some(i) = best else { break };
            let q = o.remaining.min(book[i].remaining);
            let px = limit_of(&book[i]).unwrap();
            book[i].remaining -= q;
            o.remaining -= q;
            let (b, s) = if is_buy { (o.oid, book[i].oid) } else { (book[i].oid, o.oid) };
            fills.push((b, s, px, q));
            if book[i].remaining == 0 {
                book.remove(i);
            }
        }
        if o.remaining > 0 && limit_of(&o).is_some() {
            if is_buy { self.bids.push(o) } else { self.asks.push(o) }
        }
        fills
    }

    fn cancel(&mut self, oid: u64) -> bool {
        for book in [&mut self.bids, &mut self.asks] {
            if let Some(i) = book.iter().position(|o| o.oid == oid) {
                book.remove(i);
                return true;
            }
        }
        false
    }

    /// Resting orders in the engine's iteration order: best price first, then (ts, oid).
    fn sorted(&self, buys: bool) -> Vec<(u64, Price, Amount)> {
        let mut v: Vec<&Order> = if buys { self.bids.iter().collect() } else { self.asks.iter().collect() };
        v.sort_by_key(|o| {
            let p = limit_of(o).unwrap();
            (if buys { u128::MAX - p } else { p }, o.ts, o.oid)
        });
        v.iter().map(|o| (o.oid, limit_of(o).unwrap(), o.remaining)).collect()
    }
}

fn cda_scenario(seed: u64, events: usize, c: &mut Checks) -> (u64, u64) {
    let mut rng = Lcg(seed);
    let mut engine = CdaOrderBook::new();
    let mut oracle = NaiveCda::default();
    let mut originals: HashMap<u64, Order> = HashMap::new();
    let mut filled: HashMap<u64, Amount> = HashMap::new();
    let mut next_oid = 1u64;
    let mut ts = 1_000u64;
    let (mut trades_seen, mut cancels_hit) = (0u64, 0u64);

    for step in 0..events {
        // timestamps mostly increase, sometimes repeat or go slightly back
        ts = match rng.range(10) {
            0 => ts.saturating_sub(rng.range(3)),
            1 | 2 => ts,
            _ => ts + rng.range(5),
        };
        let ctx = format!("seed {seed} step {step}");

        if rng.range(100) < 20 && next_oid > 1 {
            // cancel of a random oid: live, already filled/cancelled, or never used
            let oid = 1 + rng.range(next_oid + 3);
            let got = engine.cancel(oid);
            let want = oracle.cancel(oid);
            c.ok(got == want, || format!("cancel({oid}) returned {got}, oracle {want} [{ctx}]"));
            if got {
                cancels_hit += 1;
            }
        } else {
            let o = random_order(&mut rng, next_oid, ts);
            next_oid += 1;
            originals.insert(o.oid, o.clone());
            let taker_is_buy = o.side() == Side::Buy;
            let taker_limit = limit_of(&o);
            let trades: Vec<Trade> = engine.submit(o.clone());
            let want = oracle.submit(o.clone());

            let got: Vec<Fill> = trades.iter().map(|t| (t.buy_order_id, t.sell_order_id, t.price, t.quantity)).collect();
            c.ok(got == want, || format!("fill sequence differs [{ctx}]\n engine {got:?}\n oracle {want:?}"));

            let mut last_px: Option<Price> = None;
            for t in &trades {
                trades_seen += 1;
                c.ok(t.quantity > 0, || format!("zero-quantity trade [{ctx}]"));
                let maker_oid = if taker_is_buy { t.sell_order_id } else { t.buy_order_id };
                let maker = &originals[&maker_oid];
                c.ok(Some(t.price) == limit_of(maker), || format!("trade not at the maker's limit [{ctx}]"));
                if let Some(tp) = taker_limit {
                    c.ok(if taker_is_buy { t.price <= tp } else { t.price >= tp }, || format!("taker limit violated [{ctx}]"));
                }
                if let Some(prev) = last_px {
                    c.ok(if taker_is_buy { t.price >= prev } else { t.price <= prev }, || format!("price priority violated [{ctx}]"));
                }
                last_px = Some(t.price);
                *filled.entry(t.buy_order_id).or_default() += t.quantity;
                *filled.entry(t.sell_order_id).or_default() += t.quantity;
            }
        }

        // ---- state invariants after every event ----
        let bids: Vec<(u64, Price, Amount)> = engine.bids_iter().map(|o| (o.oid, limit_of(o).unwrap(), o.remaining)).collect();
        let asks: Vec<(u64, Price, Amount)> = engine.asks_iter().map(|o| (o.oid, limit_of(o).unwrap(), o.remaining)).collect();
        c.ok(bids == oracle.sorted(true), || format!("bid book differs from oracle [{ctx}]"));
        c.ok(asks == oracle.sorted(false), || format!("ask book differs from oracle [{ctx}]"));
        if let (Some(b), Some(a)) = (engine.best_bid(), engine.best_ask()) {
            c.ok(b < a, || format!("book crossed: bid {b} >= ask {a} [{ctx}]"));
        }
        c.ok(engine.bid_depth() == bids.iter().map(|x| x.2).sum::<Amount>(), || format!("bid depth counter drift [{ctx}]"));
        c.ok(engine.ask_depth() == asks.iter().map(|x| x.2).sum::<Amount>(), || format!("ask depth counter drift [{ctx}]"));
        c.ok(engine.bid_count() == bids.len() && engine.ask_count() == asks.len(), || format!("order count drift [{ctx}]"));
        c.ok(bids.iter().chain(asks.iter()).all(|x| x.2 > 0), || format!("empty order resting [{ctx}]"));
        // quantity conservation per resting order: remaining = original - filled
        for (oid, _, rem) in bids.iter().chain(asks.iter()) {
            let orig = originals[oid].remaining;
            let f = filled.get(oid).copied().unwrap_or(0);
            c.ok(*rem == orig - f, || format!("order {oid}: remaining {rem} != {orig} - {f} [{ctx}]"));
        }
    }
    // no order was ever filled beyond its size
    for (oid, f) in &filled {
        c.ok(*f <= originals[oid].remaining, || format!("order {oid} overfilled (seed {seed})"));
    }
    // total bought == total sold
    let vol: Amount = engine.executed_trades.iter().map(|t| t.quantity).sum();
    c.ok(vol == engine.executed_volume(), || format!("executed_volume mismatch (seed {seed})"));
    (trades_seen, cancels_hit)
}

#[test]
fn cda_matches_naive_oracle_and_keeps_invariants() {
    let mut c = Checks::default();
    let (scenarios, events) = (3_000u64, 120usize);
    let (mut trades, mut cancels) = (0u64, 0u64);
    for s in 0..scenarios {
        let (t, k) = cda_scenario(0xC0FFEE ^ (s * 7919 + 1), events, &mut c);
        trades += t;
        cancels += k;
    }
    println!("PROPERTY_SUMMARY cda scenarios={scenarios} events={} trades={trades} cancels_hit={cancels} assertions={}", scenarios * events as u64, c.n);
}

// =====================================================================
// FBA: every clear() against brute force
// =====================================================================

fn naive_demand(orders: &[Order], p: Price) -> Amount {
    orders.iter().filter(|o| o.side() == Side::Buy).filter(|o| limit_of(o).map_or(true, |l| l >= p)).map(|o| o.remaining).sum()
}
fn naive_supply(orders: &[Order], p: Price) -> Amount {
    orders.iter().filter(|o| o.side() == Side::Sell).filter(|o| limit_of(o).map_or(true, |l| l <= p)).map(|o| o.remaining).sum()
}

/// Expected fill of every eligible order: walk the side in priority order
/// (market first, then better price, then ts, then oid) and hand out `traded`.
fn expected_allocation(orders: &[Order], side: Side, p: Price, traded: Amount) -> HashMap<u64, Amount> {
    let mut elig: Vec<&Order> = orders
        .iter()
        .filter(|o| o.side() == side)
        .filter(|o| match limit_of(o) {
            None => true,
            Some(l) => if side == Side::Buy { l >= p } else { l <= p },
        })
        .collect();
    elig.sort_by_key(|o| {
        let rank = match limit_of(o) {
            None => (0u8, 0u128),
            Some(l) => (1u8, if side == Side::Buy { u128::MAX - l } else { l }),
        };
        (rank, o.ts, o.oid)
    });
    let mut left = traded;
    let mut out = HashMap::new();
    for o in elig {
        let f = o.remaining.min(left);
        left -= f;
        out.insert(o.oid, f);
    }
    out
}

fn fba_scenario(seed: u64, batches: usize, c: &mut Checks, better_orders: &mut u64, better_partial: &mut u64) -> (u64, u64, u64) {
    let mut rng = Lcg(seed);
    let mut fba = FbaOrderBook::new();
    let mut next_oid = 1u64;
    let mut ts = 1_000u64;
    let (mut cleared, mut traded_batches, mut ties) = (0u64, 0u64, 0u64);

    for b in 0..batches {
        // orders and cancels arriving during the batch (residuals of earlier batches are already pending)
        let n = rng.range(30);
        for _ in 0..n {
            ts += rng.range(3);
            if rng.range(100) < 12 && next_oid > 1 {
                fba.cancel(1 + rng.range(next_oid + 2));
            } else {
                let o = random_order(&mut rng, next_oid, ts);
                next_oid += 1;
                fba.submit(o);
            }
        }
        let ctx = format!("seed {seed} batch {b}");
        let before: Vec<Order> = fba.pending_orders.clone();
        let last_before = fba.last_clearing_price;
        let result = fba.clear();
        cleared += 1;

        let candidates: std::collections::BTreeSet<Price> = before.iter().filter_map(limit_of).collect();
        if before.is_empty() || candidates.is_empty() {
            c.ok(result.is_none(), || format!("clear() priced a batch with no limit order [{ctx}]"));
            c.ok(fba.pending_orders.len() == before.len(), || format!("batch lost orders when it could not clear [{ctx}]"));
            continue;
        }
        let res = result.unwrap_or_else(|| panic!("clear() returned None with candidates [{ctx}]"));
        let p = res.clearing_price;

        // -- price selection against brute force over the four-step cascade --
        let key = |q: Price| {
            let d = naive_demand(&before, q);
            let s = naive_supply(&before, q);
            (Reverse(d.min(s)), d.abs_diff(s), last_before.map_or(0, |l| q.abs_diff(l)), q)
        };
        let best = candidates.iter().copied().min_by_key(|&q| key(q)).unwrap();
        c.ok(p == best, || format!("clearing price {p} != brute-force optimum {best} [{ctx}]"));
        let vol = naive_demand(&before, p).min(naive_supply(&before, p));
        c.ok(res.demand_at_price == naive_demand(&before, p), || format!("reported demand wrong [{ctx}]"));
        c.ok(res.supply_at_price == naive_supply(&before, p), || format!("reported supply wrong [{ctx}]"));
        c.ok(res.traded_quantity == vol, || format!("traded {} != min(D,S) {vol} [{ctx}]", res.traded_quantity));
        // volume-maximisation over EVERY integer price (the candidate-price lemma)
        let lo = PX_LO.saturating_sub(3) as Price;
        let hi = (PX_HI + 3) as Price;
        let global_max = (lo..=hi).map(|q| naive_demand(&before, q).min(naive_supply(&before, q))).max().unwrap();
        c.ok(vol == global_max, || format!("volume {vol} below the global maximum {global_max} [{ctx}]"));
        if candidates.iter().filter(|&&q| key(q).0 == key(p).0 && key(q).1 == key(p).1).count() > 1 {
            ties += 1;
        }

        // -- every trade: uniform price, eligibility, sides --
        let by_oid: HashMap<u64, &Order> = before.iter().map(|o| (o.oid, o)).collect();
        let mut got: HashMap<u64, Amount> = HashMap::new();
        let (mut sum_buy, mut sum_sell) = (0, 0);
        for t in &res.trades {
            c.ok(t.price == p, || format!("non-uniform trade price [{ctx}]"));
            c.ok(t.quantity > 0, || format!("zero-quantity trade [{ctx}]"));
            let bo = by_oid[&t.buy_order_id];
            let so = by_oid[&t.sell_order_id];
            c.ok(bo.side() == Side::Buy && so.side() == Side::Sell, || format!("trade sides wrong [{ctx}]"));
            c.ok(limit_of(bo).map_or(true, |l| l >= p), || format!("buy filled above its limit [{ctx}]"));
            c.ok(limit_of(so).map_or(true, |l| l <= p), || format!("sell filled below its limit [{ctx}]"));
            *got.entry(bo.oid).or_default() += t.quantity;
            *got.entry(so.oid).or_default() += t.quantity;
            sum_buy += t.quantity;
            sum_sell += t.quantity;
        }
        c.ok(sum_buy == res.traded_quantity && sum_sell == res.traded_quantity, || format!("side totals differ [{ctx}]"));

        // -- rationing: allocation equals price-time priority walk on each side --
        for side in [Side::Buy, Side::Sell] {
            let want = expected_allocation(&before, side, p, res.traded_quantity);
            for o in before.iter().filter(|o| o.side() == side) {
                let w = want.get(&o.oid).copied().unwrap_or(0);
                let g = got.get(&o.oid).copied().unwrap_or(0);
                c.ok(w == g, || format!("order {} got {g}, price-time priority says {w} [{ctx}]", o.oid));
            }
        }

        // -- design claim "strictly better orders fill completely" holds only on the short side --
        // (on the long side priority rationing can leave a strictly better order partly
        // unfilled when the price range that clears the batch is not a single point);
        // counted, not asserted: the allocation check above is the exact rule.
        for o in &before {
            let strictly_better = match (o.side(), limit_of(o)) {
                (Side::Buy, Some(l)) => l > p,
                (Side::Sell, Some(l)) => l < p,
                _ => false,
            };
            if strictly_better {
                *better_orders += 1;
                if got.get(&o.oid).copied().unwrap_or(0) < o.remaining {
                    *better_partial += 1;
                    let long_side = if res.demand_at_price > res.supply_at_price { Side::Buy } else { Side::Sell };
                    c.ok(o.side() == long_side, || format!("strictly better order partly filled on the SHORT side [{ctx}]"));
                }
            }
        }

        // -- conservation and roll-over --
        let after: HashMap<u64, Amount> = fba.pending_orders.iter().map(|o| (o.oid, o.remaining)).collect();
        for o in &before {
            let f = got.get(&o.oid).copied().unwrap_or(0);
            c.ok(f <= o.remaining, || format!("order {} overfilled [{ctx}]", o.oid));
            let left = o.remaining - f;
            c.ok(after.get(&o.oid).copied().unwrap_or(0) == left, || format!("residual of order {} wrong [{ctx}]", o.oid));
        }
        let (q_before, q_after): (Amount, Amount) = (before.iter().map(|o| o.remaining).sum(), fba.pending_orders.iter().map(|o| o.remaining).sum());
        c.ok(q_before == q_after + 2 * res.traded_quantity, || format!("quantity not conserved [{ctx}]"));
        c.ok(after.len() == fba.pending_orders.len(), || format!("duplicate oid pending [{ctx}]"));
        c.ok(fba.pending_orders.iter().all(|o| o.remaining > 0), || format!("empty order rolled over [{ctx}]"));
        if res.traded_quantity > 0 {
            traded_batches += 1;
            c.ok(fba.last_clearing_price == Some(p), || format!("last_clearing_price not updated [{ctx}]"));
        } else {
            c.ok(fba.last_clearing_price == last_before, || format!("last_clearing_price changed without trade [{ctx}]"));
        }
    }
    (cleared, traded_batches, ties)
}

#[test]
fn fba_clear_matches_brute_force_and_keeps_invariants() {
    let mut c = Checks::default();
    let scenarios = 3_000u64;
    let batches = 12usize;
    let (mut cleared, mut traded, mut ties) = (0, 0, 0);
    let (mut better_orders, mut better_partial) = (0u64, 0u64);
    for s in 0..scenarios {
        let (a, b, t) = fba_scenario(0xFBA5EED ^ (s * 104729 + 3), batches, &mut c, &mut better_orders, &mut better_partial);
        cleared += a;
        traded += b;
        ties += t;
    }
    println!("PROPERTY_SUMMARY fba scenarios={scenarios} batches={cleared} batches_with_trades={traded} batches_with_price_ties={ties} strictly_better_orders={better_orders} of_which_partly_filled_long_side={better_partial} assertions={}", c.n);
}

#[test]
fn both_engines_are_deterministic() {
    // the same flow, replayed twice, gives identical trades
    let run = |seed: u64| -> (Vec<Fill>, Vec<Fill>) {
        let mut rng = Lcg(seed);
        let (mut cda, mut fba) = (CdaOrderBook::new(), FbaOrderBook::new());
        let mut ts = 1_000u64;
        let mut fba_fills = Vec::new();
        for oid in 1..=400u64 {
            ts += rng.range(3);
            let o = random_order(&mut rng, oid, ts);
            cda.submit(o.clone());
            fba.submit(o);
            if oid % 25 == 0 {
                if let Some(r) = fba.clear() {
                    fba_fills.extend(r.trades.iter().map(|t| (t.buy_order_id, t.sell_order_id, t.price, t.quantity)));
                }
            }
        }
        let cda_fills = cda.executed_trades.iter().map(|t| (t.buy_order_id, t.sell_order_id, t.price, t.quantity)).collect();
        (cda_fills, fba_fills)
    };
    let mut n = 0u64;
    for seed in 1..=500u64 {
        assert_eq!(run(seed), run(seed), "seed {seed}");
        n += 1;
    }
    println!("PROPERTY_SUMMARY determinism replays_compared={n}");
}

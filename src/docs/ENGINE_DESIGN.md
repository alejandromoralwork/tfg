# Engine Design Notes

Reference documentation for how the matching engines actually work. It covers two parts:

1. [How the FBA auction price is calculated](#1-how-the-fba-auction-price-is-calculated)
2. [Order flow: module interaction from submission to settlement](#2-order-flow-module-interaction-from-submission-to-settlement)

This is a single crate (`market_sim`, `Cargo.toml` + `src/` under
`c:\Users\pc\other\src`) — every path below is relative to `src/`.
References point to file and function names, not line numbers: those
drift every time a file is edited, so a "line 167" citation goes stale
almost immediately and is worse than no citation at all.

---

## 1. How the FBA auction price is calculated

### 1.1 The core idea

The FBA does not match orders one pair at a time the way the CDA does.
Instead it collects a whole batch of orders (`FbaOrderBook::pending_orders`)
and computes **one single price** — the *uniform clearing price* — that
every trade in that batch executes at. The price is chosen to **maximize
the total quantity that can be matched**. This is the standard
uniform-price call-auction rule (Budish, Cramton & Shim, 2015).

### 1.2 Step by step

All of this lives in `engines/fba.rs`.

**Step 1 — Candidate prices** (`candidate_prices`)

Only prices that were actually submitted as a limit price are considered —
never an arbitrary price in between. This is safe because demand and supply
are step functions that only change value at a submitted limit price, so the
volume-maximizing price is always achievable at one of them. Market orders
never contribute a candidate (they carry no price).

**Step 2 — Evaluate every candidate** (`select_price` + `aggregate_volume`)

For each candidate price `P`:

| Quantity | Definition |
|---|---|
| `demand(P)` | Sum of remaining quantity across all buy orders with limit `≥ P`, plus every buy market order (willing to transact at any price) |
| `supply(P)` | Sum of remaining quantity across all sell orders with limit `≤ P`, plus every sell market order |
| `matched_volume(P)` | `min(demand(P), supply(P))` |
| `imbalance(P)` | `\|demand(P) − supply(P)\|` |

**Step 3 — Pick the winning price**, three tiers in order:

1. **Maximize** `matched_volume(P)` across all candidates.
2. Tie → **minimize** `imbalance(P)` (fewer orders left unmatched).
3. Still tied → **pick the price closest to `last_clearing_price`** — the
   price this engine last actually cleared a trade at. This prevents the
   clearing price from jumping around for no reason when several candidates
   are mathematically equivalent; it's the same continuity convention real
   call auctions use (e.g. an opening cross referencing the prior close).
   With no price history yet, it deterministically picks the lower
   candidate instead.

```
for each candidate price P:
    compute demand(P), supply(P), volume(P), imbalance(P)
    P is better than the current best if:
        volume(P) > best.volume                                   -- tier 1
        OR (volume(P) == best.volume AND imbalance(P) < best.imbalance)   -- tier 2
        OR (volume(P) == best.volume AND imbalance(P) == best.imbalance
            AND |P - last_clearing_price| < |best - last_clearing_price)  -- tier 3
```

**Special case — a batch with no limit orders at all** (e.g. every order in
it happens to be a market order): there is no price information in the
batch to work with, so `candidate_prices` returns an empty set — it does
**not** fall back to `last_clearing_price` to invent one (an earlier
version of this code did; that was a bug, since it priced market orders off
of what may be several batches' worth of stale history). With an empty
candidate set `select_price` finds nothing, and `clear()` rolls the whole
batch straight into `pending_orders` for the next batch instead of clearing
it — see `fba_all_market_no_history_preserves_orders` and
`fba_all_market_with_history_still_rolls_over` in `inputs/test_suite.rs`
for this locked in as running tests (the second one specifically proves
having history present doesn't change the outcome).

**Step 4 — Every trade in the batch executes at that one price.** The same
`clearing_price` value is used for every `Trade` pushed inside the matching
loop. That is the defining uniform-price property — unlike the CDA, where
each trade prices at whatever the resting maker's own limit was.

**Step 5 — Rationing the heavier side.** If `demand(P*) ≠ supply(P*)`, the
larger side cannot be filled in full. Both sides are sorted by **price-time
priority** (`eligible_orders` / `order_priority`): most aggressive price
first (market orders ahead of every limit order), and among orders at the
same price, earliest submission time wins. The matching loop then walks
both sorted lists head-to-head, filling from the front of each until one
side runs out — which is always the side with less volume. The orders left
partially or fully unfilled are always exactly the ones sitting *at* the
clearing price (never a strictly-better-priced order), and among those,
later-submitted ones lose out first.

### 1.3 Worked example

Batch:

| Order | Side | Limit | Qty | Submitted |
|---|---|---|---|---|
| B1 | Buy | 105 | 10 | t=1 |
| B2 | Buy | 100 | 10 | t=2 |
| B3 | Buy | 100 | 10 | t=3 |
| S1 | Sell | 100 | 15 | t=1 |

Candidates are `{105, 100}`. At `P=105`: `demand=10` (only B1 qualifies),
`supply=15` → `volume=10`. At `P=100`: `demand=30` (all three buys
qualify), `supply=15` → `volume=15`. `P=100` uniquely maximizes matched
volume, so it wins outright — no tie-break needed. Sorted by priority:
buys = `[B1, B2, B3]`, sells = `[S1]`.

1. B1 (best price) matches 10 units against S1 → **B1 fully filled**.
2. S1 has 5 left → matches against B2 (earlier of the two orders tied at
   100) → **B2 filled 5/10**, S1 fully consumed.
3. Loop ends (seller side exhausted). **B3 gets 0.**

Everyone strictly better than 100 fills first and in full; between the two
orders tied exactly at the clearing price, the earlier one wins the
remaining capacity. Price-time priority falls straight out of the
sequential matching loop — no separate rationing step is needed.

> **A tempting variant, and why it's wrong to use as a "clean" example:**
> if S1 were priced at 90 instead of 100 (as an earlier draft of this doc
> had it), `P=90` becomes a candidate too, and `demand(90)=30`,
> `supply(90)=15` — the *same* `volume=15` and `imbalance=15` as `P=100`.
> That's a genuine tie, and with no `last_clearing_price` history yet, tier
> 3's fallback deterministically prefers the *lower* price — so the batch
> would actually clear at **90**, not 100 (same fills, different printed
> price). This is exercised directly by `fba_tie_no_history_picks_lower_price`
> and `fba_tie_with_history_picks_closest_price` in `inputs/test_suite.rs`.
> The `S1 @ 100` version above avoids that ambiguity so the rationing walk-through
> stays unambiguous, but it's worth knowing the tie-break rule is real and
> does change outcomes on real, less tidy order flow.

### 1.4 What this deliberately does *not* do

- **No external liquidity source.** There is no AMM or other counterparty of
  last resort. Unmatched volume on the heavier side simply stays unexecuted
  and rolls over to the next batch (`FbaOrderBook::clear`'s residual
  handling).
- **No multi-asset routing.** There is no `AssetPair`/pair concept anywhere
  in `Order`/`Trade` at all — this simulation only ever trades one implicit
  fixed pair (SOL/USD). One batch, one clearing price, no per-pair
  bucketing to even consider.
- **No general LP solver.** For a single asset the volume-maximizing price
  has a closed-form solution (the candidate scan above), so no external
  linear-programming dependency is needed or present — there's no LP
  encoder/solver anywhere in this crate.
- **No anchoring on stale price history for market-only batches** (see the
  Step 3 special case above) — a batch with no price information of its
  own is left unpriced this round rather than guessing.

---

## 2. Order flow: module interaction from submission to settlement

### 2.1 Module map

No box-drawing here — just what depends on what, and why:

```
types.rs
    Order, Trade, Side, OrderKind, EngineKind, PRICE_SCALE.
    Depends on nothing else in this crate.

engines/fba.rs   ->  FbaOrderBook   (submit, cancel, clear, metric methods)
engines/cda.rs   ->  CdaOrderBook   (submit, cancel, metric methods)
    Both depend only on types.rs. Neither knows the other exists, and
    neither knows metrics/ or inputs/ exist.

inputs/simulator.rs
    load_order_status_csv(path) -> Vec<Order>. Depends only on types::Order
    — a pure CSV-row-to-Order parser, no engine knowledge at all.

inputs/test_suite.rs
    run_fba_tests() / run_cda_tests() / print_checklist(). Constructs
    fresh, isolated FbaOrderBook/CdaOrderBook instances per test case and
    asserts on their behavior directly.

metrics/stats.rs
    print_summary / print_fba / print_cda. Depends on engines/fba.rs and
    engines/cda.rs, but adds no calculation of its own — every number it
    prints comes from a `pub fn` already exposed on FbaOrderBook/
    CdaOrderBook (see §2.5).

inputs/cli.rs
    The interactive REPL (`pub fn run()`). Owns exactly one FbaOrderBook
    and one CdaOrderBook for the whole session, parses commands, and
    dispatches to all of the above depending on which command ran and
    which engine mode is currently active.

main.rs
    mod types; mod engines; mod inputs; mod metrics;
    fn main() { inputs::cli::run(); }
```

The key boundary is the same spirit as before, just simpler: **the engines
never import `metrics`, and never know a CLI exists.** `inputs/cli.rs` is
the only thing that touches both an engine and `metrics::stats` in the same
place.

### 2.2 FBA path — from `add`/`load` to a settled batch

```
User types:  add buy 127 5 Alice
  (or: a `load <path>` command feeds Orders parsed straight from CSV rows,
  each already carrying its own real historical ts/oid/status_id, instead
  of one built fresh from typed arguments)

1. inputs/cli.rs builds an Order — Order::limit(...) for a typed `add`
   (using its own incrementing order_id_counter and the current wall-clock
   time as `ts`), or takes one as-is from inputs/simulator.rs for a `load`.

2. cli.rs calls fba.submit(order)                    [FbaOrderBook::submit]

       if order.is_cancellation():
           self.cancel(order.oid)      -- remove a still-pending order
                                           sharing this oid, if any (§2.4)
       elif order.is_new_live_order():
           self.pending_orders.push(order)   -- queued, nothing happens yet
       else:
           -- a rejection, or a `filled` status row -- ignored. (Fills are
           -- deliberately never replayed; see Order::is_cancellation's
           -- doc comment for why.)

   ... more add/load calls accumulate into pending_orders ...

User types:  clear

3. cli.rs calls fba.clear()                           [FbaOrderBook::clear]

       drain pending_orders
       candidate_prices()      -- every submitted limit price (§1.2)
       select_price()          -- three-tier winner (§1.2)
       eligible_orders()       -- price-time-priority sort, both sides
       sequential matching loop  ->  Vec<Trade>
       residual (unfilled) orders go back into pending_orders
       returns Option<ClearingResult>

4. cli.rs prints the clearing summary (price, trade count, how many
   orders rolled over) straight from the returned ClearingResult — no
   separate event log is involved.
```

### 2.3 CDA path — from `add`/`load` to an instant match

```
User types:  add buy 127 5 Alice          (or a `load`-sourced Order)

1. inputs/cli.rs builds an Order, same as the FBA path above.

2. cli.rs calls cda.submit(order)                    [CdaOrderBook::submit]

       if order.is_cancellation():
           self.cancel(order.oid)   -- remove from bids/asks if resting,
           return []                   see §2.4
       if not order.is_new_live_order():
           return []                -- rejection, ignored

       match order.side():
         Buy:  walk self.asks (best/cheapest price first) while
               check_price_match(taker=Buy) keeps crossing
                   -> Vec<Trade>, each priced at the MAKER's own price
               leftover limit quantity rests in self.bids
         Sell: walk self.bids (best/highest price first) while
               check_price_match(taker=Sell) keeps crossing
                   -> Vec<Trade>
               leftover limit quantity rests in self.asks

       returns Vec<Trade>  (may be empty — nothing crossed, or it fully
                             rested with no counterparty)

3. cli.rs prints how many trades cleared instantly, and at what price(s)
   — unlike FBA, every trade here can print at a different price, since
   each one prices at whichever maker it happened to match against.
```

### 2.4 Cancellation — how a `canceled` status row actually removes an order

A row whose `status_id` is one of the 8 cancellation-type codes (`canceled`
and 7 others — see `Order::is_cancellation`'s doc comment in `types.rs` for
the full list and why `filled` is deliberately excluded) is routed
differently by both engines' `submit()`:

```
FBA:  FbaOrderBook::cancel(oid)
          pending_orders.retain(|o| o.oid != oid)

CDA:  CdaOrderBook::cancel(oid)
          bids.retain(|o| o.oid != oid)
          asks.retain(|o| o.oid != oid)
```

Both are harmless no-ops if that `oid` was never live in this book, or
already cleared/matched away — `cancel` returns a `bool` saying whether it
actually found and removed anything, but `submit`'s internal call to it
doesn't need to check that: a cancel for an unknown `oid` is exactly as
valid an outcome as one that finds a match.

### 2.5 Where the two paths reconverge: metrics

Neither engine records anything anywhere for metrics purposes — there is no
central event log to replay or finalize. Every metric is a plain method
that recomputes its answer, on the spot, from whatever state the orderbook
already holds (`pending_orders` / `bids`+`asks` / `executed_trades`):

```
User types: metrics
    -> metrics::stats::print_summary(&fba, &cda)
         -> print_fba(&fba):  fba.quoted_spread_bps(), fba.depth_at_best(),
                               fba.trade_count(), fba.executed_volume(),
                               fba.executed_notional(), fba.fill_rate(),
                               fba.unexecuted_residual_share()
         -> print_cda(&cda):  cda.quoted_spread_bps(), cda.depth_at_best(),
                               cda.trade_count(), cda.executed_volume(),
                               cda.executed_notional(), cda.fill_rate(),
                               cda.book_imbalance()

User types: orderbook   (alias: ob)
    -> same idea, but scoped to whichever engine is currently active
       (current_mode), paired with that engine's own book/buffer display
       (render_pending / render_book)

User types: test engine <continuous|batch>
    -> inputs::test_suite::run_cda_tests() / run_fba_tests(), each
       constructing fresh, disposable orderbooks — nothing to do with the
       live session's own fba/cda instances
```

`metrics/stats.rs` never touches engine internals — every number it prints
comes from a `pub fn` already exposed on `FbaOrderBook`/`CdaOrderBook`. This
is the same "engines don't know metrics exist" boundary the design has
always had, just inverted from a push model to a pull one: instead of
engines pushing events out to a collector as they happen, `metrics::stats`
pulls numbers in on demand by calling public getters.

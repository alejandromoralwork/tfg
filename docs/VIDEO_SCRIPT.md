# `market_sim` — Full Codebase Walkthrough (Video Script)

**How to use this script**: it's written to be read aloud almost verbatim, with recording
cues in brackets telling you what to have on screen. `[SHOW: file.rs, lines A–B]` means open
that file and scroll to that range before speaking the line(s) under it — line numbers are
accurate as of this recording but drift as the code changes, so if a cue looks off, use it as
"roughly here" and re-anchor on the function/struct name instead. `[HIGHLIGHT: ...]` means
select or point at that specific piece of code while you say the next line. `[TERMINAL: ...]`
means switch to a terminal and run that command live. Chapters are ordered so nothing is used
before it's introduced — record them in order, or in one continuous take.

The full written reference this script is drawn from lives in `src/docs/` — mention it once
near the end ([`README.md`](src/docs/README.md) is the index) so viewers who want more depth
than the video has time for know where to go. Don't re-derive numbers on camera that are
already nailed down in `METRICS.md`/`TESTING.md`; just cite them.

**Total shape**: 11 chapters, roughly following the crate's own dependency order — `types.rs`
first because everything else depends on it, then the two engines, then the metrics system
that watches them, then the pipeline that drives real data through all of it, then the CLI that
ties it together, then testing, then deployment.

---

## Chapter 0 — Cold open

[SLIDE or terminal: project root directory listing]

"This is `market_sim` — a Rust project I built for my thesis comparing two ways a financial
exchange can match buy and sell orders. Today I'm going to walk through the entire codebase,
file by file, and explain not just *what* each piece does, but *why* it's built the way it is.

Here's the one-sentence version: real exchanges either match orders **continuously** — the
instant an order arrives, it's checked against the book and traded if possible — or they match
orders in **batches** — orders pile up for a fixed window, say one second, and then all clear
together at a single price. The first is called a Continuous Double Auction, or CDA. That's how
essentially every exchange you've ever used actually works. The second is a Frequent Batch
Auction, or FBA — it's a market-design idea, proposed as a way to remove the advantage of
being microseconds faster than everyone else.

My thesis asks: if you take the *exact same* real order flow — literally the same historical
orders, arriving at the same times — and run it through both mechanisms, what's actually
different? Not in theory. In practice, on real data.

To answer that, I needed two things: two matching engines built from scratch, faithful enough
to the real mechanics that a comparison means something, and a way to feed both of them
identical historical data and measure ~35 different microstructure metrics on the output —
spreads, depth, price impact, volatility, fill rates, all of it.

That's what this codebase is. It's one Rust binary. [SHOW: src/main.rs] Seventeen lines in
`main.rs`, four module declarations, and everything branches out from there:

```
main.rs
├── types.rs        — the shared vocabulary: Order, Trade, Side, PRICE_SCALE
├── engines/
│   ├── cda.rs       — the continuous engine
│   └── fba.rs       — the batch engine
├── metrics/
│   ├── stats.rs     — quick on-demand metrics for interactive use
│   └── timeseries.rs — the full streaming metrics pipeline
└── inputs/
    ├── cli.rs        — the interactive prompt / command dispatch
    ├── simulator.rs, binary_format.rs — reading the real dataset
    ├── simulate_cmd.rs — the main event: replays data through both engines
    ├── scan_cmd.rs, download_cmd.rs, update_cmd.rs, progress.rs, replay_checkpoint.rs
    └── test_suite.rs  — a built-in correctness checklist
```

The real dataset here is Hyperliquid's order flow — every order lifecycle event, including
rejected orders, for BTC, ETH, and SOL, at nanosecond precision, for the whole of December
2025. That's the fuel. Everything I'm about to show you exists to burn that fuel through two
engines and measure what comes out.

One architectural rule holds the whole thing together, and I'll point it out every time it
matters: **the two engines know nothing about metrics, and nothing about the CLI.**
`engines/cda.rs` and `engines/fba.rs` depend on exactly one file — `types.rs` — and nothing
else. `inputs/cli.rs` is the only file that reaches into both an engine and the metrics module
at the same time. That boundary is what makes it possible to reason about each engine in
isolation, and it's the first thing I'll point at once we're inside the code.

Let's start at the bottom of the dependency graph — the one file everything else is built on."

---

## Chapter 1 — `types.rs`: the shared vocabulary

[SHOW: src/types.rs, lines 1–10]

"`types.rs` depends on nothing else in this crate. Every other file either directly or
indirectly depends on it. So this is where I have to get the vocabulary right, because a
mistake here propagates everywhere.

### Fixed-point arithmetic, not floats

[HIGHLIGHT: line 4, `pub const PRICE_SCALE: u128 = 1_000_000;`]

"First decision, and it's a big one: **there is no floating-point price anywhere in this
crate.** Every price and every notional value is a `u128` integer, scaled by one million. So a
price of $127.06 isn't stored as the float `127.06` — it's stored as the integer
`127_060_000`. Why? Because floating-point arithmetic isn't associative — `(a + b) + c` and
`a + (b + c)` can give different results — and when you're summing millions of trade notionals
for a metrics report, those tiny errors compound into something that can actually change your
conclusions. Integer fixed-point math has none of that. It's exact, every time, and it's the
same convention research-grade trading systems actually use. `Amount` and `Price` are just
`u128` under different names — that's purely for readability, the compiler treats them
identically.

### The `Order` struct

[SHOW: src/types.rs, lines 40–70]

Every single order that ever touches either engine — whether it's a synthetic one you type into
the interactive prompt, or a real historical record decoded from a 54-byte binary file — ends
up as one of these `Order` structs. Most of the fields are a direct one-to-one mapping from the
raw dataset record: `ts` for timestamp, `oid` for order id, `user_id`, `is_ask` for side,
`limit_px`, `status_id`, and so on.

[HIGHLIGHT: `sz` and `orig_sz` fields, then the `remaining` field]

"But there's one field here that's different from all the others, and it's worth pausing on:
`remaining`. Every other field on this struct is just a copy of something from the raw record —
it never changes after the `Order` is built. `remaining` is different. It's mutated *live*, by
whichever engine is currently processing this order, every time a fill happens. It starts equal
to `orig_sz` — the order's original size — and gets subtracted from as fills occur. That's
deliberately kept separate from `sz`, which is just the record's own historical size snapshot
from the dataset, and is never touched by our engines. If I'd tried to reuse `sz` for the live
running total, I'd be conflating 'what the historical record says' with 'what our
independently-computed engine has done to this order' — two completely different things that
happen to often have the same value.

### The two predicates everything branches on

[SHOW: src/types.rs, lines 207–240]

"Now here's the pair of methods that basically every downstream file calls before doing
anything with an order: `is_new_live_order()` and `is_cancellation()`.

[HIGHLIGHT: `is_new_live_order`]

`is_new_live_order()` answers one question: does this record represent an order that should
actually enter the book? The dataset's `status_id` field has eighteen possible values —
`open`, `canceled`, `filled`, seven different rejection reasons, and so on. Only two of them
count as 'this order is now live': `status_id == 1` — meaning `open` — as long as it's not
still an un-triggered conditional order, or `status_id == 9`, meaning a conditional order that
just triggered. Everything else — every rejection, every fill, every conditional order that
hasn't fired yet — is *not* a new live order, and both engines will just drop it.

[HIGHLIGHT: `is_cancellation`]

`is_cancellation()` is the second gate. There are eight different status codes in this dataset
that all mean 'this order was canceled' for one reason or another — a plain trader-initiated
cancel, a reduce-only cancel, a self-trade-prevention cancel, a liquidation cancel, and so on.
This method returns true for exactly those eight.

Here's the detail I want to make sure lands, because it's a real design decision, not an
oversight: **`filled` — status code 5 — is deliberately excluded from `is_cancellation()`.**
Why would a fill *not* count as something to replay? Because a `filled` status in the real
dataset is Hyperliquid's own matching engine telling you what *it* decided to do with that
order. My engines are independently computing their own fills. If I replayed Hyperliquid's fill
events into my own engine, I'd be trying to reconcile two independent matching decisions against
each other — and that's not just messy, it defeats the entire point of the comparison. The
whole premise of this thesis is 'what happens if a *different* matching mechanism processes this
same order flow' — so the only things that should be replayed are events that represent genuine
trader intent, independent of which engine is doing the matching. A cancellation is trader
intent — 'I don't want this order anymore' — regardless of which engine is running. A fill is an
outcome of one specific engine's decisions. Only intent gets replayed.

### `Trade`

[SHOW: src/types.rs, lines 247–260]

One more struct here: `Trade`. Price, quantity, both counterparty ids, which engine produced it,
a timestamp, and a couple of optional blockchain fields left over from the real dataset's own
trade records that we don't otherwise use. The one subtlety: **`ts` means something different
depending on which engine produced the trade.** For a CDA trade, it's the aggressor's own
timestamp — the taker that caused the match. For an FBA trade, it's the maximum timestamp
across every order in that whole batch — because in a batch auction, there's no single
'aggressor,' there's a whole cohort of orders that all get priced together, and the trade itself
only really happens once the batch closes. Keep that distinction in your head — it matters again
when we get to the metrics system, because a few metrics use trade timestamps as their anchor.

That's the whole vocabulary. Nothing complicated on its own — but every one of these decisions
was made specifically so the two engines built on top of it can be compared fairly. Let's build
the first one."

---

## Chapter 2 — `engines/cda.rs`: the continuous engine

[SHOW: src/engines/cda.rs, lines 1–30]

"This is a real limit order book — the same conceptual structure every continuous exchange
uses. Let's start with how it's stored.

### The book's shape

[SHOW: src/engines/cda.rs, lines 12–35]

[HIGHLIGHT: `bids` and `asks` fields]

Two `BTreeMap<Price, VecDeque<Order>>` — one for bids, one for asks. A `BTreeMap` keeps its
keys sorted, so 'the best bid' is just whichever key is largest, and 'the best ask' is whichever
key is smallest — I don't need to scan anything to find the touch, I just ask the map for its
last or first entry. Within one price level, orders sit in a `VecDeque` — a double-ended queue —
in strict arrival order. That's price-time priority, straight out of the data structure: better
price always wins, and at the same price, whoever got there first gets filled first.

[HIGHLIGHT: `oid_index` field]

There's a third piece of state: `oid_index`, a hash map from order id to `(side, price)`. Why do
I need this separately, if the order is already sitting in one of those `VecDeque`s? Because
when a cancellation comes in, all I have is an order id — I don't know which side it's on or
what price level it's resting at. Without this index, canceling an order would mean scanning
every price level on both sides, which is `O(book size)`. With it, I look up the side and price
in constant time, then only need to search within that one small `VecDeque` — `O(log levels)`
overall. At real-data scale, with potentially tens of thousands of resting orders, that
difference matters.

### `submit()` — the heart of the engine

[SHOW: src/engines/cda.rs, lines 210–260]

This is the single most important function in this file. Let's walk it top to bottom.

[HIGHLIGHT: the `is_cancellation()` check at the top]

First thing it does: if this order is a cancellation, delegate straight to `cancel()` and return
an empty trade list. No matching logic runs at all for a cancel.

[HIGHLIGHT: the `is_new_live_order()` and `remaining == 0` check]

Next gate: if it's not a new live order, or its remaining quantity is already zero, bail out —
there's nothing to do. This is the same `is_new_live_order` predicate from `types.rs` — the
engine trusts that gate completely, it doesn't re-derive it.

[HIGHLIGHT: the matching loop]

Now the real work. We walk the *opposite* side of the book, best price first — if this is a buy
order, we look at asks starting from the lowest price; if it's a sell, we look at bids starting
from the highest. For each resting order we find, we call `check_price_match` — a market order
always crosses; a limit order crosses only if the taker's price is aggressive enough to overlap
the resting order's price. As long as that keeps returning true, we keep filling.

[HIGHLIGHT: the trade-price line — `get_price(&maker.kind())`]

Here's a detail that's easy to miss and genuinely important for the thesis comparison: **every
fill prices at the resting maker's own limit price** — not at the taker's price. That means if
one large market order sweeps through five different resting price levels, you get five separate
trades, potentially five different prices, all in one `submit()` call. The taker's price only
decided *whether* a level was eligible to trade against — once it's eligible, the maker who's
already resting there sets the price. That's exactly how a real continuous limit order book
works, and it's the mechanism that produces one of the CDA-side metrics later — realized price
dispersion within an interval isn't zero for CDA, the way it structurally is for FBA, precisely
because of this.

[HIGHLIGHT: the rest-or-discard branch at the end]

Whatever's left over after the matching loop: if the taker was a limit order, the leftover rests
in the book at its own price. If it was a market order, the leftover is just discarded — a
market order that doesn't find enough liquidity simply doesn't get the rest of it filled; it
never rests.

### `cancel()`

[SHOW: src/engines/cda.rs, lines 373–400]

Straightforward once you've seen `oid_index`: look up the side and price, find the order inside
that level's `VecDeque`, remove it, and if the level is now empty, remove the price key
entirely — that last part matters, because if empty price levels were left dangling in the
`BTreeMap`, 'best bid' could return a price with nothing actually resting there. Returns a bool:
did it actually find and remove something? A cancel for an oid that's already gone — because it
fully filled, or was never live in the first place — is a completely valid, harmless no-op.

### The getters, and two things worth flagging

[SHOW: src/engines/cda.rs, lines 480–530]

Everything from here down is read-only accessors — best bid, best ask, depth, spread, and so on.
Two of them deserve a specific callout because their names are slightly deceptive if you don't
read the code.

[HIGHLIGHT: `depth_at_best()`]

`depth_at_best()` on this struct sums `bid_depth` and `ask_depth` — which are **whole-book**
running totals, every resting order on both sides, not just the best price level. Despite the
name, this is not 'depth at the touch.' I want to flag that explicitly because the *metrics
timeseries* module, which we'll get to, has a separate, differently-computed field that's also
called `depth_at_best` — and that one really is touch-only. Same name, two different meanings,
in two different files, for reasons that made sense in each file's own context but that you
need to keep straight.

[HIGHLIGHT: `fill_rate()`, then the `total_filled_qty` accumulation in `submit()`]

`fill_rate()` divides `total_filled_qty` by `total_submitted_qty`. Here's the subtlety: every
time a trade happens, `total_filled_qty` increases by *quantity times two* — once for the buy
side's demand being satisfied, once for the sell side's supply being satisfied. That looks like
double-counting at first glance, but it's deliberate: it keeps the fill-rate denominator
symmetric across both sides of the market, so a book that's fully matched both ways reaches a
fill rate of 1.0, not 0.5. I actually tried deriving this by subtraction instead — 'total
submitted minus what's still resting' — the same trick the FBA engine uses — and it silently
breaks for one specific case: a market order that finds *zero* liquidity. It never rests, so it
never shows up in 'what's still resting,' but it also never filled — subtraction would wrongly
count it as filled. That's exactly the kind of bug that's invisible until you write a test for
precisely that scenario, which I did — `cda_market_order_partial_liquidity_fill_rate` — we'll
look at that test directly in the testing chapter.

That's the CDA engine. Instant matching, maker-priced fills, a real limit order book. Now let's
build its opposite."

---

## Chapter 3 — `engines/fba.rs`: the batch engine

[SHOW: src/engines/fba.rs, lines 1–30]

"Same job — take orders in, produce trades out — completely different mechanism. Where the CDA
matches the instant an order arrives, the FBA does almost nothing on submit. It just queues.

[SHOW: src/engines/fba.rs, lines 27–50]

[HIGHLIGHT: `pending_orders: Vec<Order>`]

The entire book, between clears, is one `Vec<Order>` — the batch buffer. `submit()` for a new
live order is one line: push it onto this vector. For a cancellation, it's `retain` — filter out
anything matching that order id. **Nothing executes on submit.** All the actual matching happens
in one function, called once per batch window: `clear()`.

### The uniform-price auction, step by step

[SHOW: src/engines/fba.rs, lines 91–130]

This is the algorithmic core of the whole batch-auction side of the thesis, so let's take it
slowly. The goal: pick **one single price** that every trade in this batch executes at, chosen
to maximize how much volume gets matched.

**Step one — candidate prices.**

[SHOW: src/engines/fba.rs, lines 257–295]

Only prices that were actually submitted as somebody's limit price are considered — never an
arbitrary price in between. That's safe, not just convenient: demand and supply, as functions of
price, are step functions that only change value at a submitted limit price. So the
volume-maximizing price is *always* achievable at one of the actually-submitted prices — you
never need to search the continuum in between. Market orders don't contribute a candidate at
all, since they don't carry a price.

**Step two — evaluate every candidate.**

[SHOW: src/engines/fba.rs, lines 371–424]

For each candidate price, I need `demand(price)` — total quantity from every buy order willing
to transact at that price or better, plus every market buy — and `supply(price)`, the sell-side
equivalent. This function, `demand_supply_evaluators`, is actually a rewrite of an earlier,
simpler version. The original approach rescanned the entire batch, from scratch, for every
single candidate price — if you have `N` orders you might have up to `N` candidate prices, so
that's `O(N²)` per batch clear. On a one-second FBA window over real high-frequency data, that
adds up fast. This version sorts the limit orders once, builds a cumulative-sum prefix array
once, and then answers 'how much demand qualifies at this price' with one binary search — a
`partition_point` call — per candidate. Same answer, `O(N log N)` instead of `O(N²)`. This is
exactly the kind of change that's invisible from the outside — the API didn't change, the
*results* didn't change — which is precisely why there's a whole suite of differential tests
that check this optimized version against a dead-simple, deliberately-naive reference
implementation across hundreds of randomized batches, to prove the speedup didn't silently
change behavior. We'll see that test in the testing chapter.

**Step three — pick the winner.**

[SHOW: src/engines/fba.rs, lines 296–370]

[HIGHLIGHT: the three comparison tiers inside `select_price`]

Three tiers, in strict priority order. Tier one: maximize matched volume — that's
`min(demand(price), supply(price))`. Tier two, only if there's a tie on volume: minimize the
imbalance, `|demand − supply|` — prefer the price that leaves fewer orders stranded. Tier three,
only if *that's* also tied: prefer whichever candidate is closer to `last_clearing_price` — the
price this same engine last actually cleared a trade at. That third tier exists purely for
price continuity — without it, if two prices are mathematically equivalent, the clearing price
could jump around arbitrarily between ticks for no economically meaningful reason. It's the same
convention real call auctions use — an opening cross typically references the prior session's
close. If there's no clearing history at all yet, the tie-break falls through to simply the
lower of the two prices, deterministically.

**Step four — the rationing walk.**

[SHOW: src/engines/fba.rs, lines 425–460]

Once we have a winning price, if demand and supply aren't exactly equal, one side has to be
rationed. Both sides get sorted by `eligible_orders` and `order_priority` — most aggressive
price first, market orders ahead of every limit order, and ties broken by earliest submission
time. Then it's a simple sequential walk: fill from the front of both sorted lists until one
side runs out. Price-time priority falls straight out of that sequential walk — there's no
separate rationing algorithm needed on top of it.

[SHOW: src/docs/ENGINE_DESIGN.md, §1.3 — the worked example]

Let me make this concrete with the worked example from my engine-design notes. [Walk the
`ENGINE_DESIGN.md` table on screen: B1 buy 10 @105, B2 buy 10 @100, B3 buy 10 @100, S1 sell 15
@100.] Candidates are 105 and 100. At 105, only B1 qualifies on the buy side — demand is 10,
supply is 15, so matched volume is 10. At 100, all three buys qualify — demand is 30, supply is
still 15 — matched volume is 15. A hundred wins outright, no tie-break needed, because it
strictly maximizes matched volume. Now the walk: B1, the best-priced buyer, matches all 10 units
against S1. S1 has 5 left. Between B2 and B3 — tied at exactly 100 — B2 arrived first, so B2 gets
those remaining 5. B3, despite being economically willing to trade at 100, gets zero, purely
because it was third in line at that price. That's rationing, and it's exactly what a real
call auction does.

### What this deliberately does *not* do

[HIGHLIGHT: the all-market-batch guard inside `clear()`]

One more thing worth a full callout, because it was an actual bug I found and fixed, not a
design choice I got right the first time: what happens if an entire batch is nothing but market
orders? There's no limit price anywhere in it, so `candidate_prices` returns an empty set. An
earlier version of this code fell back to pricing the batch at `last_clearing_price` in that
case — which sounds reasonable, but is actually wrong: it means pricing today's market orders
off of potentially several batches' worth of stale history, with zero information from the
current batch itself. The current version does something different: with no candidates at all,
`clear()` doesn't invent a price — it restores the entire batch back into `pending_orders`
untouched and returns `None`. The batch just rolls forward to the next window rather than
clearing on a guess. There are two tests locked in around exactly this — one proving it happens
correctly with no history at all, one proving that having stale history present *doesn't* change
the outcome. We'll look at both later.

Also worth saying explicitly, because it shapes how to interpret the FBA-side metrics later:
there's no external liquidity source here. No AMM, no market maker of last resort. If one side
outweighs the other, the excess volume simply doesn't trade — it rolls over. That's a deliberate
modeling choice, not a missing feature: the whole point is comparing two *order-driven*
mechanisms against each other on equal footing.

That's both engines. Same input type, same output type, completely different internal
mechanics — one instant and path-dependent, one delayed and uniform-price. Now let's see how we
actually measure the difference between them."

---

## Chapter 4 — The metrics system: `metrics/stats.rs` and `metrics/timeseries.rs`

[SHOW: src/metrics/mod.rs]

"Two files, two completely different philosophies, and the split is deliberate.

### `stats.rs` — the pull model

[SHOW: src/metrics/stats.rs, lines 18–52]

This one's quick. `stats.rs` has zero calculation logic of its own. Every single number it
prints is read straight off a public getter that already exists on `FbaOrderBook` or
`CdaOrderBook` — `quoted_spread_bps()`, `depth_at_best()`, `fill_rate()`, and so on. It exists
purely to support the interactive prompt's `metrics` and `orderbook` commands — you're sitting
at the REPL, you want a snapshot of where things stand *right now*, and this just pulls the
current state and prints it. No event log, no history — a pure 'ask the engine what it currently
knows.'

### `timeseries.rs` — the push model

[SHOW: src/metrics/timeseries.rs, lines 1–100]

This is the file that actually produces the 35-column CSV that the whole empirical half of the
thesis runs on. Completely different architecture: instead of asking the engines a question on
demand, this file **keeps an event log** as the `simulate` command streams real data through
both engines, and periodically folds that log into per-interval rows.

[HIGHLIGHT: the four event structs — `OrderMessage`, `TradeEvent`, `BatchClearedEvent`,
`BookSnapshot`]

Four event types get recorded. `OrderMessage` — every single record from the dataset, recorded
*before* any accept-or-reject gating, so metrics like the order-to-trade ratio can see the whole
stream, rejections included. `TradeEvent` — wraps a `Trade` with two extras: `reference_price`,
the pre-trade midpoint for CDA or the last clearing price for FBA, and `aggressor_side`, which is
always `None` for FBA trades because there's no taker-versus-maker concept in a uniform-price
batch. `BatchClearedEvent` — one per FBA batch window, whether or not it actually found a
crossing price. `BookSnapshot` — periodic CDA book state.

[SHOW: src/metrics/timeseries.rs, line 287 — `bucket_of`]

Every one of those events gets stamped into a time bucket by `bucket_of` — a timestamp maps to
the start of whichever `τ`-wide interval it falls in, where `τ` is the `simulate` command's
interval argument, one second by default. That's the row grid the whole CSV is built on.

### A representative walk through the metric formulas

I'm not going to read all 35 columns to you one at a time — that would be a spreadsheet, not a
video. The complete, formula-by-formula reference for every single column is in
[`METRICS.md`](src/docs/METRICS.md) — I'll cite it here for anything I skip. What I want to do
instead is show you the handful that best illustrate how this system actually works, and why a
few of them needed real care.

[SHOW: src/metrics/timeseries.rs, line 989 — the CDA `quoted_spread_bps` assignment]

**`quoted_spread_bps`** — for CDA, this is the mean, over every book snapshot in the interval, of
`(ask − bid) / mid × 10,000`. For FBA there's no live bid-ask spread at all — there's no
continuously-updating book — so it's approximated instead from the gap between the best unfilled
buy and the best unfilled sell in the pending batch, referenced against the clearing price. Same
column name, same units, genuinely different computation per engine — that's a pattern that
repeats through this file, and it's worth calling out every time, because comparing the two
numbers requires understanding they're not measuring identically-defined things.

[SHOW: src/metrics/timeseries.rs, lines 600–650 — `price_at_or_after` and the realized-spread
horizon loop]

**`effective_spread_bps` → `realized_spread_bps` → `price_impact_bps`.** Effective spread
measures how far the trade price deviated from the pre-trade reference price, signed by which
side was the aggressor. Realized spread measures the same deviation, but against the reference
price some number of seconds *after* the trade — one, five, or thirty seconds out — which
represents the part of that spread the liquidity provider actually got to keep, once the market
had time to react. Price impact is just the difference between the two: the part of the spread
that got competed away by adverse price movement. All three share one lookup helper,
`price_at_or_after`, a binary search over a combined series of CDA midpoints and FBA clearing
prices — and there's a specific tie-break rule in there: if a book snapshot and a batch clear
land at the exact same timestamp, the book snapshot wins, and there's a dedicated test locked in
on exactly that tie.

[SHOW: src/metrics/timeseries.rs, around line 790 — the Amihud calculation]

**`amihud_illiquidity`.** This is the canonical Amihud 2002 measure — the absolute return divided
by dollar volume, times a million by convention. What makes it interesting implementation-wise is
that it's the first metric here that spans bucket boundaries: it needs the *previous* bucket's
closing price, which might have been computed and flushed to disk in an earlier pass, long
before this bucket exists. That's threaded through a small `Carry` struct — `prev_close` — that
gets passed in and out of every flush and even gets persisted to the checkpoint file, so a
resumed run doesn't lose it.

[SHOW: src/metrics/timeseries.rs, lines 727–740 and 848 — the two `kyle_lambda` constructions]

**`kyle_lambda`** gets the deepest dive, because it's built completely differently for the two
engines, and that difference is itself a research finding, not an implementation detail. Both
constructions are an ordinary least-squares regression through the origin — slope equals the sum
of `x` times `y`, divided by the sum of `x` squared, accumulated as two running sums, no data
retained. For CDA, one observation per taker *sweep* — a marketable order that crosses several
price levels in one shot is grouped into a single observation, since all its fills share a
timestamp, a side, and a pre-trade midpoint. `x` is the net signed executed quantity; `y` is the
relative midpoint move five seconds later. On real data, this comes out **positive** and
meaningfully so — buy sweeps measurably lift the mid. For FBA, one observation *per priced
batch* instead of per trade: `x` is the net order flow — buy quantity minus sell quantity across
the whole batch, captured *before* price selection — and `y` is the percentage change in
clearing price from the previous priced batch. On real one-second data, this comes out **close
to zero**. That's not a bug, and it's not noise — it's the actual behavior of the
volume-maximizing, price-continuity-anchored selection rule: it makes the clearing price close
to indifferent to raw flow imbalance. That near-zero result is one of the more interesting
comparative findings between the two mechanisms, and it falls straight out of the algorithm we
walked through in the last chapter.

The remaining columns — depth-within-basis-point bands, realized volatility, trader surplus,
order size inflation, throughput, and so on — follow the same pattern: a running accumulator per
bucket, mostly `O(1)` per event, a couple with a documented `None`-versus-zero distinction that
matters (an interval with zero volume gets `None`, not `0.0`, because they mean different
things). Every one of them is spelled out formula-by-formula in `METRICS.md` if you need the
exact expression.

### Streaming, not one-shot

[SHOW: src/metrics/timeseries.rs, line 445 — `emit`]

One last architectural point before we move on, because it explains a design decision you'll see
again in the next chapter. `emit()` doesn't wait until the whole run is done. It flushes every
bucket up to a given watermark, folds it into the CSV, then *prunes* — drops every event whose
bucket is now finalized — so memory usage stays bounded to roughly one input file's worth of
activity, not the entire multi-day run. That only works if streaming the output in pieces
produces byte-for-byte the same numbers as computing everything in one giant pass at the end —
and there's a dedicated differential test, for both CDA and FBA, that proves exactly that
equivalence across a long randomized event stream. We'll look at it in the testing chapter.

That's the measurement half of this project. Now let's look at how real data actually gets fed
into all of this."

---

## Chapter 5 — The intake layer: `binary_format.rs` and `simulator.rs`

[SHOW: src/docs/SCHEMA.md — the binary record table]

"Before any of the code we've seen so far can run on real data, that data has to get decoded
out of Hyperliquid's own storage format. There are two intake paths, and they matter for very
different reasons.

### `binary_format.rs` — decoding the real archive

[SHOW: src/inputs/binary_format.rs, lines 35–65]

The real dataset ships as a custom packed binary format — exactly 54 bytes per record, no
delimiters, concatenated one after another, so the file size divided by 54 gives you the exact
record count. Prices and quantities inside it use their own bit-packed fixed-point encoding:
the top three bits of a 32-bit field say how many decimal places the value has, and the
remaining 29 bits are the integer value. `decode_price` unpacks that. Let me show you the worked
example from the dataset's own schema: [SHOW: src/docs/SCHEMA.md, the `$96,543.21` example] a
Bitcoin price of $96,543.21 has two decimal places and an integer value of 9,654,321 — pack those
into the top three and bottom 29 bits of a `u32`, and you get the exact encoded value the real
archive stores.

[HIGHLIGHT: `decode_signed_price`]

There's a signed variant of the same scheme for one field, `triggerCondition`, which needs a
sign — is the trigger price above or below the current price. Same decimal-place-count idea, but
one of the bits is repurposed as an explicit sign flag rather than using two's-complement.

[SHOW: src/inputs/binary_format.rs, lines 84–158 — `parse_record`]

`parse_record` takes one raw 54-byte array and reads every field at its exact fixed byte offset —
timestamp at bytes 0 through 8, user id at 8 through 12, and so on — decodes each one, and
produces a `types::Order`. One thing worth flagging: this function deliberately leaves the
human-readable status/order-type/time-in-force label strings as `None` rather than resolving
them. Resolving those costs a heap allocation per record, and at the scale this runs at —
potentially tens of millions of records per file — that allocation adds up to real time, for
labels neither engine actually reads. They only read the raw numeric ids.

[HIGHLIGHT: `looks_like_order_status_record`]

One more function worth a mention: `looks_like_order_status_record`. It's not a real format
signature — it's a cheap plausibility check. Is the timestamp somewhere between 2020 and 2035?
Are the status, order-type, and time-in-force ids within their known documented ranges? If a
wrong file somehow ends up in the input directory, this catches it after reading just the first
record, instead of silently producing millions of garbage `Order`s or panicking deep into a
multi-gigabyte file.

### `simulator.rs` — two ways in

[SHOW: src/inputs/simulator.rs, lines 301–330 — `collect_input_files`]

This file has two completely separate intake paths, and the difference matters for scale.

The first path, `load_order_status_csv`, is for small pre-decoded CSV preview files — the kind
you'd feed the interactive `load` command by hand. It reads the whole file into memory at once.
Fine for a few hundred rows. Not fine for the real archive.

The second path is the streaming one, and `collect_input_files` is where it starts. It walks a
directory tree — the dataset's own `date/coin_hour.data.gz` layout — and sorts the file list.
That sort does two jobs at once, and the second one is a small trick worth pointing out:
lexicographic sorting naturally gives chronological order by date and hour, *and*, because the
character `.` sorts before `_` in ASCII, it also happens to put `sol_00.data.gz` before
`sol_00_rejected.data.gz` — accepted-file records land ahead of rejected-file records within the
same hour, purely as a side effect of plain string sorting, with zero special-case code needed.

[HIGHLIGHT: `stream_file`, the `CountingReader` wrapper]

From there, everything streams — `stream_file` opens one file, wraps it in a gzip decoder if
needed, and calls a per-record callback as it decodes, never materializing more than one file's
worth of bytes in memory at a time. There's a `CountingReader` wrapper sitting *underneath* the
gzip decompressor specifically so the live progress bar can report physical, on-disk compressed
bytes read, not decompressed bytes — those two numbers can differ by an order of magnitude, and
the progress percentage needs to track against the number that actually matches the file size on
disk.

[SHOW: src/inputs/simulator.rs, lines 490–520 — `peek_first_ts`]

One more function that becomes important in the very next chapter: `peek_first_ts`. It decodes
just enough of a file — up to 256 records — to find the timestamp of the very first real record,
without reading the whole file. That sounds like a minor convenience function, but it's actually
load-bearing for how the `simulate` command decides when it's safe to write output — which is
exactly where we're headed next."

---

## Chapter 6 — `simulate_cmd.rs`: the centerpiece

[SHOW: src/inputs/simulate_cmd.rs, lines 76–90 — `pub fn run`]

"If there's one file in this entire project that represents the actual empirical machinery of
the thesis, it's this one. `simulate <path|coin> [interval]` takes real historical data, streams
it through *fresh, isolated* instances of both engines, and produces the time-series CSVs that
everything else in the project exists to compute.

[HIGHLIGHT: the doc comment above `run`]

Exit codes first, because they matter for how this gets deployed later: zero means success, one
means a run-time failure — I/O trouble, something transient, safe to just retry — and two means a
bad request — no data found, an incompatible checkpoint — retrying won't help, you need to fix
something first. That distinction becomes very relevant once we get to how this runs inside
Google Cloud Batch.

### The per-record closure

[SHOW: src/inputs/simulate_cmd.rs, lines 280–370]

This is the hot loop — the function that runs once for every single decoded order in the entire
archive, potentially hundreds of millions of times per coin. Let's walk it in the exact order it
executes.

[HIGHLIGHT: line 293 — `set_anchor` on both recorders]

On the very first record of the entire run, both metrics recorders — FBA's and CDA's — get their
time-bucket grid anchored to that record's timestamp, at the same moment. That's deliberate:
both engines need to share an identical grid origin, so a bucket labeled '10:00:05 to 10:00:06'
means the exact same window on both output CSVs, letting you line them up side by side later.

[HIGHLIGHT: line 300 — the `OrderMessage` push]

Next, unconditionally, before any accept-or-reject decision: push an `OrderMessage`. Every
single record, live or rejected, gets logged here. This has to happen before the gating below,
because metrics like the order-to-trade ratio need to see the *whole* stream, rejections
included, not just what made it into a book.

[HIGHLIGHT: the FBA boundary-crossing `while` loop]

Then, FBA interval boundaries: if this record's timestamp has crossed the next `τ`-wide
boundary, fire `clear_fba_batch`. Notice it's a `while`, not an `if` — if there's a quiet period
in the data with no records for several seconds, one record could cross multiple boundaries at
once, and each one still needs its own batch clear and its own `BatchClearedEvent`, even if that
batch turns out to be empty.

[HIGHLIGHT: line 325 — `is_actionable`]

Now the gate that decides whether either engine actually does anything with this record: is it a
new live order, or a cancellation? If it's neither — a rejection, a fill, an un-triggered
conditional — both engines would no-op on it anyway, so we skip the clone and the call entirely
rather than paying that cost for nothing.

[HIGHLIGHT: lines 330–340 — both engines fed]

If actionable: `fba.submit()` gets a clone of the order. Then, *before* calling into the CDA
engine, we capture the pre-trade midpoint — that becomes `reference_price` for any trade this
record produces. Then `cda.submit()` runs, timed with `Instant::now()`, because that timing
feeds directly into the `avg_clearing_latency_micros` metric.

[HIGHLIGHT: the depth-schedule recomputation]

One more efficiency detail: the bps-banded depth schedule — how much resting volume sits within
10, 50, and 100 basis points of the mid — is an `O(book size)` scan. It only gets recomputed when
the record was actionable, and the previous cached value is reused otherwise. Recomputing it on
every single record, including the ones that changed nothing, would be wasted work at real-data
scale.

Then a `BookSnapshot` gets recorded, and the loop moves to the next record.

### The flush cadence — why 35 seconds

[SHOW: src/inputs/simulate_cmd.rs, line 71 — `MARKOUT_GUARD_SECS`]

Here's a question worth asking on camera, because the answer is genuinely clever: this run might
process a multi-day archive with hundreds of millions of records. When does it actually write
anything to disk? Not at the end — that would mean holding the entire run's event history in
memory, which defeats the whole streaming design. Instead, output gets flushed **once per input
file**, and the watermark for how far it's safe to flush is computed by `flush_hi`.

[SHOW: src/inputs/simulate_cmd.rs, lines 513–528 — `flush_hi`]

The logic: a metrics bucket can only be safely finalized once every forward-looking value it
might need has actually been observed. The widest forward horizon anywhere in the metrics system
is the 30-second realized-spread markout. So `MARKOUT_GUARD_SECS` is set to 35 — 30 plus the
5-second Kyle's-lambda horizon, plus a small margin — and a bucket only gets flushed once
event-time has moved at least that far past *both* this file's own last timestamp and the very
first timestamp of the *next* file. That second condition is what `peek_first_ts`, from the last
chapter, exists for — one cheap read of the next file's first record, instead of opening the
whole thing, just to confirm no later record could still land in an already-flushed bucket.

### Resume and checkpointing

[SHOW: src/inputs/replay_checkpoint.rs, lines 61–100 — the `Checkpoint` struct]

This whole pipeline is designed to survive being killed and restarted — which matters a lot once
it's running unattended in the cloud for potentially thirteen hours. After every single input
file, a `Checkpoint` gets rewritten atomically to disk: which files are done, the bucket grid's
cursor position, the cross-flush carry values for Amihud and Kyle's lambda, and running summary
accumulators. Re-running the same command later loads that checkpoint, skips whatever's already
done, and picks up where it left off.

[HIGHLIGHT: the doc comment about approximate resume]

One honest caveat, stated plainly in the code and worth saying out loud here too: resume is
*approximate*. The engine books themselves — every resting order — and the in-flight event
window are **not** persisted, only the metrics grid state. So a resumed run leaves a short gap of
empty rows, roughly one file's worth, right at the seam where it resumed — unless nothing had
been flushed yet at all, in which case it just restarts cleanly from the top. That's a real,
documented limitation, not a silent one.

### The supporting cast

Four more files round out this pipeline, more briefly:

[SHOW: src/inputs/scan_cmd.rs, lines 41–51]

`scan_cmd.rs` streams the exact same archive but with a cheap tallying closure instead of either
engine — how many records, how many are new live orders, how many cancellations, using the exact
same predicates from `types.rs`. No book computation at all, so it can safely run multi-threaded,
splitting files round-robin across worker threads — order doesn't matter for a tally the way it
matters for a strictly sequential replay.

[SHOW: src/inputs/download_cmd.rs, lines 32–40, 183–200]

`download_cmd.rs` fetches an archive from Zenodo via `curl`, then extracts it in two deliberately
separate steps — decompress with `xz`, then untar with `tar` — rather than one combined command,
because Windows' bundled tar has no native LZMA support and shells out to an external `xz`
internally, a combination that was observed to genuinely deadlock on multi-gigabyte archives.

[SHOW: src/inputs/update_cmd.rs, lines 37, 169]

`update_cmd.rs` self-updates without needing `git` installed at all — downloads a plain tarball
snapshot from GitHub, rebuilds in a fresh temp directory, then relaunches. The whole design
exists to sidestep the classic 'can't overwrite a running executable' problem: nothing about the
currently-running binary ever gets touched until the very last step, a clean handoff to the newly
built one.

[SHOW: src/inputs/progress.rs, line 105]

And `progress.rs` is the shared live progress bar every one of these long-running commands uses —
a background thread polling a shared counter roughly once a second, redrawing in place on a real
terminal, or printing one line per update when output is piped to a log file, so container logs
stay readable as a scrolling series of snapshots instead of one garbled overwritten line.

That's the entire data pipeline, end to end — from a raw byte on disk in a Hyperliquid archive,
to a finished, resumable, crash-safe CSV row. Now let's look at how a person — or a script —
actually drives all of this."

---

## Chapter 7 — `inputs/cli.rs` and `main.rs`: tying it together

[SHOW: src/main.rs]

"Let's close the loop. `main.rs` is seventeen lines. No arguments: launch the interactive
prompt. With arguments: run exactly one command and exit with a status code. That's the entire
branching logic for the whole program's entry point — everything else lives in `cli.rs`.

### Parsing a command line

[SHOW: src/inputs/cli.rs, lines 39–63]

Every command the interactive prompt understands is one variant of the `CliCommand` enum —
`Add`, `Engine`, `Simulate`, `Download`, `TestEngine`, and so on.

[SHOW: src/inputs/cli.rs, lines 63–70]

`CliCommand::parse` takes one line of typed input and does three things: strips a leading
UTF-8 byte-order mark — because a PowerShell pipe can silently prepend one to just the first
line, and plain `trim()` doesn't catch it, since it's not whitespace — splits on any run of
whitespace, and dispatches on the lowercased first word. Every command below that has its own
small parsing rules and its own usage-error message if the arguments don't fit — I won't read
every one, they follow the same shape, but it's worth opening `TESTING.md` here if you want to
see all fifteen of this file's own parser tests, because between them they lock in every alias
and every malformed-input case.

### `run()` versus `run_once()`

[SHOW: src/inputs/cli.rs, line 388 — `pub fn run`]

`run()` is the interactive REPL. It owns exactly one live `FbaOrderBook` and one live
`CdaOrderBook` for the whole session — that's what makes commands like `add`, `clear`, and
`metrics` meaningful as a connected sequence: you're building up state across multiple commands
against the same two books.

[SHOW: src/inputs/cli.rs, line 570 — `pub fn run_once`]

`run_once()` is completely different in spirit: it's for a single one-shot invocation from a
shell script or a container — `market_sim simulate sol 1`, say — and it only wires up the
commands that don't need that persistent cross-command state: `simulate`, `scan`, `download`,
`extract`, `update`, `help`, and `test engine`. If you try to run something stateful like `add`
through this path, it's explicitly rejected with a message telling you to launch the interactive
prompt instead.

[HIGHLIGHT: the exit-code logic inside `run_once`]

And this is where that exit-code convention from `simulate_cmd.rs` actually gets consumed: zero
for success, one specifically reserved for 'a `test engine` checklist had a failing case,' and
two for every kind of usage or argument error — missing an argument, an unknown coin, an
unrecognized command entirely. That's what makes `market_sim test engine all` a meaningful thing
to run inside a Docker container with no Rust toolchain at all — the exit code alone tells a CI
pipeline or a deployment script whether every correctness check passed.

That's the whole crate, wired end to end — from `main.rs`'s four module declarations, through
two independently-built matching engines, through a metrics system watching both of them, through
a streaming pipeline that can replay a multi-day real archive, to a command layer that drives all
of it either interactively or as a scriptable one-shot binary. Now — how do I know all of this
actually works?"

---

## Chapter 8 — Testing: two systems, and why both exist

[SHOW: src/docs/TESTING.md — the comparison table near the top]

"There are two completely separate, non-overlapping ways this project verifies itself, and I
want to explain why there are two before showing you either.

`cargo test` is eighty ordinary Rust unit and differential tests, spread across eleven files,
checked in as inline `#[cfg(test)]` blocks. It needs the Rust toolchain to run, it's fast, and
it's aimed at the *plumbing* — parsers, the binary decoder, the metric formulas, and a set of
differential tests that fuzz an optimized data structure against a deliberately naive reference
implementation across hundreds of randomized scenarios.

`test engine <target>` is something different: thirty-seven hand-built checklist cases, compiled
directly into the production binary — not gated behind `#[cfg(test)]` at all — so it runs with
**no Rust toolchain whatsoever**. That's what lets it serve as the smoke test inside the Docker
image itself: `docker run --rm market_sim test engine all` exits zero only if every single case,
across both engines and the entire metrics catalogue, actually passed.

Neither one is a superset of the other. `cargo test` never runs a full multi-order matching
scenario end to end and checks it against a hand-computed expected outcome. `test engine` never
touches the CSV parser or the checkpoint file format. You genuinely need both.

### A few representative cases, in depth

I'm not going to read all eighty tests and all thirty-seven checklist cases to you — that's what
`TESTING.md` is for, and it's exhaustive. What I want to show you instead are a handful of cases
that were written specifically because I found a real bug, and I think seeing the bug is more
instructive than seeing the passing test in isolation.

[SHOW: src/inputs/test_suite.rs, lines 372–405 — `cda_market_order_partial_liquidity_fill_rate`]

This one locks in the `fill_rate` subtlety from Chapter 2. The scenario: one trader rests six
units for sale, a second trader market-buys exactly those six units — fully filled — and then a
*third* trader submits a market buy for three units into a now-empty book. That third order
vanishes, unfilled, having never rested at all. Total submitted across all three: fifteen units.
Total actually filled: twelve. The hand-computed expected fill rate is `12 / 15`, exactly `0.8`
— not `1.0`. If `fill_rate` had been implemented via subtraction — 'submitted minus what's still
resting' — that vanished, never-rested market order would be invisible to the formula entirely,
and the result would wrongly come out as `1.0`. This test is the reason `CdaOrderBook` tracks
`total_filled_qty` as a direct running total instead.

[SHOW: src/inputs/test_suite.rs, lines 406–430 — `cda_sell_crosses_bid_only_at_or_below_bid_price`]

Here's a real matching-direction bug this test caught. A resting bid at 100. Then an aggressive
seller offers at 90 — below the bid, so it correctly crosses and trades at the bid's own price of
100. Then a *passive* seller offers at 105 — above the bid — and this one must **rest**, not
cross. It sounds obvious stated out loud, but a sign or comparison-direction error in the price-
matching condition is exactly the kind of bug that's easy to introduce during a refactor and easy
to miss in code review, because both branches look almost identical. This test exists specifically
to pin the *direction* of that comparison down.

[SHOW: src/inputs/test_suite.rs, lines 675–695 — `fba_all_market_no_history_preserves_orders`]

This is the batch-engine bug I described in Chapter 3 — an all-market batch, with no clearing
history yet, used to silently price itself off of stale history from several batches back. This
test submits two pure market orders, buy and sell, with zero prior clearing history, calls
`clear()`, and asserts the result is `None` — and, just as importantly, that both orders are
still sitting in `pending_orders` afterward, not lost. There's a companion test right after it,
`fba_all_market_with_history_still_rolls_over`, that seeds some clearing history first and proves
having that history present doesn't change the outcome either — the fix isn't 'don't use history
when there's none,' it's 'never use history for an all-market batch, full stop.'

[SHOW: src/inputs/test_suite.rs, lines 824–840 — `fba_tie_with_history_picks_closest_price`]

And this one exercises the third tie-break tier from the price-selection algorithm directly. Two
candidate prices, 90 and 100, that tie exactly on both matched volume and imbalance — zero
either way. With clearing history seeded at 97, the tie-break has to pick 100, because it's
closer to 97 than 90 is. Flip that history to a different seed value and the winning price
flips too. This is the test that proves the price-continuity rule from Chapter 3 isn't just
described in a doc comment — it's actually load-bearing in the code.

### The rest, by the numbers

Beyond these four: the CDA checklist has fifteen cases total, covering resting orders, exact and
partial fills, price-versus-time priority, market order behavior, cancellation replay, and one
full hand-computed metrics scenario. The FBA checklist has fourteen, covering the full clearing
algorithm, residual rollover, and cancellation. The metrics checklist has eight, each driving
`MetricsRecorder` directly with synthetic events and checking every relevant CSV column against a
hand-worked value — including, notably, a case that engineers a *known* Kyle's-lambda slope of
exactly 2.0 for CDA and 125.0 for FBA, and asserts the regression recovers that exact number.

On the `cargo test` side: eighty tests across eleven files, four of them marked `#[ignore]`
because they need either the bundled real sample data or real `tar`/`xz` executables on the
machine — run those explicitly with `cargo test -- --ignored`. And there's one file worth calling
out for having *zero* test coverage of any kind: `update_cmd.rs`, the self-update command. It's
the one piece of this project I haven't found a good way to test in isolation, since its entire
job is downloading, rebuilding, and relaunching the actual binary.

[TERMINAL]

```
cd src
cargo test                      # the 76 non-ignored unit and differential tests
cargo test -- --ignored         # + the 4 that need real sample data or tar/xz
```

[TERMINAL, inside the running binary]

```
test engine batch
test engine continuous
test engine metrics
test engine all
```

Full reference for every single one of these, case by case, formula by formula: `TESTING.md`."

---

## Chapter 9 — Deployment, briefly

[SHOW: Dockerfile]

"Quick chapter — this is more operations than code, so I'll keep it short and point you at
`DEPLOY.md` for the complete walkthrough.

The project ships a multi-stage `Dockerfile`. The entrypoint is the `market_sim` binary itself,
so a container command like `["simulate", "sol", "1"]` becomes exactly
`market_sim simulate sol 1` — the same non-interactive path from Chapter 7. Before anything gets
deployed, the image gets sanity-checked with `docker run --rm <image> test engine all` — that's
the runtime checklist from the last chapter, doing real work here: proving the built binary is
correct with zero Rust toolchain inside the container at all.

For anything longer than a quick local run, there's a full Google Cloud Batch job spec in
`deploy/batch-sol.json` — the dataset mounted read-only from one bucket via gcsfuse, output
written to another, and the whole thing wired to retry automatically on a transient failure or a
Spot VM preemption. That retry story only works *because* of the checkpoint-and-resume design
from Chapter 6 — a retried task picks up `checkpoint.txt` from the output bucket and continues
exactly where the last attempt left off, rather than starting a potentially thirteen-hour run
over from scratch. `DEPLOY.md` walks through every step of that setup for someone who's never
touched Google Cloud before — I'd rather point you there than re-narrate a JSON config file on
camera."

---

## Chapter 10 — Closing: how it all fits together

[SHOW: the module-tree slide from Chapter 0, or src/docs/ARCHITECTURE.md's module map]

"Let's zoom back out to the whole picture one more time. `types.rs` at the bottom, depending on
nothing. Two engines built on top of it, `cda.rs` and `fba.rs`, each depending only on
`types.rs` and nothing else — not on metrics, not on the CLI, not on each other. A metrics layer
watching both engines from the outside, in two different modes: `stats.rs` pulling snapshots on
demand, `timeseries.rs` pushing a full streaming event log into a 35-column catalogue. A pipeline
in `inputs/` that can feed either a handful of typed orders or a multi-day real archive into that
whole stack. And a CLI on top of all of it, `cli.rs`, which is — deliberately — the *only* file
in this entire crate that ever reaches into both an engine and the metrics module at the same
time.

That one architectural rule is the thing I'd want you to remember if you remember nothing else
from this video: **the engines don't know metrics exist, and they don't know a CLI exists.**
Every time I needed to add a new command, or a new metric, I never had to touch either engine
file to do it — because neither one was ever written with any awareness that either of those
things existed in the first place. That boundary is what let me build, test, and reason about
the CDA and FBA engines completely independently, which is the entire methodological premise this
thesis's comparison depends on.

If you want to go deeper than this video had time for — the exact formula behind any one of the
35 metrics, the complete signature and algorithm for any function in the crate, or every single
test case spelled out with its hand-computed expected value — the full written documentation set
lives in `src/docs/`, starting at [`README.md`](src/docs/README.md). That's everything. Thanks
for watching."

---

## Appendix — quick cue index

For re-recording a single chapter without scrubbing through the whole script:

| Chapter | Primary files |
|---|---|
| 0. Cold open | `main.rs` |
| 1. `types.rs` | `src/types.rs` |
| 2. CDA engine | `src/engines/cda.rs` |
| 3. FBA engine | `src/engines/fba.rs`, `src/docs/ENGINE_DESIGN.md` §1.3 |
| 4. Metrics | `src/metrics/stats.rs`, `src/metrics/timeseries.rs`, `src/docs/METRICS.md` |
| 5. Intake | `src/inputs/binary_format.rs`, `src/inputs/simulator.rs`, `src/docs/SCHEMA.md` |
| 6. `simulate` | `src/inputs/simulate_cmd.rs`, `src/inputs/replay_checkpoint.rs`, `src/inputs/scan_cmd.rs`, `src/inputs/download_cmd.rs`, `src/inputs/update_cmd.rs`, `src/inputs/progress.rs` |
| 7. CLI / `main` | `src/inputs/cli.rs`, `src/main.rs` |
| 8. Testing | `src/inputs/test_suite.rs`, `src/docs/TESTING.md` |
| 9. Deployment | `Dockerfile`, `deploy/batch-sol.json`, `src/docs/DEPLOY.md` |
| 10. Closing | `src/docs/ARCHITECTURE.md`, `src/docs/README.md` |

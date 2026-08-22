/// Fixed-point scaling factor for prices. This project trades a single
/// fixed pair (SOL/USD), so there's no `AssetPair`/routing concept anywhere
/// — every price/order/trade is implicitly in that pair.
pub const PRICE_SCALE: u128 = 1_000_000;

pub type Amount = u128;
pub type Price = u128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

// Struct that represent the type of order, limit or market.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderKind {
    Limit { price: Price },
    Market,
}

/// Which engine produced a `Trade` — only meaningful once trades from both
/// engines get merged into one view (see `inputs::cli`'s combined `log`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineKind {
    Fba,
    Cda,
}

impl EngineKind {
    pub fn label(&self) -> &'static str {
        match self {
            EngineKind::Fba => "FBA",
            EngineKind::Cda => "CDA",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Order {
    // --- Raw L4 record fields (identical layout for accepted + rejected) ---
    pub ts: u64,                    // event timestamp, ns since Unix epoch //in data input
    pub user_id: String,            // pseudonymous participant id (userId -> users.csv address) //in data input as UserId
    pub is_builder: bool,           // whether the builder field was present // in data input as isBuilder, not very useful for the simulation, but I will keep it for now
    pub status_id: u8,              // raw status code, see statuses.csv
    pub is_ask: bool,               // true = ask/sell, false = bid/buy
    pub limit_px: Price,            // decoded limit price (PRICE_SCALE fixed-point)
    pub sz: Amount,                 // size AT THE TIME OF THIS RECORD (source semantics;
                                     // NOT the same as `remaining` below)
    pub oid: u64,                   // unique order id assigned by the matching engine
    pub timestamp_diff: u32,        // ts minus original order submission time, ms
    pub trigger_condition: i128,    // decoded signed trigger condition (PRICE_SCALE fixed-point)
    pub triggered: bool,            // whether the trigger condition was met
    pub is_trigger: bool,           // whether this is a trigger/conditional order
    pub has_children: bool,         // whether the order has child TP/SL orders
    pub is_position_tpsl: bool,     // whether this is a position-level TP/SL
    pub reduce_only: bool,          // whether the order is reduce-only
    pub order_type_id: u8,          // raw order type code, see order_types.csv
    pub tif_id: u8,                 // raw time-in-force code, see tifs.csv
    pub trigger_px: Price,          // decoded trigger price
    pub orig_sz: Amount,            // size at original submission

    pub closed_ts: Amount,          // timestamp when the order was closed (filled or canceled), ns since Unix epoch

    // Human-readable labels resolved via the mapdir lookup tables. Optional
    // because not every loader path resolves them eagerly.
    pub status: Option<String>,
    pub order_type: Option<String>,
    pub tif: Option<String>,

    // --- Fields required by the matching engines, not part of the raw record ---
    pub remaining: Amount,          // unfilled part of the order, mutated live by OUR
                                     // matching engines as they process fills — independent
                                     // of `sz`, which is a snapshot of Hyperliquid's own history
}

impl Order {
    /// Convenience constructor for synthetic/test limit orders. Fills in sensible
    /// defaults for every L4-specific field (open, GTC, no trigger, no labels).
    pub fn limit(
        id: u64,
        participant_id: impl Into<String>,
        side: Side,
        price: Price,
        quantity: Amount,
        timestamp: u64,
    ) -> Self {
        Self {
            ts: timestamp,
            user_id: participant_id.into(),
            is_builder: false,
            status_id: 1, // open
            is_ask: matches!(side, Side::Sell),
            limit_px: price,
            sz: quantity,
            oid: id,
            timestamp_diff: 0,
            trigger_condition: 0,
            triggered: false,
            is_trigger: false,
            has_children: false,
            is_position_tpsl: false,
            reduce_only: false,
            order_type_id: 0, // Limit
            tif_id: 1,        // Gtc
            trigger_px: 0,
            orig_sz: quantity,
            closed_ts: 0,
            status: None,
            order_type: None,
            tif: None,
            remaining: quantity,
        }
    }

    /// Convenience constructor for synthetic/test market orders.
    pub fn market(
        id: u64,
        participant_id: impl Into<String>,
        side: Side,
        quantity: Amount,
        timestamp: u64,
    ) -> Self {
        Self {
            ts: timestamp,
            user_id: participant_id.into(),
            is_builder: false,
            status_id: 1, // open
            is_ask: matches!(side, Side::Sell),
            limit_px: 0,
            sz: quantity,
            oid: id,
            timestamp_diff: 0,
            trigger_condition: 0,
            triggered: false,
            is_trigger: false,
            has_children: false,
            is_position_tpsl: false,
            reduce_only: false,
            order_type_id: 1, // Market
            tif_id: 2,        // Ioc
            trigger_px: 0,
            orig_sz: quantity,
            closed_ts: 0,
            status: None,
            order_type: None,
            tif: None,
            remaining: quantity,
        }
    }

    /// Derived order side. The source data stores this as a raw `is_ask` bool
    /// rather than an enum (`true` = ask/sell, `false` = bid/buy).
    pub fn side(&self) -> Side {
        if self.is_ask { Side::Sell } else { Side::Buy }
    }

    /// Derived order kind. The source data encodes this as `order_type_id`
    /// (see order_types.csv) plus a separately-stored `limit_px`, rather than
    /// a Rust enum with only two variants. Mapping of every documented code:
    ///   0  Limit               -> Limit { price: limit_px }
    ///   1  Market               -> Market
    ///   2  Stop Market          -> Market   (executes at market once triggered)
    ///   3  Take Profit Market   -> Market   (executes at market once triggered)
    ///   4  Take Profit Limit    -> Limit { price: limit_px }
    ///   5  Stop Limit           -> Limit { price: limit_px }
    ///   6  Vault Close          -> Market   (forced close, not price-limited)
    /// Any other/unrecognized code defaults to Limit at `limit_px`, the safer
    /// assumption (it won't cross the book unless genuinely marketable).
    ///
    /// Note this only decides *how* the order executes once it is live. It
    /// says nothing about *whether* it's live yet — a conditional order
    /// (`is_trigger == true`) only becomes live once triggered; see
    /// `is_new_live_order`.
    pub fn kind(&self) -> OrderKind {
        match self.order_type_id {
            1 | 2 | 3 | 6 => OrderKind::Market,
            _ => OrderKind::Limit { price: self.limit_px },
        }
    }

    pub fn limit_price(&self) -> Option<Price> {
        match self.kind() {
            OrderKind::Limit { price } => Some(price),
            OrderKind::Market => None,
        }
    }

    /// Whether this record represents a genuinely new, live order that
    /// should be submitted to a matching engine — as opposed to an outright
    /// rejection, a lifecycle bookkeeping event (fill/cancel) for an order
    /// already handled, or a conditional order that hasn't triggered yet.
    ///
    /// Only two `status_id` values represent an order actually becoming
    /// live (see statuses.csv):
    ///   - `open` (1): a regular order is accepted onto the book. For a
    ///     conditional order (`is_trigger == true`), the `open` record marks
    ///     it as *pending*, not live — it isn't resting/tradable until it
    ///     fires, so it's excluded here.
    ///   - `triggered` (9): a conditional order's trigger condition fires
    ///     and it becomes a live order.
    ///
    /// Every other status — outright rejections, cancellations, `filled`,
    /// and a not-yet-triggered conditional order's `open` record — is
    /// dropped rather than submitted, since none of them represent new
    /// resting demand.
    pub fn is_new_live_order(&self) -> bool {
        match self.status_id {
            1 => !self.is_trigger,
            9 => true,
            _ => false,
        }
    }

    /// Whether this record represents the order's owner/venue withdrawing
    /// it — trader-initiated (or venue-initiated) intent to remove the
    /// order, independent of which matching engine processes the flow. An
    /// engine that has this `oid` still sitting in its own book/pending
    /// buffer should remove it in response (see
    /// `FbaOrderBook::submit`/`CdaOrderBook::submit`).
    ///
    /// The 8 cancel-type codes from `statuses.csv`: `canceled`(2),
    /// `reduceOnlyCanceled`(7), `scheduledCancel`(10),
    /// `siblingFilledCanceled`(11), `selfTradeCanceled`(12),
    /// `marginCanceled`(13), `vaultWithdrawalCanceled`(14),
    /// `liquidatedCanceled`(16).
    ///
    /// Deliberately does **not** include `filled`(5), even though it's
    /// also a way an order stops being live: a fill is Hyperliquid's *own*
    /// matching engine's outcome, not something this simulation's
    /// independently-computed CDA/FBA engines should trust — replaying it
    /// would mean reconciling a "remaining size after this fill" against
    /// whatever our own engine already independently did to that same
    /// order, which is often ill-defined. Rejection codes are excluded
    /// too — a rejected order never entered any book, so there's nothing
    /// to remove.
    pub fn is_cancellation(&self) -> bool {
        matches!(self.status_id, 2 | 7 | 10 | 11 | 12 | 13 | 14 | 16)
    }

    pub fn reduce(&mut self, fill: Amount) {
        self.remaining = self.remaining.saturating_sub(fill);
    }
}

#[derive(Clone, Debug)]
pub struct Trade {
    pub trade_id: u64,
    pub price: Price, // clearing price of the trade
    pub quantity: Amount,
    pub buyer_id: String,
    pub seller_id: String,
    pub buy_order_id: u64,
    pub sell_order_id: u64,
    pub engine_type: EngineKind, // which engine generated the trade
    pub ts: u64,
    // Optional on-chain settlement metadata
    pub trade_tx_hash: Option<String>,
    pub chain_id: Option<u64>,
}

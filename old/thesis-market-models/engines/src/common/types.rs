use std::collections::BTreeMap;

/// Fixed-point scaling factor for prices.
pub const PRICE_SCALE: u128 = 1_000_000;

pub type Amount = u128;
pub type Price = u128;



#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AssetPair {
    pub base: String, // e.g., "BTC"
    pub quote: String, // e.g., "USD"
    // Optional on-chain metadata
    pub base_contract: Option<String>,
    pub quote_contract: Option<String>,
    pub base_decimals: Option<u8>,
    pub quote_decimals: Option<u8>,
}

impl AssetPair {
    //impl Into<String>: accepts anything that can be converted into a String
    pub fn new(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
            base_contract: None,
            quote_contract: None,
            base_decimals: None,
            quote_decimals: None,
        }
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.base, self.quote)
    }
}

impl Default for AssetPair {
    /// This simulation only replays the Hyperliquid SOL dataset, so SOL/USD is
    /// the default asset pair wherever one isn't otherwise specified.
    fn default() -> Self {
        AssetPair::new("SOL", "USD")
    }
}

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

// The Order struct mirrors the Hyperliquid L4 order-status record (see
// data/SCHEMA.md): the accepted and rejected order streams share this exact
// 54-byte layout, so one struct covers both — a rejected record is simply an
// Order whose status_id/status is one of the "never touched the book" codes.
//
// Fields are grouped in two blocks:
//   1. Raw record fields, named after the source schema (snake_case).
//   2. Fields the matching engines need that are NOT part of the raw record
//      (which asset pair this belongs to, and our own live fill-tracking state).
//
// Field access that used to be simple enum fields (`side`, `kind`) is now
// derived via methods (`side()`, `kind()`), since the source data encodes them
// as a raw bool (`is_ask`) and a raw type code (`order_type_id`) instead of a
// Rust enum. See the `impl Order` block below.
#[derive(Clone, Debug)]
pub struct Order {
    // --- Raw L4 record fields (identical layout for accepted + rejected) ---
    pub ts: u64,                    // event timestamp, ns since Unix epoch
    pub user_id: String,            // pseudonymous participant id (userId -> users.csv address)
    pub is_builder: bool,           // whether the builder field was present
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

    // Human-readable labels resolved via the mapdir lookup tables. Optional
    // because not every loader path resolves them eagerly.
    pub status: Option<String>,
    pub order_type: Option<String>,
    pub tif: Option<String>,

    // --- Fields required by the matching engines, not part of the raw record ---
    pub pair: AssetPair,            // which asset pair this order belongs to (from the
                                     // source file/coin, not the 54-byte record itself)
    pub remaining: Amount,          // unfilled part of the order, mutated live by OUR
                                     // matching engines as they process fills — independent
                                     // of `sz`, which is a snapshot of Hyperliquid's own history

    // Optional on-chain / client metadata (unrelated to the L4 schema, kept for
    // generality; unused when replaying the dataset)
    pub wallet: Option<String>, //This are not really needed for the simulation, still I will keep them
    pub client_order_id: Option<String>, //not needed but I keep it
    pub chain_id: Option<u64>, //same here
}

impl Order {
    /// Convenience constructor for synthetic/test limit orders. Fills in sensible
    /// defaults for every L4-specific field (open, GTC, no trigger, no labels).
    pub fn limit(
        id: u64,
        participant_id: impl Into<String>,
        pair: AssetPair,
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
            status: None,
            order_type: None,
            tif: None,
            pair,
            remaining: quantity,
            wallet: None,
            client_order_id: None,
            chain_id: None,
        }
    }

    /// Convenience constructor for synthetic/test market orders.
    pub fn market(
        id: u64,
        participant_id: impl Into<String>,
        pair: AssetPair,
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
            status: None,
            order_type: None,
            tif: None,
            pair,
            remaining: quantity,
            wallet: None,
            client_order_id: None,
            chain_id: None,
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
    ///     and it becomes a live order. (Our best-effort reading of the
    ///     documented status semantics — book-diff data was unavailable to
    ///     confirm the exact sequence for conditional orders; revisit if
    ///     real trigger-order data suggests otherwise.)
    ///
    /// Every other status — the outright rejection codes (`badAloPxRejected`,
    /// `perpMarginRejected`, `minTradeNtlRejected`, `reduceOnlyRejected`,
    /// `perpMaxPositionRejected`, `oracleRejected`), every cancellation
    /// variant, `filled`, and a not-yet-triggered conditional order's `open`
    /// record — is dropped rather than submitted, since none of them
    /// represent new resting demand.
    pub fn is_new_live_order(&self) -> bool {
        match self.status_id {
            1 => !self.is_trigger,
            9 => true,
            _ => false,
        }
    }

    pub fn reduce(&mut self, fill: Amount) {
        self.remaining = self.remaining.saturating_sub(fill);
    }
}

#[derive(Clone, Debug)]
pub struct Batch {
    pub id: u64,
    pub orders: Vec<Order>,
}

#[derive(Clone, Debug)]
pub struct Trade {
    pub trade_id: u64,
    pub pair: AssetPair,
    pub price: Price,
    pub quantity: Amount,
    pub buyer_id: String,
    pub seller_id: String,
    pub buy_order_id: u64,
    pub sell_order_id: u64,
    // Execution timestamp (ns since Unix epoch, matching Order::ts). Needed
    // by the metrics layer for interval bucketing, time-to-execution, and
    // markout-horizon lookups (realized spread) — not used by matching
    // logic itself.
    pub ts: u64,
    // Optional on-chain settlement metadata
    pub trade_tx_hash: Option<String>,
    pub chain_id: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub wallet: Option<String>,
    pub balances: BTreeMap<String, i128>,
}

impl Account {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            wallet: None,
            balances: BTreeMap::new(),
        }
    }

    pub fn apply(&mut self, asset: impl Into<String>, delta: i128) {
        let entry = self.balances.entry(asset.into()).or_insert(0);
        *entry += delta;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetTransfer {
    pub asset: String,
    pub amount: Amount,
    pub asset_contract: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementEdge {
    // Sender of the settlement transfer bundle.
    pub from: String,
    // Receiver of the settlement transfer bundle.
    pub to: String,
    // One edge can carry several asset transfers between the same two accounts (multi asset settlement).
    pub transfers: Vec<AssetTransfer>,
}

impl SettlementEdge {
    // Create an empty settlement edge between two participants.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            transfers: Vec::new(),
        }
    }
}

pub type NetPositions = BTreeMap<String, BTreeMap<String, i128>>;

#[derive(Clone, Debug)]
pub struct SettlementPlan {
    pub net_positions: NetPositions,
    pub edges: Vec<SettlementEdge>,
    pub naive_transfer_count: usize,
    pub optimized_transfer_count: usize,
}

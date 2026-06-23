use std::collections::BTreeMap;

/// Fixed-point scaling factor for prices.
pub const PRICE_SCALE: u128 = 1_000_000;

pub type Amount = u128;
pub type Price = u128;


//Clone: Allows to create a deep copy of the struct using .clone(). Since String allocations live on the heap, cloning an AssetPair will duplicate those strings in memory.
//Debug: Allows you to format the struct for debugging output using {:?} in macros like println! or dbg!. It will print looking like this: AssetPair { base: "BTC", quote: "USD" }



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

#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,
    pub participant_id: String,
    pub pair: AssetPair,
    pub side: Side,
    pub kind: OrderKind,
    pub quantity: Amount,
    pub remaining: Amount, //unfilled part of the order
    pub timestamp: u64,
    // Optional on-chain / client metadata
    pub wallet: Option<String>,
    pub client_order_id: Option<String>,
    pub chain_id: Option<u64>,
}

impl Order {
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
            id,
            participant_id: participant_id.into(),
            pair,
            side,
            kind: OrderKind::Limit { price },
            quantity,
            remaining: quantity,
            timestamp,
                wallet: None,
                client_order_id: None,
                chain_id: None,
        }
    }
 // market order
    pub fn market(
        id: u64,
        participant_id: impl Into<String>,
        pair: AssetPair,
        side: Side,
        quantity: Amount,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            participant_id: participant_id.into(),
            pair,
            side,
            kind: OrderKind::Market,
            quantity,
            remaining: quantity,
            timestamp,
                wallet: None,
                client_order_id: None,
                chain_id: None,
        }
    }

    pub fn limit_price(&self) -> Option<Price> {
        match self.kind {
            OrderKind::Limit { price } => Some(price),
            OrderKind::Market => None,
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

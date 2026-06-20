use std::collections::BTreeMap;

/// Fixed-point scaling factor for prices.
pub const PRICE_SCALE: u128 = 1_000_000;

pub type Amount = u128;
pub type Price = u128;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AssetPair {
    pub base: String,
    pub quote: String,
}

impl AssetPair {
    pub fn new(base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
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
    pub remaining: Amount,
    pub timestamp: u64,
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
        }
    }

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
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub balances: BTreeMap<String, i128>,
}

impl Account {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementEdge {
    pub from: String,
    pub to: String,
    pub transfers: Vec<AssetTransfer>,
}

impl SettlementEdge {
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

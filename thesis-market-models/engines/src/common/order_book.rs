use std::cmp::Reverse;

use super::{AssetPair, Amount, Order, OrderKind, Price, Side, Trade, PRICE_SCALE};

#[derive(Clone, Debug)]
pub struct OrderBook {
    pub pair: AssetPair,
    bids: Vec<Order>,
    asks: Vec<Order>,
    next_trade_id: u64,
}

impl OrderBook {
    pub fn new(pair: AssetPair) -> Self {
        Self {
            pair,
            bids: Vec::new(),
            asks: Vec::new(),
            next_trade_id: 1,
        }
    }

    pub fn submit(&mut self, mut order: Order) -> Vec<Trade> {
        assert_eq!(order.pair, self.pair, "order submitted to the wrong book");

        let mut trades = Vec::new();

        match order.side {
            Side::Buy => self.match_buy(&mut order, &mut trades),
            Side::Sell => self.match_sell(&mut order, &mut trades),
        }

        if order.remaining > 0 && matches!(order.kind, OrderKind::Limit { .. }) {
            self.store(order);
        }

        trades
    }

    pub fn snapshot(&self) -> (Vec<Order>, Vec<Order>) {
        (self.bids.clone(), self.asks.clone())
    }

    fn store(&mut self, order: Order) {
        match order.side {
            Side::Buy => self.bids.push(order),
            Side::Sell => self.asks.push(order),
        }
    }

    fn match_buy(&mut self, incoming: &mut Order, trades: &mut Vec<Trade>) {
        while incoming.remaining > 0 {
            let candidate = self.best_ask_index(incoming);
            let Some(index) = candidate else { break };

            let mut resting = self.asks.remove(index);
            let fill = incoming.remaining.min(resting.remaining);
            incoming.reduce(fill);
            resting.reduce(fill);

            trades.push(self.make_trade(incoming, &resting, fill));

            if resting.remaining > 0 {
                self.asks.insert(index, resting);
            }
        }
    }

    fn match_sell(&mut self, incoming: &mut Order, trades: &mut Vec<Trade>) {
        while incoming.remaining > 0 {
            let candidate = self.best_bid_index(incoming);
            let Some(index) = candidate else { break };

            let mut resting = self.bids.remove(index);
            let fill = incoming.remaining.min(resting.remaining);
            incoming.reduce(fill);
            resting.reduce(fill);

            trades.push(self.make_trade(&resting, incoming, fill));

            if resting.remaining > 0 {
                self.bids.insert(index, resting);
            }
        }
    }

    fn best_ask_index(&self, incoming: &Order) -> Option<usize> {
        self.asks
            .iter()
            .enumerate()
            .filter(|(_, order)| order.remaining > 0 && order.participant_id != incoming.participant_id)
            .filter(|(_, order)| match incoming.kind {
                OrderKind::Market => true,
                OrderKind::Limit { price } => order.limit_price().map_or(false, |resting| resting <= price),
            })
            .min_by_key(|(_, order)| (order.limit_price().unwrap_or(PRICE_SCALE * 2), order.timestamp, order.id))
            .map(|(index, _)| index)
    }

    fn best_bid_index(&self, incoming: &Order) -> Option<usize> {
        self.bids
            .iter()
            .enumerate()
            .filter(|(_, order)| order.remaining > 0 && order.participant_id != incoming.participant_id)
            .filter(|(_, order)| match incoming.kind {
                OrderKind::Market => true,
                OrderKind::Limit { price } => order.limit_price().map_or(false, |resting| resting >= price),
            })
            .max_by_key(|(_, order)| (order.limit_price().unwrap_or(0), Reverse(order.timestamp), Reverse(order.id)))
            .map(|(index, _)| index)
    }

    fn make_trade(&mut self, buy: &Order, sell: &Order, quantity: Amount) -> Trade {
        let price = match (buy.limit_price(), sell.limit_price()) {
            (Some(buy_price), Some(sell_price)) => sell_price.max(buy_price.min(sell_price)),
            (Some(price), None) | (None, Some(price)) => price,
            (None, None) => PRICE_SCALE,
        };

        let trade = Trade {
            trade_id: self.next_trade_id,
            pair: self.pair.clone(),
            price,
            quantity,
            buyer_id: buy.participant_id.clone(),
            seller_id: sell.participant_id.clone(),
            buy_order_id: buy.id,
            sell_order_id: sell.id,
        };
        self.next_trade_id += 1;
        trade
    }
}

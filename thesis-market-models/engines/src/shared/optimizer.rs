use std::collections::BTreeMap;

use crate::common::{Amount, AssetTransfer, NetPositions, SettlementEdge, SettlementPlan, Trade, PRICE_SCALE};

#[derive(Clone, Debug)]
pub struct SettlementSummary {
    pub plan: SettlementPlan,
    pub asset_edges: BTreeMap<String, usize>,
}

#[derive(Default, Debug)]
pub struct SettlementOptimizer;

impl SettlementOptimizer {
    pub fn new() -> Self {
        Self
    }

    pub fn optimize_trades(&self, trades: &[Trade]) -> SettlementSummary {
        let net_positions = self.net_positions_from_trades(trades);
        let plan = self.optimize_net_positions(net_positions, trades.len() * 2);
        let asset_edges = self.count_asset_edges(&plan.edges);

        SettlementSummary { plan, asset_edges }
    }

    pub fn optimize_net_positions(&self, net_positions: NetPositions, naive_transfer_count: usize) -> SettlementPlan {
        let per_asset_transfers = self.settle_assets(&net_positions);
        let edges = self.bundle_transfers(per_asset_transfers);

        SettlementPlan {
            net_positions,
            optimized_transfer_count: edges.len(),
            naive_transfer_count,
            edges,
        }
    }

    pub fn net_positions_from_trades(&self, trades: &[Trade]) -> NetPositions {
        let mut net_positions: NetPositions = BTreeMap::new();

        for trade in trades {
            let base = trade.pair.base.clone();
            let quote = trade.pair.quote.clone();
            let quote_amount = trade.quantity.saturating_mul(trade.price) / PRICE_SCALE;

            self.apply(&mut net_positions, &trade.buyer_id, &base, trade.quantity as i128);
            self.apply(&mut net_positions, &trade.buyer_id, &quote, -(quote_amount as i128));
            self.apply(&mut net_positions, &trade.seller_id, &base, -(trade.quantity as i128));
            self.apply(&mut net_positions, &trade.seller_id, &quote, quote_amount as i128);
        }

        net_positions
    }

    fn apply(&self, net_positions: &mut NetPositions, participant: &str, asset: &str, delta: i128) {
        let asset_balances = net_positions.entry(participant.to_string()).or_default();
        let balance = asset_balances.entry(asset.to_string()).or_insert(0);
        *balance += delta;
    }

    fn settle_assets(&self, net_positions: &NetPositions) -> Vec<(String, String, AssetTransfer)> {
        let mut assets = BTreeMap::<String, Vec<(String, i128)>>::new();

        for (participant, balances) in net_positions {
            for (asset, balance) in balances {
                if *balance != 0 {
                    assets.entry(asset.clone()).or_default().push((participant.clone(), *balance));
                }
            }
        }

        let mut transfers = Vec::new();

        for (asset, mut balances) in assets {
            balances.sort_by(|left, right| left.0.cmp(&right.0));

            let mut debtors: Vec<(String, Amount)> = balances
                .iter()
                .filter(|(_, balance)| *balance < 0)
                    .map(|(participant, balance)| (participant.clone(), (-*balance) as Amount))
                .collect();
            let mut creditors: Vec<(String, Amount)> = balances
                .iter()
                .filter(|(_, balance)| *balance > 0)
                .map(|(participant, balance)| (participant.clone(), *balance as Amount))
                .collect();

            debtors.sort_by(|left, right| left.0.cmp(&right.0));
            creditors.sort_by(|left, right| left.0.cmp(&right.0));

            let mut debtor_index = 0usize;
            let mut creditor_index = 0usize;

            while debtor_index < debtors.len() && creditor_index < creditors.len() {
                let amount = debtors[debtor_index].1.min(creditors[creditor_index].1);
                transfers.push((
                    debtors[debtor_index].0.clone(),
                    creditors[creditor_index].0.clone(),
                    AssetTransfer {
                        asset: asset.clone(),
                        amount,
                        asset_contract: None,
                    },
                ));

                debtors[debtor_index].1 -= amount;
                creditors[creditor_index].1 -= amount;

                if debtors[debtor_index].1 == 0 {
                    debtor_index += 1;
                }
                if creditors[creditor_index].1 == 0 {
                    creditor_index += 1;
                }
            }
        }

        transfers
    }

    fn bundle_transfers(&self, transfers: Vec<(String, String, AssetTransfer)>) -> Vec<SettlementEdge> {
        let mut bundles: BTreeMap<(String, String), SettlementEdge> = BTreeMap::new();

        for (from, to, transfer) in transfers {
            let entry = bundles
                .entry((from.clone(), to.clone()))
                .or_insert_with(|| SettlementEdge::new(from.clone(), to.clone()));
            entry.transfers.push(transfer);
        }

        let mut edges: Vec<SettlementEdge> = bundles.into_values().collect();
        edges.sort_by(|left, right| left.from.cmp(&right.from).then(left.to.cmp(&right.to)));
        edges
    }

    fn count_asset_edges(&self, edges: &[SettlementEdge]) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();

        for edge in edges {
            for transfer in &edge.transfers {
                *counts.entry(transfer.asset.clone()).or_insert(0) += 1;
            }
        }

        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{AssetPair, Trade, PRICE_SCALE};

    #[test]
    fn optimizer_reduces_cleared_transfer_count() {
        let pair = AssetPair::new("AAA", "BBB");
        let trades = vec![
            Trade { trade_id: 1, pair: pair.clone(), price: PRICE_SCALE, quantity: 10, buyer_id: "A".into(), seller_id: "B".into(), buy_order_id: 1, sell_order_id: 2, ts: 0, trade_tx_hash: None, chain_id: None },
            Trade { trade_id: 2, pair: pair.clone(), price: PRICE_SCALE, quantity: 10, buyer_id: "B".into(), seller_id: "C".into(), buy_order_id: 3, sell_order_id: 4, ts: 0, trade_tx_hash: None, chain_id: None },
            Trade { trade_id: 3, pair: pair.clone(), price: PRICE_SCALE, quantity: 10, buyer_id: "C".into(), seller_id: "A".into(), buy_order_id: 5, sell_order_id: 6, ts: 0, trade_tx_hash: None, chain_id: None },
        ];

        let optimizer = SettlementOptimizer::new();
        let summary = optimizer.optimize_trades(&trades);

        assert_eq!(summary.plan.optimized_transfer_count, 0);
        assert!(summary.plan.naive_transfer_count > 0);
    }
}

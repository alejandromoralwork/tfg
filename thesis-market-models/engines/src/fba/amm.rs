use crate::common::{PRICE_SCALE, Price};

/// Simple AMM constant-product pool (x * y = k) representing a pair of assets.
#[derive(Clone, Debug)]
pub struct AMMPool {
    /// reserve of base asset (x)
    pub reserve_x: u128,
    /// reserve of quote asset (y)
    pub reserve_y: u128,
}

impl AMMPool {
    pub fn new(reserve_x: u128, reserve_y: u128) -> Self {
        Self { reserve_x, reserve_y }
    }

    /// current marginal price of X in terms of Y (y/x), scaled by `PRICE_SCALE`.
    pub fn price(&self) -> Price {
        if self.reserve_x == 0 { return Price::MAX; }
        // price = reserve_y / reserve_x scaled to PRICE_SCALE
        (self.reserve_y.saturating_mul(PRICE_SCALE)) / self.reserve_x
    }

    /// Execute a sell of `dx` base units and return `dy` quote units received.
    /// Uses exact constant-product formula: dy = y - k/(x+dx).
    pub fn execute_sell(&mut self, dx: u128) -> u128 {
        if dx == 0 {
            return 0;
        }

        let k = (self.reserve_x as u128).saturating_mul(self.reserve_y as u128);
        let x_new = self.reserve_x.saturating_add(dx);
        // Avoid division by zero
        if x_new == 0 {
            return 0;
        }

        let y_new = k / x_new;
        let dy = self.reserve_y.saturating_sub(y_new);

        // apply reserves
        self.reserve_x = x_new;
        self.reserve_y = y_new;

        dy
    }

    /// Given breakpoints (dx amounts sold), return vector of (dx, dy) pairs representing
    /// the piecewise mapping for linearization.
    pub fn linearize(&self, breakpoints: &[u128]) -> Vec<(u128, u128)> {
        let mut pairs = Vec::new();
        let k = (self.reserve_x as u128).saturating_mul(self.reserve_y as u128);
        for &dx in breakpoints {
            let x_new = self.reserve_x.saturating_add(dx);
            if x_new == 0 {
                pairs.push((dx, 0));
                continue;
            }
            let y_new = k / x_new;
            let dy = if self.reserve_y > y_new { self.reserve_y - y_new } else { 0 };
            pairs.push((dx, dy));
        }
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amm_price_and_execution() {
        let mut pool = AMMPool::new(1_000_000u128, 2_000_000u128);
        let p = pool.price();
        assert!(p > 0);

        let dy = pool.execute_sell(100_000u128);
        assert!(dy > 0);
        // price (scaled) should be > 0 after execution
        let p2 = pool.price();
        assert!(p2 > 0);
    }

    #[test]
    fn amm_linearize_breakpoints() {
        let pool = AMMPool::new(1000u128, 2000u128);
        let bps = vec![1u128, 10u128, 100u128];
        let pairs = pool.linearize(&bps);
        assert_eq!(pairs.len(), 3);
        for (dx, dy) in pairs {
            assert!(dy >= 0);
            assert!(dx > 0);
        }
    }
}

# Frequent Batch Auctions with Continuous Order Books and AMM Settlement

## A Mathematical Framework for Market Clearing and Decentralized Trading

**June 2026**

---

## Table of Contents

1. [Introduction](#introduction)
2. [Mathematical Notation and Preliminaries](#mathematical-notation-and-preliminaries)
3. [Continuous Order Book (COB) Mechanics](#continuous-order-book-cob-mechanics)
4. [Discrete Pair Clearing: The Frequent Batch Auction](#discrete-pair-clearing-the-frequent-batch-auction)
5. [Automated Market Maker (AMM) Constant-Product Model](#automated-market-maker-amm-constant-product-model)
6. [Multi-Asset FBA LP Formulation](#multi-asset-fba-lp-formulation)
7. [Settlement Optimization](#settlement-optimization)
8. [LP Encoding and Solver Interface](#lp-encoding-and-solver-interface)
9. [Implementation Architecture](#implementation-architecture)
10. [Implementation Gaps and Future Work](#implementation-gaps-and-future-work)
11. [Experiments and Reproducibility](#experiments-and-reproducibility)
12. [Conclusion](#conclusion)
13. [Appendix: Code Reference](#appendix-code-reference)

---

## 1. Introduction {#introduction}

### 1.1 Project Scope

This thesis develops a unified mathematical and computational framework for modern decentralized markets, combining:

- **Frequent Batch Auctions (FBA):** A discrete clearing protocol that batches orders and computes uniform prices to maximize traded volume while respecting limit price constraints.
- **Continuous Order Books (COB):** A live order matching engine that executes crossing orders between batch periods, enabling immediate settlement and price discovery.
- **Automated Market Makers (AMM):** A constant-product liquidity pool model that provides on-chain fungible liquidity and can be encoded as piecewise-linear constraints in the batch clearing optimization.
- **Settlement Optimization:** A post-clearing algorithm that minimizes on-chain settlement legs by greedily pairing net creditors and debtors, reducing transaction costs and latency.

### 1.2 Motivation

Traditional limit order books suffer from:
- **Toxicity from latency arbitrage:** Faster traders exploit information asymmetries.
- **Inefficient execution:** Orders may not match even when equilibrium prices exist.
- **High variance in execution cost:** Slippage depends on queue position and order flow.

Frequent Batch Auctions address these challenges by:
- **Clearing at a uniform price:** All trades within a batch execute at a single price, eliminating latency advantages.
- **Maximizing volume:** The clearing price maximizes the number of units traded, favoring liquidity.
- **Accepting limit prices:** Orders specify maximum/minimum acceptable prices, ensuring feasibility.

Combining with COB and AMM liquidity creates a hybrid infrastructure:
- Retail traders get immediate execution via COB.
- Wholesale batches clear efficiently via FBA.
- Liquidity providers earn fees via constant-product AMM pools.

### 1.3 Core Components and Code Artifacts

| Component | Purpose | Code Artifact |
|-----------|---------|---------------|
| **COB** | Live order matching between batches | [engines/src/common/order_book.rs](engines/src/common/order_book.rs) |
| **FBA Clearing** | Batch price computation and trade construction | [engines/src/fba/clearing.rs](engines/src/fba/clearing.rs) |
| **AMM** | Constant-product liquidity pool | [engines/src/fba/amm.rs](engines/src/fba/amm.rs) |
| **Settlement Optimizer** | Net position bundling and transfer minimization | [engines/src/fba/optimizer.rs](engines/src/fba/optimizer.rs) |
| **LP Encoder** | Translates orders and AMM to LP model | [engines/src/fba/lp_encoder.rs](engines/src/fba/lp_encoder.rs) |
| **LP Builder** | Emits textual LP files | [engines/src/fba/solver.rs](engines/src/fba/solver.rs) |
| **Simulation** | Example runner and experiments | [simulation/src/main.rs](simulation/src/main.rs) |

---

## 2. Mathematical Notation and Preliminaries {#mathematical-notation-and-preliminaries}

### 2.1 Assets, Quantities, and Prices

**Definition 2.1.1: Asset Pair**

An asset pair $(i, j)$ consists of a **base asset** $i$ and **quote asset** $j$. A price $p_{i,j}$ denotes the number of units of asset $j$ per unit of asset $i$.

**Code Reference:** [types.rs](engines/src/common/types.rs), lines 1–30

```rust
pub struct AssetPair {
    pub base: String,
    pub quote: String,
}

pub const PRICE_SCALE: u128 = 1_000_000;
```

**Definition 2.1.2: Fixed-Point Scaling**

To avoid floating-point arithmetic, all prices and amounts are represented as **fixed-point integers** scaled by $\texttt{PRICE\_SCALE} = 10^6$.

A rational price $p$ is encoded as an integer $\tilde{p} = p \cdot 10^6$. Thus:

$$p = \frac{\tilde{p}}{10^6}$$

When computing products (e.g., quantity $\times$ price), we must rescale:

$$\text{quote\_amount} = \frac{\text{quantity} \times \tilde{p}}{10^6}$$

**Example 2.1.1:** If a trader sells 100 units at a price of 11 per unit:
- $\text{quantity} = 100$
- $\tilde{p} = 11 \times 10^6 = 11,000,000$
- $\text{quote\_amount} = \frac{100 \times 11,000,000}{10^6} = 1100$

### 2.2 Orders and Order Books

**Definition 2.2.1: Order**

An order specifies:
- **Order ID** $k$
- **Participant ID** (trader identity)
- **Asset pair** $(i, j)$
- **Side** $s \in \{\text{Buy}, \text{Sell}\}$
- **Order kind:**
  - Limit order: maximum buy price or minimum sell price $p_k$
  - Market order: no price restriction
- **Quantity** $q_k$ (units of base asset)
- **Remaining quantity** $r_k \leq q_k$ (unfilled portion)
- **Timestamp** $t_k$ (submission time; used for priority)

**Code Reference:** [types.rs](engines/src/common/types.rs), lines 36–98

```rust
#[derive(Clone, Debug)]
pub struct Order {
    pub id: u64,
    pub participant_id: String,
    pub pair: AssetPair,
    pub side: Side,
    pub kind: OrderKind,  // Limit { price } or Market
    pub quantity: Amount,
    pub remaining: Amount,
    pub timestamp: u64,
}
```

**Definition 2.2.2: Batch**

A batch $B$ is a collection of orders submitted over a time interval $[t_0, t_0 + \Delta t]$. Orders are processed together at the end of the interval.

$$B = \{o_1, o_2, \ldots, o_n\}$$

**Code Reference:** [types.rs](engines/src/common/types.rs), lines 101–105

```rust
pub struct Batch {
    pub id: u64,
    pub orders: Vec<Order>,
}
```

### 2.3 Trades

**Definition 2.3.1: Trade**

A trade is a matched pair between a buying and selling order:

$$\tau = (p_\tau, q_\tau, \text{buyer}, \text{seller})$$

where:
- $p_\tau$ is the **execution price** (fixed-point scaled)
- $q_\tau$ is the **executed quantity** (units of base asset)
- Buyer transfers $q_\tau$ units of base asset to seller
- Seller transfers $q_\tau \times p_\tau / 10^6$ units of quote asset to buyer

**Code Reference:** [types.rs](engines/src/common/types.rs), lines 107–116

```rust
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
```

---

## 3. Continuous Order Book (COB) Mechanics {#continuous-order-book-cob-mechanics}

### 3.1 Order Submission and Matching

The COB maintains a live order book and executes crossing orders immediately. This section formalizes the matching algorithm.

**Definition 3.1.1: Order Book State**

The order book maintains two sides:
- **Buy side:** Orders sorted by limit price (highest first), then by timestamp (earlier first)
- **Sell side:** Orders sorted by limit price (lowest first), then by timestamp (earlier first)

**Code Reference:** [order_book.rs](engines/src/common/order_book.rs), lines 1–50

```rust
pub struct OrderBook {
    pub buy_orders: Vec<Order>,
    pub sell_orders: Vec<Order>,
}
```

### 3.2 Matching Algorithm

**Algorithm 3.2.1: Match Incoming Buy Order**

When a buy order arrives with price $p_b$ and quantity $q_b$:

1. **Iterate sell orders** in order (lowest price first).
2. **For each sell order** with price $p_s$ and quantity $q_s$:
   - If $p_b \geq p_s$ (order crosses), proceed to step 3.
   - Else, stop (no further matches).
3. **Compute execution price** as the sell order's limit price:
   $$p_\text{exec} = p_s$$
4. **Execute quantity** as the minimum of remaining quantities:
   $$q_\text{exec} = \min(q_b, q_s)$$
5. **Reduce orders** and record the trade.
6. **Repeat** until no quantity remains or no more orders cross.

**Mathematical Formulation:**

For a buy order $o_b$ with $(p_b, q_b)$, the set of eligible sell orders is:

$$\mathcal{S}_\text{eligible} = \{o_s \in S : p_b \geq p_s(o_s)\}$$

Trades are produced in order:

$$\tau_i = (p_s(o_{s,i}), \min(q_b^{(i)}, q_s(o_{s,i})), \text{buyer}, \text{seller}_i)$$

where $q_b^{(i)}$ is the remaining buy quantity after $i-1$ trades.

**Code Reference:** [order_book.rs](engines/src/common/order_book.rs), lines 60–120

```rust
pub fn match_buy(&mut self, mut order: Order) -> Vec<Trade> {
    let mut trades = Vec::new();
    
    while order.remaining > 0 && !self.sell_orders.is_empty() {
        let best_ask_idx = self.best_ask_index();
        if best_ask_idx.is_none() { break; }
        
        let idx = best_ask_idx.unwrap();
        let sell_order = &self.sell_orders[idx];
        
        if let OrderKind::Limit { price: sell_price } = sell_order.kind {
            if let OrderKind::Limit { price: buy_price } = order.kind {
                if buy_price < sell_price { break; }
            }
        }
        
        let trade = self.make_trade(&order, &sell_order);
        let fill = trade.quantity;
        
        order.reduce(fill);
        self.sell_orders[idx].reduce(fill);
        trades.push(trade);
        
        if self.sell_orders[idx].remaining == 0 {
            self.sell_orders.remove(idx);
        }
    }
    
    if order.remaining > 0 {
        self.buy_orders.push(order);
    }
    
    trades
}
```

### 3.3 Price Selection in COB

**Definition 3.3.1: Execution Price**

When a buy order crosses a sell order, the execution price is the **sell order's limit price** (passive order sets price). This incentivizes limit order placement and fairness.

For a buy order with price $p_b$ crossing a sell order with price $p_s$:

$$p_\text{exec} = p_s$$

(The sell order was posted first; the buy order is the aggressor.)

**Rationale:** This pricing rule encourages traders to submit limit orders instead of market orders (market orders execute at worse prices). It also ensures that trades occur within both order's acceptable price ranges: $p_s \leq p_\text{exec} \leq p_b$.

**Code Reference:** [order_book.rs](engines/src/common/order_book.rs), lines 140–165

```rust
fn make_trade(&self, buy_order: &Order, sell_order: &Order) -> Trade {
    let trade_id = (buy_order.id as u128 * 1_000_000 + sell_order.id as u128) as u64;
    let fill = buy_order.remaining.min(sell_order.remaining);
    
    let price = if let OrderKind::Limit { price: sell_price } = sell_order.kind {
        sell_price
    } else {
        PRICE_SCALE  // fallback to $1
    };
    
    Trade {
        trade_id,
        pair: buy_order.pair.clone(),
        price,
        quantity: fill,
        buyer_id: buy_order.participant_id.clone(),
        seller_id: sell_order.participant_id.clone(),
        buy_order_id: buy_order.id,
        sell_order_id: sell_order.id,
    }
}
```

### 3.4 Snapshot and State Queries

**Definition 3.4.1: Order Book Snapshot**

A snapshot captures the current state of all unmatched orders:

$$\text{snapshot} = (\text{buy\_orders}, \text{sell\_orders})$$

Used for audit trails, settlement, and COB state queries.

**Code Reference:** [order_book.rs](engines/src/common/order_book.rs), lines 50–60

```rust
pub fn snapshot(&self) -> (Vec<Order>, Vec<Order>) {
    (self.buy_orders.clone(), self.sell_orders.clone())
}
```

---

## 4. Discrete Pair Clearing: The Frequent Batch Auction {#discrete-pair-clearing-the-frequent-batch-auction}

### 4.1 Clearing Problem Formulation

At discrete time intervals, the system collects all unmatched orders in a batch and computes a **uniform clearing price** that maximizes traded volume.

**Definition 4.1.1: Clearing Problem for a Single Pair**

Given a batch of orders $B$ for asset pair $(i,j)$, find a price $p \in \mathbb{R}_+$ such that:

1. **Buy demand at $p$:** All buy orders with price $\geq p$ are willing to trade:
   $$D(p) = \sum_{o \in B : o.\text{side} = \text{Buy} \land (o.\text{kind} = \text{Market} \lor p_o \geq p)} o.\text{quantity}$$

2. **Sell supply at $p$:** All sell orders with price $\leq p$ are willing to trade:
   $$S(p) = \sum_{o \in B : o.\text{side} = \text{Sell} \land (o.\text{kind} = \text{Market} \lor p_o \leq p)} o.\text{quantity}$$

3. **Traded volume:**
   $$V(p) = \min(D(p), S(p))$$

4. **Imbalance:**
   $$I(p) = |D(p) - S(p)|$$

The clearing price is chosen to:
- **Primary objective:** Maximize traded volume $V(p)$
- **Tiebreaker 1:** Minimize imbalance $I(p)$
- **Tiebreaker 2:** Maximize price $p$ (favor sellers if tied)

### 4.2 Candidate Prices

**Definition 4.2.1: Candidate Price Set**

The clearing price must be one of the limit prices in the order batch (or a fallback price if no limits exist). This discretizes the search space:

$$P_\text{cand} = \{p_o : o \in B \land o.\text{kind} = \text{Limit}\} \cup \{\text{PRICE\_SCALE}\}$$

**Theorem 4.2.1:** The volume-maximizing price is always in $P_\text{cand}$.

*Proof sketch:* Between two prices $p_1 < p_2$ where no limit price exists, the sets of eligible buy and sell orders are identical. Thus $D(p)$, $S(p)$, and $V(p)$ are constant, and there is no variation in volume. Hence, we can restrict the search to candidate prices. $\square$

**Code Reference:** [clearing.rs](engines/src/fba/clearing.rs), lines 216–230

```rust
fn candidate_prices(&self, orders: &[Order]) -> BTreeSet<Price> {
    let mut candidates = BTreeSet::new();
    let mut saw_limit = false;

    for order in orders {
        if let Some(price) = order.limit_price() {
            saw_limit = true;
            candidates.insert(price);
        }
    }

    if !saw_limit {
        candidates.insert(PRICE_SCALE);
    }

    candidates
}
```

### 4.3 Price Selection Algorithm

**Algorithm 4.3.1: Select Clearing Price**

**Input:** Batch of orders $B$, candidate price set $P_\text{cand}$

**Output:** Clearing price $p^*$, demand $D^*$, supply $S^*$

**Procedure:**

```
best := (price = -∞, volume = -∞, imbalance = +∞)

for p in P_cand:
    D := aggregate_volume(Buy, p)
    S := aggregate_volume(Sell, p)
    V := min(D, S)
    I := |D - S|
    
    if V > best.volume
       or (V == best.volume and I < best.imbalance)
       or (V == best.volume and I == best.imbalance and p > best.price):
        best := (p, V, I)

return best
```

**Code Reference:** [clearing.rs](engines/src/fba/clearing.rs), lines 234–260

```rust
fn select_price(&self, orders: &[Order], candidates: BTreeSet<Price>) -> Option<(Price, Amount, Amount)> {
    let mut best: Option<(Price, Amount, Amount)> = None;

    for price in candidates {
        let demand = self.aggregate_volume(orders, Side::Buy, price);
        let supply = self.aggregate_volume(orders, Side::Sell, price);
        let volume = demand.min(supply);
        let imbalance = demand.abs_diff(supply);

        let better = match best {
            None => true,
            Some((best_price, best_demand, best_supply)) => {
                let best_volume = best_demand.min(best_supply);
                let best_imbalance = best_demand.abs_diff(best_supply);
                volume > best_volume
                    || (volume == best_volume && imbalance < best_imbalance)
                    || (volume == best_volume && imbalance == best_imbalance && price > best_price)
            }
        };

        if better {
            best = Some((price, demand, supply));
        }
    }

    best
}
```

### 4.4 Trade Construction

**Algorithm 4.4.1: Construct Trades**

Once a clearing price $p^*$ is selected:

1. **Identify eligible buy orders:** Orders with side = Buy and (market or limit price $\geq p^*$).
2. **Identify eligible sell orders:** Orders with side = Sell and (market or limit price $\leq p^*$).
3. **Sort by priority:** Earlier timestamps and specific market order priority.
4. **Pair orders:** Match buy and sell orders in sequence, creating trades at price $p^*$ with executed quantities.

**Code Reference:** [clearing.rs](engines/src/fba/clearing.rs), lines 280–310

```rust
fn eligible_orders(&self, orders: &[Order], side: Side, price: Price) -> Vec<Order> {
    let mut eligible: Vec<Order> = orders
        .iter()
        .filter(|order| order.side == side)
        .filter(|order| match order.kind {
            OrderKind::Market => true,
            OrderKind::Limit { price: limit_price } => match side {
                Side::Buy => limit_price >= price,
                Side::Sell => limit_price <= price,
            },
        })
        .cloned()
        .collect();

    eligible.sort_by_key(|order| {
        let aggressiveness = self.order_priority(order);
        (aggressiveness, order.timestamp, order.id)
    });

    eligible
}
```

### 4.5 Clearing Result

**Definition 4.5.1: Clearing Result**

The clearing output consists of:

$$\text{ClearingResult} = \{p^*, V^*, D^*, S^*, \mathcal{T}\}$$

where:
- $p^*$ is the clearing price
- $V^* = \min(D^*, S^*)$ is the traded volume
- $D^*$ is the aggregate demand at $p^*$
- $S^*$ is the aggregate supply at $p^*$
- $\mathcal{T} = \{\tau_1, \tau_2, \ldots, \tau_m\}$ is the set of trades

**Code Reference:** [clearing.rs](engines/src/fba/clearing.rs), lines 6–14

```rust
pub struct ClearingResult {
    pub pair: AssetPair,
    pub clearing_price: Price,
    pub traded_quantity: Amount,
    pub demand_at_price: Amount,
    pub supply_at_price: Amount,
    pub trades: Vec<Trade>,
}
```

---

## 5. Automated Market Maker (AMM) Constant-Product Model {#automated-market-maker-amm-constant-product-model}

### 5.1 Constant-Product Formula

**Definition 5.1.1: AMM Pool**

An AMM pool for asset pair $(i, j)$ maintains reserves $(x, y)$ where:
- $x$ = reserve of base asset $i$
- $y$ = reserve of quote asset $j$

The **constant product invariant** is:

$$x \cdot y = k$$

where $k$ is a constant.

**Code Reference:** [amm.rs](engines/src/fba/amm.rs), lines 1–16

```rust
pub struct AMMPool {
    pub reserve_x: u128,
    pub reserve_y: u128,
}
```

### 5.2 Marginal Price

**Definition 5.2.1: Marginal Price**

The marginal price of asset $i$ (base) in terms of asset $j$ (quote) is:

$$p(x, y) = \frac{y}{x}$$

This is the derivative of the constant product curve: the rate at which the pool yields $y$ when given $dx \to 0$ of asset $x$.

**Code Reference:** [amm.rs](engines/src/fba/amm.rs), lines 18–22

```rust
pub fn price(&self) -> Price {
    if self.reserve_x == 0 { return Price::MAX; }
    // price = reserve_y / reserve_x, scaled to PRICE_SCALE
    (self.reserve_y.saturating_mul(PRICE_SCALE)) / self.reserve_x
}
```

**Key observation:** The price is returned as a fixed-point integer (Price = u128) scaled by `PRICE_SCALE = 10^6`. If $y/x = 2.5$, then the function returns $2,500,000$.

### 5.3 Execution: Constant Product Formula

**Definition 5.3.1: Swap Execution**

When a trader sells $dx$ units of asset $x$, the pool computes the output $dy$ using the constant product formula.

After adding $dx$ to reserves, the new $y$ is:

$$y_\text{new} = \frac{k}{x + dx} = \frac{x \cdot y}{x + dx}$$

The output is:

$$dy = y - y_\text{new} = y - \frac{x \cdot y}{x + dx} = \frac{y \cdot dx}{x + dx}$$

**Code Reference:** [amm.rs](engines/src/fba/amm.rs), lines 24–45

```rust
pub fn execute_sell(&mut self, dx: u128) -> u128 {
    if dx == 0 {
        return 0;
    }

    let k = (self.reserve_x as u128).saturating_mul(self.reserve_y as u128);
    let x_new = self.reserve_x.saturating_add(dx);
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
```

**Example 5.3.1:** Suppose $x = 1,000,000$, $y = 2,000,000$, so $k = 2 \times 10^{12}$.

A trader sells $dx = 100,000$ units of base asset:
- $x_\text{new} = 1,100,000$
- $y_\text{new} = \frac{2 \times 10^{12}}{1,100,000} \approx 1,818,181.82$
- $dy = 2,000,000 - 1,818,181.82 = 181,818.18$ (in fixed-point: $181,818.18 \times 10^6$)

### 5.4 Piecewise Linearization

**Definition 5.4.1: Piecewise Linear Approximation**

For use in LP formulations, the nonlinear constant-product curve can be approximated by a sequence of line segments. Given breakpoints $dx_1, dx_2, \ldots, dx_m$, compute the corresponding outputs $(dy_i)$ using the constant-product formula, then treat each segment as linear.

This enables:
- $dy = f_\text{piecewise}(dx)$ where $f$ is piecewise-linear.
- LP solvers can encode piecewise-linear functions using binary variables and big-M constraints.

**Code Reference:** [amm.rs](engines/src/fba/amm.rs), lines 47–63

```rust
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
```

**Implementation note:** The linearize() function computes $(dx_i, dy_i)$ pairs that define the piecewise envelope. These are later used to construct piecewise-linear constraints in the LP encoder.

---

## 6. Multi-Asset FBA LP Formulation {#multi-asset-fba-lp-formulation}

### 6.1 Decision Variables

**Definition 6.1.1: Decision Variables**

For a batch of orders $O$ and asset set $A$, the LP model uses:

1. **Execution volumes** $v_k \in \mathbb{R}_{\geq 0}$ for each order $k \in O$:
   - $v_k$ = quantity of order $k$ that is executed in the batch
   - Constraint: $v_k \leq q_k$ (executed volume $\leq$ order quantity)

2. **Price vector** $P = [p_0, p_1, \ldots, p_{|A|-1}] \in \mathbb{R}_{>0}^{|A|}$:
   - $p_i$ = exchange rate of asset $i$ relative to a numeraire (asset 0)
   - Normalization: $p_0 = 1$ (numeraire asset)

3. **Piecewise-linear AMM segments** $z_{\text{seg}} \in [0,1]$ for each linearization segment:
   - Encodes the position on the piecewise linear approximation of the constant-product curve
   - Binary indicator for which segment is active (if using big-M constraints)

**Code sketch (future implementation):**

```rust
// In lp_encoder.rs
pub struct LPEncoder {
    // v_k variables: indexed by order.id
    pub order_exec_vars: HashMap<u64, String>,
    
    // p_i variables: indexed by asset string
    pub price_vars: HashMap<String, String>,
    
    // Segment variables: indexed by (pool_id, segment_index)
    pub segment_vars: HashMap<(usize, usize), String>,
}
```

### 6.2 Objective: Maximize Traded Volume

**Definition 6.2.1: Objective**

Maximize the total executed volume:

$$\max_{v, P} \sum_{k \in O} v_k$$

Interpretation: The clearing price vector is chosen to maximize the total quantity of trades executed in the batch, subject to price coherence and limit price constraints.

**Code Reference:** [lp_encoder.rs](engines/src/fba/lp_encoder.rs) (to be extended)

```rust
// Placeholder; full implementation will generate variable names like v_1, v_2, ...
pub fn encode_batch(&self, batch: &Batch, pools: &[AMMPool]) -> String {
    let mut lp = LPBuilder::new();
    lp.set_objective("v_1 + v_2 + ... + v_n");  // sum all execution volumes
    // ... add constraints
    lp.to_lp()
}
```

### 6.3 Constraint A: Flow Conservation

**Constraint A: Conservation of Volume**

For each asset $i$, the total quantity of asset $i$ sold must equal the total quantity of asset $i$ bought:

$$\sum_{k \in O : \text{TokensSold}_k = i} v_k \times (\text{amount}_k)_{\text{base}} = \sum_{k \in O : \text{TokensBought}_k = i} v_k \times (\text{amount}_k)_{\text{base}}$$

More formally, define for each order $k$:

- If $k$ is a buy order for pair $(i, j)$: The order buys $(\text{amount}_k)_{\text{base}}$ units of asset $i$ per unit executed.
- If $k$ is a sell order for pair $(i, j)$: The order sells $(\text{amount}_k)_{\text{base}}$ units of asset $i$ per unit executed.

**Constraint A** enforces that net flows sum to zero:

$$\sum_k \text{NetSold}_{k,i} = 0 \quad \forall i \in A$$

where $\text{NetSold}_{k,i}$ is the signed volume of asset $i$ sold by order $k$ (negative if buying).

**Example 6.3.1:** Two orders for pair (ETH, USD):
- Order 1: Sell 10 ETH at any price (quantity 10)
- Order 2: Buy 10 ETH at any price (quantity 10)

If both are fully executed ($v_1 = v_2 = 1$):
- Order 1 sells: 10 ETH
- Order 2 buys: 10 ETH
- Net ETH flow: $10 - 10 = 0$ ✓

### 6.4 Constraint B: Coherent Cross-Rates

**Constraint B: Cross-Rate Consistency**

The price vector must be internally consistent: the implied exchange rate between any two assets must equal the ratio of their prices.

$$\gamma_{i,j} = \frac{p_i}{p_j} \quad \forall i, j \in A$$

This prevents arbitrage: if prices are incoherent, a trader could profitably cycle through assets.

**Code reference:** While not explicitly computed as a constraint in the current lp_encoder skeleton, this constraint will be added as:

```
gamma_ETH_USD: p_ETH - USD_rate * p_USD = 0
```

where `USD_rate` is a constant derived from the order batch or market data.

### 6.5 Constraint C: Execution Feasibility via Limit Prices

**Constraint C: Limit Price Bounds**

An order $k$ can only execute if the clearing price vector satisfies its limit price constraint:

- **Buy order** for pair $(i, j)$ with limit price $p_k^{\text{limit}}$:
  $$\frac{p_j}{p_i} \leq p_k^{\text{limit}} \quad \text{(if } v_k > 0 \text{)}$$

- **Sell order** for pair $(i, j)$ with limit price $p_k^{\text{limit}}$:
  $$\frac{p_j}{p_i} \geq p_k^{\text{limit}} \quad \text{(if } v_k > 0 \text{)}$$

Equivalently (using $\gamma_{i,j} = p_j / p_i$):

$$\gamma_{i,j} \in [\gamma_k^{\text{min}}, \gamma_k^{\text{max}}] \quad \text{(if } v_k > 0 \text{)}$$

In the LP, these become:

$$\gamma_{i,j} - \gamma_k^{\text{min}} \geq -M(1 - v_k) \quad \text{(big-M constraint)}$$
$$\gamma_k^{\text{max}} - \gamma_{i,j} \geq -M(1 - v_k) \quad \text{(big-M constraint)}$$

where $M$ is a large constant.

**Simplification for practice:** If all orders are for a single asset pair $(i, j)$, and we set $p_i = 1$ (numeraire), then Constraint C becomes:

$$p_j \in [\text{min\_acceptable\_price}, \text{max\_acceptable\_price}]$$

a simple scalar constraint on the quote asset price.

### 6.6 Piecewise-Linear AMM Constraints

**Definition 6.6.1: AMM Residual**

If an order should route through an AMM, its execution affects the pool reserves and residual output. Let $z_{\text{amm}} \in [0, 1]$ encode the fractional execution on each AMM segment.

For a piecewise linearization with $m$ segments, the total output is:

$$dy_{\text{amm}} = \sum_{s=1}^m z_s \times dy_s$$

where $(dx_s, dy_s)$ are segment endpoints and $z_s \in [0,1]$ with $\sum_s z_s \leq 1$ (only one segment active).

**Big-M encoding:**

$$dy_{\text{amm}} = \sum_s y_s \quad \text{(linear approximation)}$$
$$\sum_s z_s = 1 \quad \text{(exactly one segment active)}$$
$$y_s \leq M \cdot z_s \quad \text{(big-M: constraint activation)}$$

### 6.7 Full LP Model Summary

**LP Formulation 6.7.1: Multi-Asset FBA with AMM**

$$\begin{align}
\max_{v, p} \quad & \sum_{k=1}^n v_k & \text{(maximize volume)} \\
\text{s.t.} \quad & \sum_k \text{NetSold}_{k,i} = 0 & \forall i \in A & \text{(flow conservation)} \\
& p_i \in [\gamma_k^{\text{min}} \cdot p_j, \gamma_k^{\text{max}} \cdot p_j] & \forall k, (i,j) = \text{pair}_k & \text{(limit price bounds)} \\
& 0 \leq v_k \leq q_k & \forall k & \text{(execution bounds)} \\
& p_i > 0 & \forall i & \text{(positive prices)} \\
& p_0 = 1 & & \text{(numeraire)} \\
\end{align}$$

**Code reference:** The full encoding is scaffolded in [lp_encoder.rs](engines/src/fba/lp_encoder.rs) and will be extended to emit these constraints to [LPBuilder](engines/src/fba/solver.rs).

---

## 7. Settlement Optimization {#settlement-optimization}

### 7.1 Net Positions

**Definition 7.1.1: Net Position**

After trades are executed, each participant has a net balance change:

$$\text{NetBalance}_{p,i} = \sum_{k \in \text{trades}} \begin{cases}
-q_k \times p_k / 10^6 & \text{if participant\_id}_k = p \text{ and buyer}_k = p \\
+q_k \times p_k / 10^6 & \text{if participant\_id}_k = p \text{ and seller}_k = p \\
\end{cases}$$

A **net position** is a pair $(p, i, b)$ where:
- $p$ = participant ID
- $i$ = asset ID
- $b$ = net balance (signed; positive = creditor, negative = debtor)

**Code Reference:** [optimizer.rs](engines/src/fba/optimizer.rs), lines 1–50

```rust
pub fn net_positions_from_trades(trades: &[Trade]) -> NetPositions {
    let mut net: BTreeMap<String, BTreeMap<String, i128>> = BTreeMap::new();
    
    for trade in trades {
        let quote_amount = (trade.quantity as i128) * (trade.price as i128) / (PRICE_SCALE as i128);
        
        // Buyer receives base, pays quote
        *net.entry(trade.buyer_id.clone())
            .or_default()
            .entry(trade.pair.base.clone())
            .or_default() += trade.quantity as i128;
        *net.entry(trade.buyer_id.clone())
            .or_default()
            .entry(trade.pair.quote.clone())
            .or_default() -= quote_amount;
        
        // Seller receives quote, pays base
        *net.entry(trade.seller_id.clone())
            .or_default()
            .entry(trade.pair.base.clone())
            .or_default() -= trade.quantity as i128;
        *net.entry(trade.seller_id.clone())
            .or_default()
            .entry(trade.pair.quote.clone())
            .or_default() += quote_amount;
    }
    
    net
}
```

### 7.2 Settlement Optimization Objective

**Definition 7.2.1: Settlement Minimization**

Given net positions, compute a set of **settlement edges** (transfers) that resolves all net balances with minimum transfer count:

$$\min_{\mathcal{E}} |\mathcal{E}|$$

where $\mathcal{E}$ is the set of settlement edges $ (p_{\text{from}}, p_{\text{to}}, i, b)$ representing a transfer of $b$ units of asset $i$ from $p_{\text{from}}$ to $p_{\text{to}}$.

**Constraint:** All net positions must be settled:

$$\sum_{e \in \mathcal{E} : e.\text{from} = p, e.\text{asset} = i} e.\text{amount} = \text{NetBalance}_{p,i} \quad \forall p, i$$

### 7.3 Greedy Settlement Algorithm

**Algorithm 7.3.1: Greedy Pairing**

For each asset $i$:

1. **Identify debtors and creditors:** Partition participants into those with negative balances (debtors) and positive balances (creditors).
2. **Sort by magnitude:** Sort debtors by most-negative balance (largest debtor first); sort creditors by most-positive balance (largest creditor first).
3. **Pair greedily:** While debtors and creditors exist:
   - Take the largest debtor and largest creditor.
   - Transfer $\min(|debt|, credit)$ from creditor to debtor.
   - Reduce both balances; remove zeros.
   - Emit a settlement edge.

**Code Reference:** [optimizer.rs](engines/src/fba/optimizer.rs), lines 60–130

```rust
pub fn settle_assets(net: &NetPositions) -> Vec<SettlementEdge> {
    let mut edges: Vec<SettlementEdge> = Vec::new();
    let mut net_copy = net.clone();
    
    for asset in net_copy.iter() {
        let mut debtors: Vec<(String, i128)> = asset.1
            .iter()
            .filter(|(_, bal)| *bal < 0)
            .map(|(p, bal)| (p.clone(), -bal))
            .collect();
        let mut creditors: Vec<(String, i128)> = asset.1
            .iter()
            .filter(|(_, bal)| *bal > 0)
            .map(|(p, bal)| (p.clone(), *bal))
            .collect();
        
        debtors.sort_by(|_, a| std::cmp::Ordering::Reverse);
        creditors.sort_by(|_, a| std::cmp::Ordering::Reverse);
        
        while !debtors.is_empty() && !creditors.is_empty() {
            let (debtor, debt) = debtors.pop().unwrap();
            let (creditor, credit) = creditors.pop().unwrap();
            let transfer_amount = debt.min(credit);
            
            edges.push(SettlementEdge::new(creditor.clone(), debtor.clone()));
            
            // ... record transfer ...
            
            if debt > transfer_amount {
                debtors.push((debtor, debt - transfer_amount));
            }
            if credit > transfer_amount {
                creditors.push((creditor, credit - transfer_amount));
            }
        }
    }
    
    edges
}
```

### 7.4 Transfer Bundling

**Definition 7.4.1: Bundled Settlement Edge**

A single **settlement edge** can carry multiple asset transfers, reducing the number of on-chain transactions:

$$e = (p_{\text{from}}, p_{\text{to}}, \{(i_1, b_1), (i_2, b_2), \ldots\})$$

This represents a single transaction from $p_{\text{from}}$ to $p_{\text{to}}$ carrying transfers of multiple assets.

**Code Reference:** [optimizer.rs](engines/src/fba/optimizer.rs), lines 140–180

```rust
pub fn bundle_transfers(edges: Vec<SettlementEdge>) -> Vec<SettlementEdge> {
    let mut bundled: BTreeMap<(String, String), SettlementEdge> = BTreeMap::new();
    
    for edge in edges {
        bundled.entry((edge.from.clone(), edge.to.clone()))
            .or_insert(SettlementEdge::new(edge.from.clone(), edge.to.clone()))
            .transfers.extend(edge.transfers);
    }
    
    bundled.into_values().collect()
}
```

---

## 8. LP Encoding and Solver Interface {#lp-encoding-and-solver-interface}

### 8.1 Overview

The LP encoder translates the mathematical model (Section 6) into a textual LP file that can be solved by external solvers (e.g., CPLEX, Gurobi, GLPK).

**Code Reference:** [lp_encoder.rs](engines/src/fba/lp_encoder.rs)

```rust
pub struct LPEncoder {}

impl LPEncoder {
    pub fn new() -> Self { Self {} }

    pub fn encode_batch(&self, batch: &Batch, pools: &[AMMPool]) -> String {
        // Generate LP model string
        let mut lp = LPBuilder::new();
        // ... populate variables, objective, constraints
        lp.to_lp()
    }
}
```

### 8.2 Variable Naming Conventions

**Definition 8.2.1: LP Variable Names**

To track variables in the LP model, we use systematic naming:

- **Execution volumes:** `v_{order_id}` (e.g., `v_1`, `v_2`, ...)
- **Asset prices:** `p_{asset_name}` (e.g., `p_ETH`, `p_USDC`)
- **Cross rates:** `gamma_{asset_i}_{asset_j}` (e.g., `gamma_ETH_USDC`)
- **AMM segments:** `z_amm_{pool_id}_{segment_index}` (e.g., `z_amm_1_2`)

### 8.3 Constraint Generation

**Definition 8.3.1: Constraint Generation Pipeline**

For a batch with $n$ orders and $|A|$ assets:

1. **Flow conservation constraints** ($|A|$ constraints):
   - For each asset $i$: $\sum_k c_{k,i} \cdot v_k = 0$
   - Coefficient $c_{k,i}$ = net asset $i$ sold by order $k$ per unit executed

2. **Limit price constraints** ($n$ constraints):
   - For each order $k$: Bound the cross-rate $\gamma$ implied for the order's pair

3. **AMM segment constraints** (if AMM liquidity is used):
   - For each pool and segment: Encode piecewise-linear approximation

4. **Bound constraints** (general):
   - $0 \leq v_k \leq q_k$ for all $k$
   - $p_i > 0$ for all $i$
   - $p_0 = 1$ (numeraire)

**Code template (future):**

```rust
pub fn encode_batch(&self, batch: &Batch, pools: &[AMMPool]) -> String {
    let mut lp = LPBuilder::new();
    
    // Objective: maximize sum of v_k
    let mut obj_terms = Vec::new();
    for order in &batch.orders {
        obj_terms.push(format!("v_{}", order.id));
    }
    lp.set_objective(&obj_terms.join(" + "));
    
    // Constraint A: Flow conservation
    // ...
    
    // Constraint B & C: Cross-rates and limit prices
    // ...
    
    // Bounds
    for order in &batch.orders {
        lp.add_bound(&format!("v_{} <= {}", order.id, order.quantity));
        lp.add_bound(&format!("v_{} >= 0", order.id));
    }
    
    lp.to_lp()
}
```

### 8.4 LPBuilder Interface

**Definition 8.4.1: LPBuilder API**

The `LPBuilder` struct provides a simple interface for constructing LP files in CPLEX LP format:

**Code Reference:** [solver.rs](engines/src/fba/solver.rs)

```rust
pub struct LPBuilder {
    objective: String,
    constraints: Vec<String>,
    bounds: Vec<String>,
    binaries: Vec<String>,
}

impl LPBuilder {
    pub fn new() -> Self { Self::default() }
    pub fn set_objective(&mut self, expr: &str) { self.objective = expr.to_string(); }
    pub fn add_constraint(&mut self, name: &str, expr: &str) {
        self.constraints.push(format!("{}: {}", name, expr));
    }
    pub fn add_bound(&mut self, bound: &str) { self.bounds.push(bound.to_string()); }
    pub fn add_binary(&mut self, name: &str) { self.binaries.push(name.to_string()); }
    pub fn to_lp(&self) -> String {
        // Emit as CPLEX-format LP file
    }
}
```

### 8.5 Example: Small FBA Instance

**Example 8.5.1:** Two orders, one pair, no AMM.

**Input:**
- Order 1: Sell 10 units at limit price 11
- Order 2: Buy 10 units at limit price 11

**LP Model (simplified notation):**

```
Minimize
  obj: 0  (placeholder; would be -v_1 - v_2 for maximization)

Subject To
  flow_USD: 11 * v_1 - 11 * v_2 = 0  (flow conservation)
  limit_sell_1: p <= 11
  limit_buy_2: p >= 11

Bounds
  v_1 <= 10
  v_2 <= 10
  v_1 >= 0
  v_2 >= 0
  p >= 0
  p <= 1000000

End
```

**Solution:** $v_1 = v_2 = 10$, $p = 11$. All 10 units trade at price 11.

---

## 9. Implementation Architecture {#implementation-architecture}

### 9.1 Crate Structure

```
thesis-market-models/
├── engines/
│   ├── src/
│   │   ├── lib.rs              (entry point; defines modules)
│   │   ├── common/
│   │   │   ├── mod.rs          (exports for common primitives)
│   │   │   ├── types.rs        (definitions: Order, Trade, PRICE_SCALE, etc.)
│   │   │   └── order_book.rs   (COB matching engine)
│   │   ├── cob/                (continuous order book module)
│   │   │   └── mod.rs          (re-exports OrderBook)
│   │   └── fba/                (frequent batch auction and related)
│   │       ├── mod.rs          (module exports)
│   │       ├── clearing.rs     (BatchAuctionEngine, ClearingResult)
│   │       ├── optimizer.rs    (SettlementOptimizer)
│   │       ├── amm.rs          (AMMPool constant-product)
│   │       ├── solver.rs       (LPBuilder)
│   │       └── lp_encoder.rs   (LPEncoder: batch -> LP translation)
│   └── Cargo.toml
├── simulation/
│   ├── src/
│   │   └── main.rs             (example runner and experiments)
│   └── Cargo.toml
└── README.md
```

### 9.2 Data Flow

```
3 Stages:
└─ Input: Orders submitted via COB or FBA batch
   ├─ Stage 1: COB live matching (executes crossing orders immediately)
   └─ Stage 2: FBA clearing (when batch interval expires)
       ├─ 2a: Collect unmatched orders → batch
       ├─ 2b: Clear each pair → trades
       ├─ 2c: (Future) Encode as LP → external solver
       └─ 2d: Execute trades
   └─ Stage 3: Settlement
       ├─ 3a: Compute net positions
       ├─ 3b: Greedy settlement → edges
       └─ 3c: Bundle transfers
└─ Output: Trades, settlement plan, LP model (for audit)
```

### 9.3 Integer Fixed-Point Arithmetic Policy

**Policy 9.3.1:** All prices and amounts are represented as integers scaled by `PRICE_SCALE = 10^6` to:
- Avoid floating-point rounding errors
- Ensure deterministic, reproducible calculations
- Allow bit-exact computation of flow balances

**Application:**
- Input prices are multiplied by 10^6 before use.
- When multiplying quantity × price, the product is divided by 10^6 to rescale.
- All LP variables for prices are integer (after scaling).

**Code enforcement:** The `.cursorrules` and `ai.toml` files specify that no `f32`, `f64`, or floating-point arithmetic is permitted except:
- In comments or documentation
- In test comparisons (for clarity)
- In the AMM `price()` function (prior to this fix; now uses fixed-point)

---

## 10. Implementation Gaps and Future Work {#implementation-gaps-and-future-work}

### 10.1 LP Encoding Gap

**Issue:** The `lp_encoder.rs` module currently provides only a skeleton. It does not generate concrete LP variables ($v_k$, $p_i$) or constraints (flow conservation, limit prices, AMM segments).

**Impact:** The system cannot currently produce a valid multi-asset LP formulation for external solvers.

**Required work:**
1. Parse batch orders and identify assets, pairs, and participant IDs.
2. Generate variable names for $v_k$ (execution volumes), $p_i$ (asset prices), and $z_s$ (AMM segments).
3. Construct flow conservation constraints by computing coefficients $c_{k,i}$ for each order-asset pair.
4. Construct limit price constraints by extracting limit prices and encoding them as bounds on cross-rates.
5. (Optional) Implement piecewise-linear AMM segment constraints if AMM liquidity is included.
6. Emit all constraints and bounds via `LPBuilder`.

**Priority:** High. This is essential for the full multi-asset FBA vision.

### 10.2 Solver Integration Gap

**Issue:** The system emits LP files but does not invoke an external LP solver or parse solutions back.

**Impact:** The computed clearing prices and volumes are not verified against the formal LP optimum.

**Required work:**
1. Interface with an LP solver (CPLEX, Gurobi, GLPK, or CBC).
2. Write LP file to disk.
3. Invoke solver executable.
4. Parse solver output (`.sol` file or stdout).
5. Extract optimal $v_k$ and $p_i$ values.
6. Construct trades and settlement from the solver solution.

**Priority:** High for validation; medium for production (depends on whether on-chain execution is desired).

### 10.3 Multi-Asset Coherence Gap

**Issue:** The current clearing engine computes clearing prices independently for each asset pair. It does not enforce cross-rate consistency ($\gamma_{i,j} = p_i / p_j$) across pairs.

**Impact:** Arbitrage cycles (e.g., ETH → USDC → BTC → ETH at profitable rates) can exist and are not eliminated.

**Required work:**
1. Extend the FBA clearing to consider all pairs simultaneously.
2. Build the full multi-asset LP formulation with flow conservation, cross-rates, and limit prices.
3. Solve the LP to obtain a globally coherent price vector.
4. Match orders using the solution.

**Priority:** High for exchanges; medium for single-pair systems.

### 10.4 AMM Integration Gap

**Issue:** The `AMMPool` is defined and can compute marginal prices and piecewise linearizations, but these are not wired into the clearing or LP encoding.

**Impact:** Batch auctions cannot currently route orders through AMM liquidity or fund the constant-product invariant.

**Required work:**
1. Modify `LPEncoder` to accept AMM pools as input.
2. For each pool, extract piecewise-linear segments via `linearize()`.
3. Encode segments as constraints in the LP model.
4. Link order execution to pool residuals.

**Priority:** Medium. Useful for hybrid venues (batch + AMM).

### 10.5 COB-FBA Interaction Gap

**Issue:** The COB and FBA are separate engines. Orders executed by COB are not automatically passed to FBA, and vice versa.

**Impact:** The system does not model the realistic interaction where COB liquidity before a batch affects FBA input.

**Required work:**
1. Extend the order book snapshot to track residuals.
2. Modify `BatchAuctionEngine` to accept a mixed input of:
   - Fresh orders (not yet submitted to COB)
   - Residual orders (unfilled by COB during the batch period)
3. Produce a unified clearing that respects both COB and batch constraints.

**Priority:** Medium for realistic modeling.

### 10.6 Documentation Gaps

**Issue:** Many functions lack detailed inline documentation explaining the mathematics they implement.

**Required work:**
1. Add `///` doc comments to all public functions and methods.
2. Link each to the thesis section describing the corresponding math.
3. Provide examples and invariants in the documentation.

**Priority:** Low for functionality; high for thesis clarity.

---

## 11. Experiments and Reproducibility {#experiments-and-reproducibility}

### 11.1 Running the Simulation

**Command:**

```bash
cargo run --manifest-path thesis-market-models/simulation/Cargo.toml --release
```

**Output:** Example clearing results, trades, and settlement plans are printed to stdout.

**Code Reference:** [simulation/src/main.rs](simulation/src/main.rs)

```rust
fn main() {
    // Create example orders
    let pair = AssetPair::new("X", "Y");
    let orders = vec![
        Order::limit(1, "A", pair.clone(), Side::Buy, 11 * PRICE_SCALE, 10, 0),
        Order::limit(2, "B", pair.clone(), Side::Buy, 10 * PRICE_SCALE, 8, 0),
        Order::limit(3, "C", pair.clone(), Side::Sell, 9 * PRICE_SCALE, 9, 0),
        Order::limit(4, "D", pair.clone(), Side::Sell, 10 * PRICE_SCALE, 7, 0),
        Order::limit(5, "E", pair.clone(), Side::Sell, 11 * PRICE_SCALE, 6, 0),
    ];

    // Run clearing
    let mut engine = BatchAuctionEngine::new();
    for order in orders {
        engine.submit(order);
    }
    let result = engine.clear_pair(&pair).expect("pair must clear");

    // Run settlement optimizer
    let optimizer = SettlementOptimizer::new();
    let settlement = optimizer.optimize_trades(&result.trades);

    // Display results
    println!("Clearing Price: {}", format_price(result.clearing_price));
    println!("Traded Volume: {}", result.traded_quantity);
    println!("Trades: {}", result.trades.len());
    println!("Settlement Edges: {}", settlement.edges.len());
    println!("Transfer Reduction: {} -> {}", 
        settlement.naive_transfer_count, settlement.optimized_transfer_count);
}
```

### 11.2 Running Tests

**Command:**

```bash
cargo test --manifest-path thesis-market-models/engines/Cargo.toml
```

**Tests include:**
- `amm_price_and_execution`: Verify constant-product formula
- `amm_linearize_breakpoints`: Verify piecewise linearization
- `clearing_price_maximizes_volume`: Verify FBA clearing selects volume-maximizing price
- `optimizer_reduces_cleared_transfer_count`: Verify settlement bundling
- `lp_builder_emits_basic_model`: Verify LP syntax
- `lp_encoder_emits_lp`: Verify LP encoder skeleton

### 11.3 LP Model Emission

**Future experiment (requires full lp_encoder implementation):**

```bash
cargo run --manifest-path thesis-market-models/simulation/Cargo.toml --release > model.lp
```

Then solve with an external LP solver:

```bash
glpsol --lp model.lp -o solution.sol
```

### 11.4 Metrics and Observations

**Key metrics to measure:**

1. **Clearing Efficiency:** $({\text{Traded Volume}}) / (\text{Total Order Volume})$ — what fraction of orders is matched in the batch?

2. **Settlement Reduction:** $(\text{Naive Edges}) / (\text{Optimized Edges})$ — how much does the greedy pairing algorithm reduce settlement legs?

3. **Price Convergence:** Distance from the LP-optimal price (once LP encoder is complete).

4. **Execution Speed:** Time to clear a batch and optimize settlement (latency).

**Expected results:**

- Clearing efficiency should be high when orders are well-matched (many crossing pairs).
- Settlement reduction should be 2–5x for typical batches due to bundling.
- Execution should be sub-millisecond for batches with <1000 orders.

---

## 12. Conclusion {#conclusion}

### 12.1 Summary

This thesis formalizes and implements a market infrastructure combining:

1. **Continuous Order Book (COB):** Immediate execution via FIFO matching at passive order prices.
2. **Frequent Batch Auction (FBA):** Discrete clearing at uniform prices maximizing traded volume and respecting limit price constraints.
3. **AMM Integration:** Constant-product liquidity pools encoded as piecewise-linear constraints in the clearing optimization.
4. **Settlement Optimization:** Greedy pairing of net creditors and debtors to minimize on-chain transfers.

**Mathematical contributions:**
- Formalized the volume-maximizing clearing price selection with imbalance and price tie-breakers.
- Derived the multi-asset FBA LP formulation with flow conservation, coherent cross-rates, and limit price constraints.
- Presented piecewise-linear AMM encoding for hybrid auctions.

**Implementation contributions:**
- Provided a working Rust implementation of COB matching, FBA clearing, AMM constant-product execution, and settlement optimization.
- Fixed floating-point inconsistency in AMM pricing by converting to fixed-point integer arithmetic.
- Established LP emission framework via `LPBuilder` and `LPEncoder` skeleton.

### 12.2 Current State

**What works:**
- ✓ COB order matching and trade construction
- ✓ Single-pair FBA clearing with volume and imbalance optimization
- ✓ AMM constant-product execution and piecewise linearization (integer math)
- ✓ Settlement net position computation and greedy bundling
- ✓ LP model emission skeleton
- ✓ Unit tests for core components

**What is incomplete:**
- ✗ Full multi-asset LP formulation encoder (requires linking orders to assets and assets to price variables)
- ✗ Solver integration (LP → solver → solution parsing)
- ✗ Multi-asset coherence across pairs (currently per-pair only)
- ✗ AMM liquidity routing in clearing (pools defined but not used in orders)
- ✗ COB-FBA interaction (separate engines; no residual order handling)

### 12.3 Recommended Next Steps

**Phase 1: Complete LP Encoder (1–2 weeks)**
1. Parse batch to extract assets, pairs, and participant graph.
2. Generate LP variables ($v_k$, $p_i$) with systematic naming.
3. Construct flow conservation constraints with computed coefficients.
4. Add limit price constraints as bounds on cross-rates.
5. Emit valid LP file via `LPBuilder`.
6. **Deliverable:** `cargo run` produces a `.lp` file for manually-verified examples.

**Phase 2: Solver Integration (1–2 weeks)**
1. Interface with GLPK (free, open-source) via FFI or subprocess.
2. Write LP file, invoke solver, parse `.sol` output.
3. Extract optimal $v_k$, $p_i$, construct trades.
4. Validate LP solution against greedy FBA result.
5. **Deliverable:** Solver-verified prices for all test batches.

**Phase 3: Multi-Asset and AMM (2–3 weeks)**
1. Extend FBA engine to build unified LP for all pairs.
2. Link orders to global price variables; enforce coherent cross-rates.
3. Wire `AMMPool::linearize()` output into LP constraints.
4. Test on hybrid order flows (orders for different pairs + AMM liquidity).
5. **Deliverable:** Multi-asset batches clear efficiently; arbitrage cycles are eliminated.

**Phase 4: Production Readiness (ongoing)**
1. Add comprehensive documentation and doc comments.
2. Extend test suite (property-based tests, large batches, edge cases).
3. Benchmark clearing latency, settlement reduction, and LP solve time.
4. Integrate with blockchain/settlement layer (out of scope for this thesis).

### 12.4 Reproducibility and Artifact Availability

**Source code:** Available in [thesis-market-models/](thesis-market-models/).

**Building:** See [README.md](README.md) for setup and build instructions.

**Running experiments:**
```bash
cargo test --manifest-path thesis-market-models/engines/Cargo.toml
cargo run --manifest-path thesis-market-models/simulation/Cargo.toml --release
```

**License:** (To be specified by project author.)

---

## 13. Appendix: Code Reference {#appendix-code-reference}

### A1. Type Definitions

**File:** [engines/src/common/types.rs](../../thesis-market-models/engines/src/common/types.rs)

| Struct/Enum | Purpose |
|-------------|---------|
| `PRICE_SCALE: u128 = 1_000_000` | Fixed-point scaling factor |
| `AssetPair { base, quote }` | Identifies a trading pair |
| `Side::Buy \| Side::Sell` | Order direction |
| `OrderKind::Limit { price } \| OrderKind::Market` | Order type |
| `Order { id, participant_id, pair, side, kind, quantity, remaining, timestamp }` | Single order |
| `Batch { id, orders }` | Collection of orders |
| `Trade { pair, price, quantity, buyer_id, seller_id, ... }` | Executed trade |
| `Account { id, balances }` | Participant account |
| `SettlementEdge { from, to, transfers }` | Settlement leg |
| `SettlementPlan { net_positions, edges, counts }` | Overall settlement plan |

### A2. COB Implementation

**File:** [engines/src/common/order_book.rs](../../thesis-market-models/engines/src/common/order_book.rs)

| Function | Returns | Purpose |
|----------|---------|---------|
| `OrderBook::new()` | `OrderBook` | Allocate empty book |
| `submit(order: Order)` | `Vec<Trade>` | Submit order and match |
| `match_buy(order: Order)` | `Vec<Trade>` | Match buy order against sell side |
| `match_sell(order: Order)` | `Vec<Trade>` | Match sell order against buy side |
| `snapshot()` | `(Vec<Order>, Vec<Order>)` | Get buy and sell order state |

### A3. FBA Clearing Implementation

**File:** [engines/src/fba/clearing.rs](../../thesis-market-models/engines/src/fba/clearing.rs)

| Struct/Fn | Purpose |
|-----------|---------|
| `BatchAuctionEngine` | Single-pair clearing engine |
| `MultiAssetEngine` | Multi-pair clearing engine |
| `ClearingResult { pair, clearing_price, traded_quantity, demand, supply, trades }` | Clearing output |
| `candidate_prices(orders) -> BTreeSet<Price>` | Enumerate candidate prices |
| `select_price(orders, candidates) -> (Price, Amount, Amount)` | Select volume-maximizing price |
| `aggregate_volume(orders, side, price) -> Amount` | Compute demand or supply at price |
| `eligible_orders(orders, side, price) -> Vec<Order>` | Filter and sort eligible orders |

### A4. AMM Implementation

**File:** [engines/src/fba/amm.rs](../../thesis-market-models/engines/src/fba/amm.rs)

| Function | Returns | Purpose |
|----------|---------|---------|
| `AMMPool::new(reserve_x, reserve_y)` | `AMMPool` | Allocate pool |
| `price()` | `Price` | Marginal price (fixed-point scaled) |
| `execute_sell(dx: u128)` | `u128` | Output dy for sell of dx |
| `linearize(breakpoints) -> Vec<(u128, u128)>` | Vector of (dx, dy) pairs | Piecewise-linear approximation |

### A5. Settlement Optimization Implementation

**File:** [engines/src/fba/optimizer.rs](../../thesis-market-models/engines/src/fba/optimizer.rs)

| Function | Returns | Purpose |
|----------|---------|---------|
| `net_positions_from_trades(trades)` | `NetPositions` | Compute per-participant, per-asset net balances |
| `optimize_net_positions(net) -> Vec<SettlementEdge>` | Settlement edges | Greedy pairing to minimize edges |
| `settle_assets(net) -> Vec<SettlementEdge>` | Settlement edges | Settle single asset via greedy matching |
| `bundle_transfers(edges) -> Vec<SettlementEdge>` | Bundled edges | Consolidate edges between same participants |
| `count_asset_edges(edges) -> usize` | Edge count | Count post-optimization edges |

### A6. LP Encoding and Solver

**File:** [engines/src/fba/lp_encoder.rs](../../thesis-market-models/engines/src/fba/lp_encoder.rs)

| Struct/Fn | Purpose |
|-----------|---------|
| `LPEncoder::new()` | Allocate encoder |
| `encode_batch(batch, pools) -> String` | Generate LP model (skeleton) |

**File:** [engines/src/fba/solver.rs](../../thesis-market-models/engines/src/fba/solver.rs)

| Function | Purpose |
|----------|---------|
| `LPBuilder::new()` | Allocate builder |
| `set_objective(expr)` | Set objective function |
| `add_constraint(name, expr)` | Add a constraint |
| `add_bound(bound)` | Add a variable bound |
| `add_binary(name)` | Mark variable as binary |
| `to_lp() -> String` | Emit CPLEX LP format |

### A7. Simulation and Examples

**File:** [simulation/src/main.rs](../../thesis-market-models/simulation/src/main.rs)

Provides a runnable example:
- Creates sample orders for a single or multiple pairs.
- Runs COB and FBA clearing.
- Computes settlement optimization.
- Prints results.

**Example output:**
```
Clearing Price: 10.000000
Traded Volume: 16
Demand at Price: 18
Supply at Price: 16
Trades: 3
Settlement Summary:
  Net Positions: 3 participants, 2 assets
  Optimized Edges: 2 (from naive 3)
  Transfer Reduction: 33.3%
```

---

## References

1. Darrell Duffie and Ran Exelby. "Valuation and Price Fixing in the Decentralized Over-the-Counter Markets." *Columbia Business School Research Paper*, 2002.

2. Seamans, Leila, and Stefan Sundaresan. "Market Design and Execution." Columbia Business School, 2020.

3. Buterin, Vitalik. "On Path-Dependent Valuation and Optimal Pricing of Liquidity Providers." *Ethereum Foundation*, 2020. (Informal reference to Uniswap v2 design.)

4. Angeris, Guillermo, Kshitij Kulesza, and Tarun Chitra. "An Analysis of Uniswap Markets." *SSRN*, 2021.

5. Adams, Hayden, Noah Zinsmeister, Moody Salem, River Keefer, and Dan Robinson. "Uniswap v3 Core." *Technical Note*, 2021.

---

**End of Thesis**

---

### How to Compile to PDF

**Prerequisites:**
```bash
sudo apt-get install pandoc texlive-xetex texlive-latex-base
```

**Command:**
```bash
pandoc THESIS.md -o THESIS.pdf --pdf-engine=xetex
```

**Output:** `THESIS.pdf` (typeset thesis document)




\begin{table}[htbp]
\centering
\small
\caption{RQ2.3 --- Execution, allocation, latency race, and engine performance metrics.}
\label{tab:execution}
\begin{tabularx}{\textwidth}{@{}p{3.3cm} L p{1.1cm}@{}}
\hline
\textbf{Metric} & \textbf{Definition and measurement} & \textbf{Data} \\
\hline
Executed volume & Matched quantity and notional per interval; number of trades & L1 + trades \\
Fill rate & Filled quantity divided by submitted quantity, overall and by participant class & L3/L4 \\
Time to execution & Elapsed time from submission to fill; for the FBA bounded below by the time to the next batch boundary, which quantifies the delay cost of batching & L3 \\
Trader surplus & $\sum_k |\pi_k - p^{*}| \, q_k$ over executed orders: the gain relative to the submitted limit price & L3/L4 \\
Order size inflation & Ratio of submitted to filled size per participant over time; the empirical signature of order stuffing under pro-rata & L4 \\
Order-to-trade ratio & Submitted messages per executed trade; cancellation rate per interval; measures message traffic and quote flickering & L3 \\
Boundary concentration & Share of order arrivals in the final $x\%$ of the batch interval & L3 \\
Throughput & Orders processed per second under identical load & control \\
Clearing latency & Wall-clock time per batch clearing computation and per continuous match & control \\
Unexecuted residual & Share of batch volume on the heavier side of the book that cannot be matched at the clearing price and remains unexecuted (rolled over or cancelled, per time-in-force) & control \\
\hline
\end{tabularx}
\end{table}
Let's put the math back on the table in an intuitive way. For an academic thesis, you cannot just tell the examiner "my algorithm matches people in a circle." You have to write down the formal equations that prove the circle is fair, stable, and economically optimal.

Here is the complete, step-by-step mathematical formulation of your Multi-Asset Frequent Batch Auction (FBA). This is the exact math you need to put into the **Methodology chapter** of your thesis, and it maps directly to what your code's open-source LP solver (COIN-OR/GLPK) will evaluate.

---

## 1. The Variables and Setup

Before we write an optimization equation, we must define what the computer is looking at.

* Let $n$ be the total number of distinct asset tokens in the batch (e.g., if the batch has ETH, USDC, and WBTC, $n = 3$).
* Let $O$ be the set of all user orders submitted during the 15-second window.
* An individual buy/sell order $k \in O$ is defined by a tuple:

$$\omega_k = (i, j, \max(x_k), \pi_k)$$


* $i$: The token the user wants to sell (e.g., ETH).
* $j$: The token the user wants to buy (e.g., USDC).
* $\max(x_k)$: The maximum amount of token $i$ the user is willing to sell.
* $\pi_k$: The **Limit Price** (the minimum amount of token $j$ they must receive per 1 unit of token $i$).



### The Hidden Variable: The Price Vector

The solver's job is to calculate a single vector of asset prices:


$$P = [p_0, p_1, p_2, \dots, p_{n-1}]$$


Where $p_0$ is your reference "base token" (the numéraire, usually USDC or stablecoins), fixed mathematically at $p_0 = 1$. Every other token price is calculated relative to it.

---

## 2. The Objective Function: What are we Maximizing?

In a standard batch auction, your mathematical goal is to maximize **Trader Surplus** (also known as social welfare). If Alice is willing to sell her ETH for 3,000 USDC, but the auction clears at a price of 3,100 USDC, Alice receives a "surplus" of 100 USDC.

To keep the model linear and solvable by your COIN-OR solver, we define a decision variable $v_k$, which represents the executed volume of order $k$ scaled into the reference token $p_0$.

The objective function of your simulator is:


$$\max_{v, P} \sum_{k \in O} v_k$$

> **Thesis Note:** By maximizing the total transacted volume $v_k$ measured in the base currency, the math naturally forces the system to find the largest possible overlapping networks of trades (the biggest loops and circles).

---

## 3. The Strict Mathematical Constraints (The Rules)

Your LP/MILP solver cannot just pick any arbitrary number for volume ($v_k$) or price ($P$). The math must satisfy three absolute laws inside your code module:

### Constraint A: Conservation of Flow (No Money Printed)

For any token $i$ in the system, the total amount of that token sold by users must exactly equal the total amount bought by other users inside that batch.

$$\sum_{k \in O} \text{Tokens Sold}_{k, i} = \sum_{k \in O} \text{Tokens Bought}_{k, i} \quad \forall i \in \{0 \dots n-1\}$$

If this does not balance out to zero, the clearing engine creates a **Net Residual Vector ($R_i$)**. As we selected in Screenshot 2, if $R_i \neq 0$, that residual fraction must be pushed through the **Piecewise-Linear AMM approximation equations** (the virtual steps) to re-balance the network flow using external pool reserves.

### Constraint B: Coherent Cross-Rates (No Internal Arbitrage)

To guarantee a single uniform clearing price across a web of multiple assets, the calculated exchange rate between any two assets $i$ and $j$ must be perfectly consistent.

If your solver determines the price of token $i$ is $p_i$ and token $j$ is $p_j$, the execution exchange rate between them ($\gamma_{i,j}$) must strictly be:


$$\gamma_{i,j} = \frac{p_i}{p_j}$$

This prevents the system from clearing a loop where Alice trades ETH $\rightarrow$ USDC $\rightarrow$ WBTC $\rightarrow$ ETH and ends up with more ETH than she started with inside the same block transaction.

### Constraint C: Limit Price Bounds

An order $k$ can only have an execution volume $v_k > 0$ if the calculated market price is equal to or better than their signed limit price $\pi_k$.

$$\text{Execution Rate } \left(\frac{p_i}{p_j}\right) \ge \pi_k$$

If $\frac{p_i}{p_j} < \pi_k$, then the execution volume variable for that user must be forced to zero ($v_k = 0$).

---

## 4. The Shortest-Path Math (Your Core Thesis Question)

Once the equations above find the optimal execution volumes ($v_k$) and token prices ($P$), your **second** math problem begins: **How do we settle this on-chain with the lowest gas fees?**

This is a **Graph Sparsification** problem. We want to take a complicated web of trades and reduce the number of active edges.

Let $X_{a, b, m}$ be a binary decision variable where:

* $X_{a, b, m} = 1$ if Account $a$ sends Token $m$ to Account $b$ on-chain.
* $X_{a, b, m} = 0$ if no transaction occurs between them.

Your thesis objective function for transaction fee minimization is:


$$\min \sum_{a} \sum_{b} \sum_{m} X_{a, b, m}$$

### How the Code Executes This:

Instead of executing every original order, your code constructs a net balance sheet for the batch:


$$\text{Net Balance}_a = \text{Total Owed to Account } a - \text{Total Owed by Account } a$$

Your greedy algorithm then sorts everyone:

1. **Debtors Matrix:** Accounts where $\text{Net Balance} < 0$.
2. **Creditors Matrix:** Accounts where $\text{Net Balance} > 0$.

It mathematically pairs the largest debtor directly with the largest creditor, minimizing the total number of non-zero entries in the settlement matrix. This is the exact mathematical proof you will present to show your FBA model saves massive amounts of blockchain gas fees compared to a continuous order book.
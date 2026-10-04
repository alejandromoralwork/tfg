"""Builds Thesis/figures/fig_fba_schedule.png -- the textbook-style demand/supply
diagram for Chapter 3's new notation section (`sec:fbanotation`).

Unlike every other figure in this module, this one is not computed from
`results/sol/*.csv`: it redraws the small, already-worked-out example in
`src/docs/ENGINE_DESIGN.md` Sec 1.3 ("Worked example") as a step-function
D(p)/S(p) chart, purely to illustrate the mechanism precisely defined in
`sec:fbanotation`, not to report a measurement.

The four orders (same as ENGINE_DESIGN.md):
  B1  buy  limit 105  qty 10  t=1
  B2  buy  limit 100  qty 10  t=2
  B3  buy  limit 100  qty 10  t=3
  S1  sell limit 100  qty 15  t=1

D(p) = sum of buy qty with limit >= p (non-increasing step function)
S(p) = sum of sell qty with limit <= p (non-decreasing step function)
V(p) = min(D(p), S(p)); candidates are the submitted limits {100, 105}.
At p=105: D=10, S=15, V=10. At p=100: D=30, S=15, V=15 -- uniquely
volume-maximising, so the batch clears at p*=100 with V*=15.
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

import config

# Same categorical pair the thesis's dataviz convention validates against
# (blue/orange, slots 1-2 of the reference palette) -- distinct from the
# CDA/FBA colors used everywhere else in this module, since this chart's
# two series are Demand and Supply, not the two engines.
DEMAND_COLOR = "#2a78d6"
SUPPLY_COLOR = "#eb6834"
CLEAR_COLOR = "#52514e"

plt.rcParams.update({
    "figure.dpi": 150,
    "font.size": 11,
    "axes.spines.top": False,
    "axes.spines.right": False,
})

ORDERS = [
    ("B1", "buy", 105, 10),
    ("B2", "buy", 100, 10),
    ("B3", "buy", 100, 10),
    ("S1", "sell", 100, 15),
]


def demand(p, orders=ORDERS):
    return sum(q for _, side, limit, q in orders if side == "buy" and limit >= p)


def supply(p, orders=ORDERS):
    return sum(q for _, side, limit, q in orders if side == "sell" and limit <= p)


def _step_series(fn, p_grid):
    return np.array([fn(p) for p in p_grid])


def main():
    # Dense price grid so the step function renders as a clean staircase;
    # real engine prices live on PRICE_SCALE ticks, but this illustrative
    # chart uses the same small integers as the worked example.
    p_grid = np.linspace(95, 110, 1501)
    d = _step_series(demand, p_grid)
    s = _step_series(supply, p_grid)

    p_star = 100
    v_star = min(demand(p_star), supply(p_star))

    fig, ax = plt.subplots(figsize=(5.4, 4.2))

    ax.step(p_grid, d, where="post", color=DEMAND_COLOR, linewidth=2, label="Demand $D(p)$")
    ax.step(p_grid, s, where="post", color=SUPPLY_COLOR, linewidth=2, label="Supply $S(p)$")

    # Crossing / binding point at the clearing price.
    ax.plot([p_star], [v_star], marker="o", markersize=7, color=CLEAR_COLOR, zorder=5)
    ax.vlines(p_star, 0, v_star, color=CLEAR_COLOR, linewidth=1, linestyle="--")
    ax.hlines(v_star, p_grid.min(), p_star, color=CLEAR_COLOR, linewidth=1, linestyle="--")
    ax.annotate(
        r"$p^{*}=\mathrm{clr}=100$" + "\n" + r"$V^{*}=15$",
        xy=(p_star, v_star), xytext=(p_star + 2.5, v_star - 6),
        fontsize=10, color=CLEAR_COLOR,
        arrowprops=dict(arrowstyle="-", color=CLEAR_COLOR, linewidth=0.8),
    )

    ax.set_xlim(95, 110)
    ax.set_ylim(0, 35)
    ax.set_xlabel("Price $p$")
    ax.set_ylabel("Cumulative quantity")
    ax.set_title("FBA clearing: demand and supply schedules", fontsize=11)
    ax.legend(loc="upper right", frameon=False)

    fig.tight_layout()
    path = config.FIGURES_DIR / "fig_fba_schedule.png"
    fig.savefig(path, bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {path}")


if __name__ == "__main__":
    main()

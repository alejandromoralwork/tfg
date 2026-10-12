"""Builds Thesis/figures/fig_fba_schedule.png -- the demand/supply diagram
for Chapter 2's FBA section (`fig:fbaschedule` in ch2_background.tex).

Unlike every other figure in this module, this one is not computed from
`results/sol/*.csv`: it draws a small hand-made batch as step-function
D(p)/S(p) curves, purely to illustrate the clearing mechanism, not to report
a measurement.

The eight orders:
  B1  buy  limit 106  qty 10        S1  sell limit  96  qty 10
  B2  buy  limit 104  qty 15        S2  sell limit  99  qty 10
  B3  buy  limit 101  qty 15        S3  sell limit 101  qty 15
  B4  buy  limit  98  qty 10        S4  sell limit 104  qty 15

D(p) = sum of buy qty with limit >= p (non-increasing step function)
S(p) = sum of sell qty with limit <= p (non-decreasing step function)
V(p) = min(D(p), S(p)):
  p in [99, 101): D=40, S=20, V=20
  p = 101:        D=40, S=35, V=35   <- unique maximum
  p in (101, 104]: D=25, S>=35, V=25
so the batch clears at p*=101 with V*=35. Demand exceeds supply by 5 at p*,
so B3 (limit = p*) is rationed to 10 of its 15 and 5 roll over as residual;
B4 and S4 are not eligible at p* and roll over whole.

Surplus (eq:surplus) at p*=101:
  buyers  B1 (106-101)*10 + B2 (104-101)*15 = 50 + 45 = 95
  sellers S1 (101-96)*10  + S2 (101-99)*10  = 50 + 20 = 70
B3 and S3 trade at their own limit and gain nothing. With price on the
horizontal axis, buyer surplus is the area right of p* under D and seller
surplus the area left of p* under S; each order contributes one rectangle
(width = distance of its limit from p*, height = its executed quantity).
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.patches as patches
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
    ("B1", "buy", 106, 10),
    ("B2", "buy", 104, 15),
    ("B3", "buy", 101, 15),
    ("B4", "buy", 98, 10),
    ("S1", "sell", 96, 10),
    ("S2", "sell", 99, 10),
    ("S3", "sell", 101, 15),
    ("S4", "sell", 104, 15),
]

P_MIN, P_MAX = 94, 108


def demand(p, orders=ORDERS):
    return sum(q for _, side, limit, q in orders if side == "buy" and limit >= p)


def supply(p, orders=ORDERS):
    return sum(q for _, side, limit, q in orders if side == "sell" and limit <= p)


def _step_series(fn, p_grid):
    return np.array([fn(p) for p in p_grid])


def _clearing(orders=ORDERS):
    """Volume-maximising price over the submitted limits (unique here)."""
    limits = sorted({limit for _, _, limit, _ in orders})
    vols = {p: min(demand(p), supply(p)) for p in limits}
    v_star = max(vols.values())
    p_set = [p for p, v in vols.items() if v == v_star]
    assert len(p_set) == 1, f"example must clear without a tie, got {p_set}"
    return p_set[0], v_star


def _surplus_rectangles(p_star, side):
    """One (price_from, price_to, q_from, q_to) box per inframarginal order,
    stacked in priority order so the union is the area under the curve."""
    if side == "buy":
        ranked = sorted((o for o in ORDERS if o[1] == "buy" and o[2] > p_star),
                        key=lambda o: -o[2])
    else:
        ranked = sorted((o for o in ORDERS if o[1] == "sell" and o[2] < p_star),
                        key=lambda o: o[2])
    boxes, base = [], 0
    for _, _, limit, q in ranked:
        boxes.append((min(limit, p_star), max(limit, p_star), base, base + q))
        base += q
    return boxes


def main():
    # Dense price grid so the step function renders as a clean staircase;
    # real engine prices live on PRICE_SCALE ticks, but this illustrative
    # chart uses small integers.
    p_grid = np.linspace(P_MIN, P_MAX, 2801)
    d = _step_series(demand, p_grid)
    s = _step_series(supply, p_grid)

    p_star, v_star = _clearing()

    fig, ax = plt.subplots(figsize=(6.0, 4.4))

    # Surplus areas first, so the curves are drawn on top of them.
    for side, color, label in (("buy", DEMAND_COLOR, "buyer\nsurplus"),
                               ("sell", SUPPLY_COLOR, "seller\nsurplus")):
        boxes = _surplus_rectangles(p_star, side)
        for p0, p1, q0, q1 in boxes:
            ax.add_patch(patches.Rectangle(
                (p0, q0), p1 - p0, q1 - q0,
                facecolor=color, alpha=0.18, edgecolor=color,
                linewidth=0.5, zorder=1,
            ))
        # Label in the widest box of the region.
        p0, p1, q0, q1 = max(boxes, key=lambda b: (b[1] - b[0]) * (b[3] - b[2]))
        ax.text((p0 + p1) / 2, (q0 + q1) / 2, label,
                ha="center", va="center", fontsize=9, color=color)

    ax.step(p_grid, d, where="post", color=DEMAND_COLOR, linewidth=2, label="Demand $D(p)$")
    ax.step(p_grid, s, where="post", color=SUPPLY_COLOR, linewidth=2, label="Supply $S(p)$")

    # Clearing point.
    ax.plot([p_star], [v_star], marker="o", markersize=7, color=CLEAR_COLOR, zorder=5)
    ax.hlines(v_star, P_MIN, p_star, color=CLEAR_COLOR, linewidth=1, linestyle="--", zorder=2)
    ax.annotate(
        rf"$p^{{*}}={p_star}$" + "\n" + rf"$V^{{*}}={v_star}$",
        xy=(p_star, v_star), xytext=(p_star + 1.6, v_star + 6),
        fontsize=10, color=CLEAR_COLOR,
        arrowprops=dict(arrowstyle="-", color=CLEAR_COLOR, linewidth=0.8),
    )

    ax.set_xlim(P_MIN, P_MAX)
    ax.set_ylim(0, 62)
    ax.set_xlabel("Price $p$")
    ax.set_ylabel("Cumulative quantity")
    ax.set_title("FBA clearing: demand and supply schedules", fontsize=11)
    ax.legend(loc="upper center", ncol=2, frameon=False)

    fig.tight_layout()
    path = config.FIGURES_DIR / "fig_fba_schedule.png"
    fig.savefig(path, bbox_inches="tight")
    plt.close(fig)
    print(f"wrote {path}")


if __name__ == "__main__":
    main()

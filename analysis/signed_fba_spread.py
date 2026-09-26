"""Signed FBA effective spread, realized spread and price impact -- a
CDA-borrowed tick rule, replacing the unsigned (mean-absolute-deviation)
FBA values that the taker/maker-blind clearing rule otherwise forces.

Why not tick off the FBA's own price series? Effective spread's D_k
(trade direction) exists to convert a raw price deviation into a
quantity that is positive when the trade cost the taker money, regardless
of which side they were on. If D_k is derived from the same price move
the deviation itself is computed from -- e.g. "buy" whenever this batch's
clearing price rose above the previous batch's clearing price, the
formula's own reference price m_k -- the construction is circular: sign(p_k
- m_k) * |p_k - m_k| collapses back to (p_k - m_k), and D_k=buy/sell's
usual sign-flip (+ for buy, - for sell) exactly cancels the classification
that produced it. No new information is added; it is arithmetically
identical to leaving the value unsigned.

The CDA's tick avoids this because it is genuinely independent of the
FBA's own reference price: the CDA and FBA are the same underlying order
flow processed two different ways, so when the CDA's own price ticks up
or down in a matched second, that is real information about which way
flow was leaning that second -- not a restatement of the FBA's own
clearing price move. This is the standard Lee-Ready tick rule
(Lee & Ready, 1991), just sourced from the CDA's independent price
series rather than the FBA's own (circular) one.

Limitation, stated plainly: this is a proxy, not a true aggressor flag.
Tick-rule classification is well known in the market microstructure
literature to be imperfect (commonly ~80-85% accurate against true
trade-direction data); a matched-second CDA tick is one step further
removed again, since it borrows the *other* engine's classification for
*this* engine's batch rather than using any information internal to the
FBA trade itself. It is the best Python-only construction available from
the data this thesis has, not a claim of exact trade-level signing.
"""

import numpy as np
import pandas as pd

import paired_stats

HORIZONS = ["1s", "5s", "30s"]


def build_intersection(merged):
    """Matched buckets (vwap present on both sides), carrying the raw
    unsigned FBA spread columns to be signed and the CDA's own (already
    signed, unchanged) columns needed for the paired comparison."""
    cols = ["interval_start_ns", "vwap_fba", "vwap_cda",
            "effective_spread_bps_fba", "effective_spread_bps_cda"]
    for h in HORIZONS:
        cols += [f"realized_spread_bps_{h}_fba", f"realized_spread_bps_{h}_cda",
                  f"price_impact_bps_{h}_cda"]
    mask = merged["vwap_fba"].notna() & merged["vwap_cda"].notna()
    sub = merged.loc[mask, cols]
    return sub.sort_values("interval_start_ns").reset_index(drop=True)


def compute_cda_tick_signs(vwap_cda):
    """Lee-Ready zero-tick rule on the CDA's own price series: +1 if this
    value is higher than the previous one in the series, -1 if lower, and
    on a tie inherit the last nonzero tick. First observation is NaN (no
    prior tick to compare against)."""
    vals = vwap_cda.to_numpy(dtype=np.float64)
    signs = np.full(len(vals), np.nan)
    last_sign = np.nan
    for i in range(1, len(vals)):
        if vals[i] > vals[i - 1]:
            last_sign = 1.0
        elif vals[i] < vals[i - 1]:
            last_sign = -1.0
        signs[i] = last_sign
    return signs


def compute(merged):
    """Builds tick-signed FBA effective spread, realized spread (3
    horizons) and price impact (3 horizons, recomputed as signed
    effective minus signed realized rather than re-signing the old
    already-differenced value), and their paired Newey-West comparison
    against the CDA's unchanged, genuinely signed columns. Returns a dict
    keyed by the same metric names paired_stats/make_tables use
    ("effective_spread_bps", "realized_spread_bps_1s", ...), each holding
    a paired_diff() stats block, plus tick-coverage diagnostics."""
    intersection = build_intersection(merged)
    tick = compute_cda_tick_signs(intersection["vwap_cda"])
    df = intersection.assign(tick_sign=tick).dropna(subset=["tick_sign"])

    n_matched = len(intersection)
    n_signed = len(df)
    print(f"signed FBA spread (CDA tick rule): {n_signed:,}/{n_matched:,} "
          f"matched buckets have a defined CDA tick")

    stats = {}
    signed = {"interval_start_ns": df["interval_start_ns"],
              "effective_spread_bps_cda": df["effective_spread_bps_cda"]}

    signed_eff_fba = df["tick_sign"] * df["effective_spread_bps_fba"]
    signed["effective_spread_bps_fba"] = signed_eff_fba
    stats["effective_spread_bps"] = paired_stats.paired_diff(
        signed_eff_fba, df["effective_spread_bps_cda"])

    for h in HORIZONS:
        signed_real_fba = df["tick_sign"] * df[f"realized_spread_bps_{h}_fba"]
        signed[f"realized_spread_bps_{h}_fba"] = signed_real_fba
        stats[f"realized_spread_bps_{h}"] = paired_stats.paired_diff(
            signed_real_fba, df[f"realized_spread_bps_{h}_cda"])

        signed_impact_fba = signed_eff_fba - signed_real_fba
        signed[f"price_impact_bps_{h}_fba"] = signed_impact_fba
        stats[f"price_impact_bps_{h}"] = paired_stats.paired_diff(
            signed_impact_fba, df[f"price_impact_bps_{h}_cda"])

    return dict(
        stats=stats,
        n_matched=n_matched,
        n_signed=n_signed,
        signed_df=pd.DataFrame(signed),
    )


if __name__ == "__main__":
    import load
    m = load.load_merged()
    result = compute(m)
    for key, s in result["stats"].items():
        print(f"{key}: CDA={s['cda_mean']:.3f} FBA={s['fba_mean']:.3f} "
              f"diff={s['diff_mean']:.3f} p={s['p_value']} n={s['n_paired']}")

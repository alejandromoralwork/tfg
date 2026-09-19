"""Paired-difference statistics between the CDA and FBA columns of one
metric, with Newey-West (HAC) standard errors -- the method ch4's
"Statistical Treatment" subsection names.

Each 1-second interval gives one CDA observation and one FBA observation
from the same replayed flow (paired by construction). Only intervals where
*both* engines have a qualifying observation for that metric enter the
paired sample -- for several metrics (e.g. effective_spread_bps) that's a
minority of the 2,678,400 grid rows, since most 1-second buckets have no
qualifying trade/batch on one or both sides. n_paired records exactly how
many intervals that was, per metric.
"""

import numpy as np
import statsmodels.api as sm


def newey_west_lags(n, cap=60):
    """Automatic bandwidth (Newey & West 1994), capped for bounded compute."""
    if n < 2:
        return 0
    lags = int(np.floor(4 * (n / 100) ** (2 / 9)))
    return max(0, min(lags, cap))


def paired_diff(fba_col, cda_col):
    """fba_col, cda_col: two aligned pandas Series (same index/order --
    both engines' rows for one metric, positionally paired by
    interval_start_ns). Returns a dict of scalars, or all-NaN/n_paired=0 if
    no interval has both sides present.
    """
    mask = fba_col.notna() & cda_col.notna()
    n = int(mask.sum())
    if n == 0:
        return dict(cda_mean=np.nan, fba_mean=np.nan, diff_mean=np.nan,
                     hac_se=np.nan, t_stat=np.nan, p_value=np.nan,
                     n_paired=0, hac_lags=0)

    fba_vals = fba_col[mask].to_numpy(dtype=np.float64)
    cda_vals = cda_col[mask].to_numpy(dtype=np.float64)
    diff = fba_vals - cda_vals

    lags = newey_west_lags(n)
    if n > 1 and np.std(diff) > 0:
        model = sm.OLS(diff, np.ones(n))
        res = model.fit(cov_type="HAC", cov_kwds={"maxlags": lags})
        diff_mean = float(res.params[0])
        hac_se = float(res.bse[0])
        t_stat = float(res.tvalues[0])
        p_value = float(res.pvalues[0])
    else:
        # degenerate (n=1, or diff is exactly constant, e.g. both sides
        # always 0) -- HAC/OLS inference isn't meaningful, report the mean
        # and leave inferential stats as NaN rather than fabricate a p-value.
        diff_mean = float(np.mean(diff))
        hac_se = t_stat = p_value = np.nan

    return dict(
        cda_mean=float(np.mean(cda_vals)),
        fba_mean=float(np.mean(fba_vals)),
        diff_mean=diff_mean,
        hac_se=hac_se,
        t_stat=t_stat,
        p_value=p_value,
        n_paired=n,
        hac_lags=lags,
    )


def one_sided_stats(col):
    """Descriptive stats for a CDA-only or FBA-only metric (no pairing)."""
    vals = col.dropna().to_numpy(dtype=np.float64)
    if len(vals) == 0:
        return dict(mean=np.nan, std=np.nan, min=np.nan, max=np.nan,
                     median=np.nan, n=0)
    return dict(
        mean=float(np.mean(vals)), std=float(np.std(vals)),
        min=float(np.min(vals)), max=float(np.max(vals)),
        median=float(np.median(vals)), n=int(len(vals)),
    )

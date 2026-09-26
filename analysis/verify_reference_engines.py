"""Independent re-implementation of both matching engines, checked against the Rust simulator.

Purpose: evidence that the Rust CDA and FBA engines (and the replay glue around
them) do what the written rules say. Nothing here shares code with the Rust
crate: the 54-byte records are decoded with numpy from the layout in
data/SCHEMA.md, and the two engines are written again from the rules with
different data structures:

  CDA  Rust: BTreeMap<price, VecDeque<Order>>      Python: two heaps with lazy deletion
  FBA  Rust: candidate set + prefix sums + binary  Python: per-price level totals, cumulative
             search + sorted eligible indices              sums over the sorted price list and
                                                            a lazy priority walk

For each hour the Rust `market_sim simulate` is run on a directory holding only
that hour, and every one-second bucket of the two output CSVs is compared with
the Python result (trade count, executed volume, executed notional, VWAP; for the
FBA also depth at best and unexecuted residual share). Hour totals are compared
against the exact integers in the checkpoint summary (the CSV prints notional
with 7 significant digits).

Usage: python analysis/verify_reference_engines.py  <rust_output_root> <label>=<hour_dir> ...
The hour directories must have been run through `market_sim simulate <hour_dir> 1`
from the working directory that holds `<rust_output_root>` (= that directory's output/).
"""
from __future__ import annotations

import gzip
import heapq
import json
import sys
from bisect import bisect_left, insort
from pathlib import Path

import numpy as np
import pandas as pd

W = 1_000_000_000  # tau = 1 s, in ns
PRICE_SCALE = 1_000_000

# 54-byte layout, copied from data/SCHEMA.md (offsets 0..53).
DTYPE = np.dtype([
    ("ts", "<u8"), ("userId", "<u4"), ("isBuilder", "u1"), ("statusId", "u1"), ("isAsk", "u1"),
    ("limitPx", "<u4"), ("sz", "<u4"), ("oid", "<u8"), ("timestampDiff", "<u4"),
    ("triggerCondition", "<u4"), ("triggered", "u1"), ("isTrigger", "u1"), ("hasChildren", "u1"),
    ("isPositionTpsl", "u1"), ("reduceOnly", "u1"), ("orderTypeId", "u1"), ("tifId", "u1"),
    ("triggerPx", "<u4"), ("origSz", "<u4"),
])
assert DTYPE.itemsize == 54

CANCEL_STATUSES = {2, 7, 10, 11, 12, 13, 14, 16}   # statuses.csv: the eight cancel codes
MARKET_TYPES = {1, 2, 3, 6}                          # order_types.csv: market-like types


def fixed_to_scaled(enc: np.ndarray) -> np.ndarray:
    """SCHEMA.md price encoding -> integer with 6 decimals, round-half-up for 7 decimals."""
    dec = (enc >> 29).astype(np.int64)
    val = (enc & 0x1FFFFFFF).astype(np.int64)
    out = np.where(dec <= 6, val * 10 ** np.maximum(6 - dec, 0), 0)
    seven = dec == 7
    out = np.where(seven, (val + 5) // 10, out)
    return out


def load_hour(path: Path):
    raw = gzip.open(path, "rb").read()
    assert len(raw) % 54 == 0, "file size is not a multiple of 54"
    a = np.frombuffer(raw, dtype=DTYPE)
    orig = (fixed_to_scaled(a["origSz"]) + PRICE_SCALE // 2) // PRICE_SCALE
    keep = orig > 0                      # records whose size rounds to zero never become orders
    n_all = len(a)
    a = a[keep]
    orig = orig[keep]
    px = fixed_to_scaled(a["limitPx"])
    status = a["statusId"].astype(int)
    live = ((status == 1) & (a["isTrigger"] == 0)) | (status == 9)
    cancel = np.isin(status, list(CANCEL_STATUSES))
    market = np.isin(a["orderTypeId"], list(MARKET_TYPES))
    sz_raw = fixed_to_scaled(a["sz"])
    filled_zero = (status == 5) & (sz_raw == 0)
    # venue taker volume: `filled` records written at the order's own submission time (timestampDiff == 0)
    last, taker, taker_sec = {}, 0, {}
    ts0 = int(a["ts"][0]) if len(a) else 0
    for oid_, st_, sz_, or_, td_, ts_ in zip(a["oid"].tolist(), status.tolist(), sz_raw.tolist(), orig.tolist(), a["timestampDiff"].tolist(), a["ts"].tolist()):
        if st_ == 5:
            prev = last.get(oid_, or_ * PRICE_SCALE)
            if td_ == 0 and prev > sz_:
                taker += prev - sz_
                b_ = (ts_ - ts0) // W
                taker_sec[b_] = taker_sec.get(b_, 0) + (prev - sz_) / PRICE_SCALE
        last[oid_] = sz_
    return {
        "venue_taker_volume": taker / PRICE_SCALE, "venue_taker_by_sec": taker_sec,
        "filled_zero": filled_zero.tolist(),
        "n_all": n_all, "n_kept": int(keep.sum()),
        "ts": a["ts"].astype(np.int64).tolist(), "oid": a["oid"].astype(np.int64).tolist(),
        "is_ask": (a["isAsk"] != 0).tolist(), "px": px.tolist(), "qty": orig.tolist(),
        "live": live.tolist(), "cancel": cancel.tolist(), "market": market.tolist(),
        "user": a["userId"].tolist(), "tif": a["tifId"].tolist(),
    }


# ----------------------------------------------------------------------------- CDA
def reference_cda(d, anchor, venue_fills_remove_orders=False, trace=None):
    """Continuous double auction, price-time priority, trades at the resting order's price.

    Also returns the per-bucket accumulators needed to recompute three CDA metrics from scratch:
    quoted spread, depth at best and effective spread.
    """
    asks, bids = [], []                 # heaps of (price, ts, oid, seq) / (-price, ts, oid, seq)
    rem, alive = {}, {}
    seq_of_oid = {}
    trades = []                         # (bucket, qty, price)
    dup_live_oids = 0
    seq = 0
    snaps = {}                          # bucket -> [sum quoted spread, n spread, sum depth, n snapshots]
    eff = {}                            # bucket -> [sum deviation*qty, sum qty]

    def top(h, sign):
        while h and not alive[h[0][3]]:
            heapq.heappop(h)
        if not h:
            return None, 0
        return sign * h[0][0], rem[h[0][3]]

    fz = d["filled_zero"] if venue_fills_remove_orders else [False] * len(d["ts"])
    user_of = {}                        # seq -> user id of the resting order
    self_vol = 0                        # volume where buyer and seller are the same user
    alo_vol = 0                         # volume where the TAKER is a post-only (ALO) order: impossible on the venue
    for ts, oid, is_ask, px, qty, live, cancel, market, fzero, user, tif in zip(
            d["ts"], d["oid"], d["is_ask"], d["px"], d["qty"], d["live"], d["cancel"], d["market"], fz, d["user"], d["tif"]):
        cancel = cancel or fzero
        bb, _ = top(bids, -1)
        ba, _ = top(asks, 1)
        mid_before = (bb + ba) // 2 if (bb is not None and ba is not None) else (bb if bb is not None else ba)
        n_tr = len(trades)
        if cancel:
            s = seq_of_oid.pop(oid, None)
            if s is not None and alive.get(s):
                alive[s] = False
        elif live:
            left = qty
            if not is_ask:                  # buy taker
                while left > 0:
                    while asks and not alive[asks[0][3]]:
                        heapq.heappop(asks)
                    if not asks:
                        break
                    ap, ats, aoid, s = asks[0]
                    if not market and px < ap:
                        break
                    f = min(left, rem[s])
                    left -= f
                    rem[s] -= f
                    trades.append(((ts - anchor) // W, f, ap))
                    if trace:
                        trace(ts, "BUY taker", oid, user, aoid, user_of[s], ap, f)
                    if user_of[s] == user:
                        self_vol += f
                    if tif == 0 and not market:
                        alo_vol += f
                    if rem[s] == 0:
                        alive[s] = False
                        heapq.heappop(asks)
                        if seq_of_oid.get(aoid) == s:
                            del seq_of_oid[aoid]
                if left > 0 and not market:
                    seq += 1
                    rem[seq], alive[seq] = left, True
                    user_of[seq] = user
                    if oid in seq_of_oid and alive.get(seq_of_oid[oid]):
                        dup_live_oids += 1
                    seq_of_oid[oid] = seq
                    heapq.heappush(bids, (-px, ts, oid, seq))
            else:                           # sell taker
                while left > 0:
                    while bids and not alive[bids[0][3]]:
                        heapq.heappop(bids)
                    if not bids:
                        break
                    nbp, bts, boid, s = bids[0]
                    bp = -nbp
                    if not market and px > bp:
                        break
                    f = min(left, rem[s])
                    left -= f
                    rem[s] -= f
                    trades.append(((ts - anchor) // W, f, bp))
                    if trace:
                        trace(ts, "SELL taker", oid, user, boid, user_of[s], bp, f)
                    if user_of[s] == user:
                        self_vol += f
                    if tif == 0 and not market:
                        alo_vol += f
                    if rem[s] == 0:
                        alive[s] = False
                        heapq.heappop(bids)
                        if seq_of_oid.get(boid) == s:
                            del seq_of_oid[boid]
                if left > 0 and not market:
                    seq += 1
                    rem[seq], alive[seq] = left, True
                    user_of[seq] = user
                    if oid in seq_of_oid and alive.get(seq_of_oid[oid]):
                        dup_live_oids += 1
                    seq_of_oid[oid] = seq
                    heapq.heappush(asks, (px, ts, oid, seq))
            # effective spread of this order's trades: 2 * side * (price - mid_before) / mid_before
            if mid_before:
                for b, q, p in trades[n_tr:]:
                    dev = 2.0 * (p - mid_before) / mid_before * 1e4 * (-1 if is_ask else 1)
                    e = eff.setdefault(b, [0.0, 0])
                    e[0] += dev * q
                    e[1] += q
        # book snapshot after every record: touch prices and the touch order's remaining size
        bb, bq = top(bids, -1)
        ba, aq = top(asks, 1)
        row = snaps.setdefault((ts - anchor) // W, [0.0, 0, 0.0, 0])
        row[2] += (bq + aq) / 2.0
        row[3] += 1
        if bb is not None and ba is not None and bb + ba > 0:
            row[0] += (ba - bb) / ((bb + ba) / 2.0) * 1e4
            row[1] += 1
    return trades, dup_live_oids, snaps, eff, self_vol, alo_vol


# ----------------------------------------------------------------------------- FBA
class Batch:
    """Pending orders grouped by price level; D(p)/S(p) by cumulative sums over the sorted levels."""

    def __init__(self):
        self.buy_lvl, self.sell_lvl = {}, {}           # price -> list of [ts, oid, rem]
        self.buy_tot, self.sell_tot = {}, {}           # price -> live quantity
        self.buy_mkt, self.sell_mkt = [], []           # lists of [ts, oid, rem]
        self.prices = []                                # sorted prices having >=1 live limit order
        self.cnt = {}                                   # price -> live limit orders (both sides)
        self.by_oid = {}                                # oid -> list of (order, side_is_ask, price|None)
        self.ts_heap = []                               # (-ts, id(order)) lazy max-ts
        self.n_live = 0
        self.last_clearing = None

    def add(self, ts, oid, is_ask, px, qty, market):
        o = [ts, oid, qty]
        self.n_live += 1
        heapq.heappush(self.ts_heap, (-ts, id(o), o))
        if market:
            (self.sell_mkt if is_ask else self.buy_mkt).append(o)
            key = None
        else:
            lvl, tot = (self.sell_lvl, self.sell_tot) if is_ask else (self.buy_lvl, self.buy_tot)
            lvl.setdefault(px, []).append(o)
            tot[px] = tot.get(px, 0) + qty
            if self.cnt.get(px, 0) == 0:
                insort(self.prices, px)
            self.cnt[px] = self.cnt.get(px, 0) + 1
            key = px
        self.by_oid.setdefault(oid, []).append((o, is_ask, key))

    def _drop_limit(self, o, is_ask, px):
        tot = self.sell_tot if is_ask else self.buy_tot
        tot[px] -= o[2]
        self.cnt[px] -= 1
        if self.cnt[px] == 0:
            del self.cnt[px]
            self.prices.pop(bisect_left(self.prices, px))

    def cancel(self, oid):
        for o, is_ask, px in self.by_oid.pop(oid, []):
            if o[2] > 0:
                if px is not None:
                    self._drop_limit(o, is_ask, px)
                self.n_live -= 1
                o[2] = -1                                # dead marker

    def _finish(self, q):
        """An order was filled completely: it is no longer live (and leaves its price level)."""
        self.n_live -= 1
        if q is not None:
            self.cnt[q] -= 1
            if self.cnt[q] == 0:
                del self.cnt[q]
                self.prices.pop(bisect_left(self.prices, q))

    def max_ts(self):
        while self.ts_heap and self.ts_heap[0][2][2] <= 0:
            heapq.heappop(self.ts_heap)
        return -self.ts_heap[0][0] if self.ts_heap else 0

    def _walk(self, side_is_ask, p):
        """Eligible orders in priority order: market first, then better price, then (ts, oid)."""
        mk = self.sell_mkt if side_is_ask else self.buy_mkt
        for o in sorted((x for x in mk if x[2] > 0), key=lambda x: (x[0], x[1])):
            yield o, None
        if side_is_ask:
            levels = [q for q in self.prices if q <= p and q in self.sell_lvl]
        else:
            levels = [q for q in reversed(self.prices) if q >= p and q in self.buy_lvl]
        for q in levels:
            for o in sorted((x for x in (self.sell_lvl if side_is_ask else self.buy_lvl)[q] if x[2] > 0), key=lambda x: (x[0], x[1])):
                yield o, q

    def clear(self):
        """Returns None if nothing could be priced, else (p, D, S, fills[(qty)], batch_ts)."""
        if not self.prices:
            return None
        P = np.array(self.prices, dtype=np.int64)
        bt = np.array([self.buy_tot.get(q, 0) for q in self.prices], dtype=np.int64)
        st = np.array([self.sell_tot.get(q, 0) for q in self.prices], dtype=np.int64)
        mb = sum(o[2] for o in self.buy_mkt if o[2] > 0)
        ms = sum(o[2] for o in self.sell_mkt if o[2] > 0)
        D = mb + (bt[::-1].cumsum()[::-1])            # buys willing to pay >= p
        S = ms + st.cumsum()                           # sells willing to accept <= p
        vol = np.minimum(D, S)
        imb = np.abs(D - S)
        dist = np.abs(P - self.last_clearing) if self.last_clearing is not None else np.zeros_like(P)
        j = int(np.lexsort((P, dist, imb, -vol))[0])   # max volume, min imbalance, nearest last price, lowest price
        p, d, s, traded = int(P[j]), int(D[j]), int(S[j]), int(vol[j])
        batch_ts = self.max_ts()
        fills = []
        bi, si = self._walk(False, p), self._walk(True, p)
        b = next(bi, None)
        a = next(si, None)
        while b is not None and a is not None:
            (ob, qb), (oa, qa) = b, a
            f = min(ob[2], oa[2])
            if f == 0:                                   # exhausted order: move on
                if ob[2] == 0:
                    b = next(bi, None)
                if oa[2] == 0:
                    a = next(si, None)
                continue
            ob[2] -= f
            oa[2] -= f
            if qb is not None:
                self.buy_tot[qb] -= f
            if qa is not None:
                self.sell_tot[qa] -= f
            fills.append(f)
            if ob[2] == 0:
                self._finish(qb)
                b = next(bi, None)
            if oa[2] == 0:
                self._finish(qa)
                a = next(si, None)
        if traded > 0:
            self.last_clearing = p
        # drop exhausted orders from the per-oid index lazily (rem == 0 is treated as gone)
        return p, d, s, fills, batch_ts


def reference_fba(d, anchor):
    bt = Batch()
    trades = []                     # (bucket, qty, price)
    batches = {}                    # bucket -> [num_resid, den_resid, sum_depth, n_batches, n_unpriced]
    ts_list = d["ts"]
    state = {"open": None, "next": None}

    def close_batch(close_ts):
        if bt.n_live == 0:
            return
        res = bt.clear()
        bucket = (close_ts - anchor) // W
        row = batches.setdefault(bucket, [0, 0, 0.0, 0, 0])
        row[3] += 1
        if res is None:
            row[4] += 1
            return
        p, dd, ss, fills, batch_ts = res
        for f in fills:
            trades.append(((batch_ts - anchor) // W, f, p))
        row[0] += abs(dd - ss)
        row[1] += max(dd, ss)
        row[2] += (dd + ss) / 2

    last = None
    for ts, oid, is_ask, px, qty, live, cancel, market in zip(
            ts_list, d["oid"], d["is_ask"], d["px"], d["qty"], d["live"], d["cancel"], d["market"]):
        last = ts
        if state["next"] is None:
            state["next"], state["open"] = ts + W, ts
        while ts >= state["next"]:
            close_batch(state["next"])
            state["open"], state["next"] = state["next"], state["next"] + W
        if live:
            bt.add(ts, oid, is_ask, px, qty, market)
        elif cancel:
            bt.cancel(oid)
    if bt.n_live > 0:
        close_batch(max(last, state["open"]))
    return trades, batches


# ----------------------------------------------------------------------------- comparison
def aggregate(trades, nrows):
    cnt = np.zeros(nrows, dtype=np.int64)
    vol = np.zeros(nrows, dtype=np.int64)
    notional = [0] * nrows
    for b, q, p in trades:
        if 0 <= b < nrows:
            cnt[b] += 1
            vol[b] += q
            notional[b] += q * p
    return cnt, vol, notional


def summary_totals(ckpt_path: Path, engine: str):
    for line in ckpt_path.read_text().splitlines():
        if line.startswith(f"{engine}_summary "):
            parts = line.split(" ", 1)[1].split(";")
            return int(parts[0]), int(parts[1]), int(parts[2]), int(parts[3])
    raise KeyError(engine)


def compare_cda_metrics(df, snaps, eff):
    n = len(df)
    ok = {"quoted_spread_bps": 0, "depth_at_best": 0, "effective_spread_bps": 0}
    for i in range(n):
        row = snaps.get(i)
        q_ref = row[0] / row[1] if row and row[1] else float("nan")
        d_ref = row[2] / row[3] if row and row[3] else float("nan")
        e_row = eff.get(i)
        e_ref = e_row[0] / e_row[1] if e_row and e_row[1] else float("nan")
        for col, ref in (("quoted_spread_bps", q_ref), ("depth_at_best", d_ref), ("effective_spread_bps", e_ref)):
            got = df[col].iloc[i]
            if (np.isnan(ref) and np.isnan(got)) or (not np.isnan(ref) and not np.isnan(got) and abs(ref - got) <= 2e-6 * max(1.0, abs(ref))):
                ok[col] += 1
    return {k + "_equal": v for k, v in ok.items()}


def compare_engine(name, csv_path, ckpt_path, anchor, trades, batches=None):
    df = pd.read_csv(csv_path)
    n = len(df)
    assert int(df["interval_start_ns"].iloc[0]) == anchor, "grid origin differs"
    cnt, vol, notional = aggregate(trades, n)
    out = {"buckets": n}
    ok_cnt = int((df["trade_count"].to_numpy() == cnt).sum())
    ok_vol = int((df["executed_volume"].to_numpy() == vol).sum())
    notional_f = np.array([float(x) for x in notional])
    rel = np.abs(df["executed_notional"].to_numpy() - notional_f) / np.maximum(notional_f, 1)
    ok_not = int((rel < 5e-7).sum())
    vw_ref = np.where(vol > 0, notional_f / np.maximum(vol, 1), np.nan)
    vw_csv = df["vwap"].to_numpy(dtype=float)
    both_nan = np.isnan(vw_ref) & np.isnan(vw_csv)
    ok_vwap = int((both_nan | (np.abs(vw_ref - vw_csv) / np.maximum(vw_ref, 1) < 5e-7)).sum())
    out.update(trade_count_equal=ok_cnt, volume_equal=ok_vol, notional_equal=ok_not, vwap_equal=ok_vwap)
    first_bad = [int(i) for i in np.flatnonzero((df["trade_count"].to_numpy() != cnt) | (df["executed_volume"].to_numpy() != vol))[:3]]
    out["first_mismatching_buckets"] = first_bad
    # exact integer totals against the checkpoint summary
    rows, ntr, tvol, tnot = summary_totals(ckpt_path, name.lower())
    out["totals"] = {
        "trades": [len(trades), ntr], "volume": [int(vol.sum()), tvol],
        "notional": [int(sum(q * p for _, q, p in trades)), tnot],
    }
    out["totals_equal"] = (len(trades) == ntr and int(vol.sum()) == tvol and sum(q * p for _, q, p in trades) == tnot)
    if batches is not None:
        resid_ok = depth_ok = resid_n = 0
        for i in range(n):
            row = batches.get(i)
            if row is None or row[3] != 1 or row[4] != 0:
                continue                                  # only buckets with exactly one priced batch
            resid_n += 1
            share = row[0] / row[1] if row[1] else float("nan")
            csv_r = df["unexecuted_residual_share"].iloc[i]
            csv_d = df["depth_at_best"].iloc[i]
            if (np.isnan(share) and np.isnan(csv_r)) or abs(share - csv_r) < 2e-6:
                resid_ok += 1
            if abs(row[2] - csv_d) < 2e-6 * max(1.0, abs(row[2])):
                depth_ok += 1
        out.update(single_batch_buckets=resid_n, residual_share_equal=resid_ok, depth_at_best_equal=depth_ok)
    return out


def main():
    root = Path(sys.argv[1])
    results = {}
    for spec in sys.argv[2:]:
        label, hour_dir = spec.split("=", 1)
        hour_dir = Path(hour_dir)
        gz = next(hour_dir.rglob("*.data.gz"))
        d = load_hour(gz)
        anchor = d["ts"][0]
        out_dir = root / "output" / hour_dir.name
        print(f"[{label}] {gz.name}: {d['n_all']:,} records, {d['n_kept']:,} kept, anchor {anchor}", flush=True)
        tr_c, dup, snaps, eff, self_vol, alo_vol = reference_cda(d, anchor)
        tr_f, batches = reference_fba(d, anchor)
        rc = compare_engine("cda", out_dir / "cda_timeseries.csv", out_dir / "checkpoint.txt", anchor, tr_c)
        rf = compare_engine("fba", out_dir / "fba_timeseries.csv", out_dir / "checkpoint.txt", anchor, tr_f, batches)
        rc["duplicate_live_oids_in_reference"] = dup
        tr_v = reference_cda(d, anchor, venue_fills_remove_orders=True)[0]
        rc["volume_simulated"] = float(sum(q for _, q, _ in tr_c))
        rc["volume_simulated_if_venue_completions_remove_orders"] = float(sum(q for _, q, _ in tr_v))
        rc["volume_venue_taker_fills"] = float(d["venue_taker_volume"])
        rc["volume_self_trades"] = float(self_vol)
        sim_sec = {}
        for b_, q_, _p in tr_c:
            sim_sec[b_] = sim_sec.get(b_, 0) + q_
        ven_sec = d["venue_taker_by_sec"]
        gaps_ = sorted(((sim_sec.get(b_, 0) - ven_sec.get(b_, 0)) for b_ in set(sim_sec) | set(ven_sec)), reverse=True)
        exc = sum(g_ for g_ in gaps_ if g_ > 0)
        rc["excess_total"] = float(exc)
        rc["excess_in_top15_seconds"] = float(sum(gaps_[:15]))
        wb = max(set(sim_sec) | set(ven_sec), key=lambda b_: sim_sec.get(b_, 0) - ven_sec.get(b_, 0))
        rc["largest_excess_second"] = {"sim": float(sim_sec.get(wb, 0)), "venue": float(ven_sec.get(wb, 0))}
        rc["volume_taker_is_post_only"] = float(alo_vol)
        rc.update(compare_cda_metrics(pd.read_csv(out_dir / "cda_timeseries.csv"), snaps, eff))
        results[label] = {"records": d["n_all"], "kept": d["n_kept"], "cda": rc, "fba": rf}
        print(json.dumps(results[label], indent=1, default=int), flush=True)
    out = Path(__file__).parent / "output" / "verification_reference_engines.json"
    if out.exists():                                     # keep hours from earlier runs
        old = json.loads(out.read_text())
        old.update(results)
        results = old
    out.write_text(json.dumps(results, indent=1, default=int))


if __name__ == "__main__":
    main()

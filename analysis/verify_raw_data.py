"""Checks on the raw input data, independent of the Rust decoder.

Reads every hourly file of the month with numpy (layout from data/SCHEMA.md) and checks,
per file and per order:

  file level     size is a multiple of 54; every boolean byte is 0 or 1; status, order-type and
                 time-in-force codes are inside the lookup tables; timestamp order (backward
                 steps within files and across file boundaries); timestamps versus the hour in
                 the file name
  record level   size fields: current size <= original size; records whose size rounds to zero
                 (must equal the simulator's skipped count); new-live / cancellation / other
                 classification (must equal the simulator's scan counts)
  order level    the whole lifecycle of every order (oid), with state carried across days:
                 first record is an open/triggered event or the order is explained by an earlier
                 day; submission time (ts - timestampDiff) is constant per order; size never
                 increases; events after a terminal event
  venue fills    every `filled` record is a fill of (previous size - size) on one side of a trade;
                 the fills of buyers and of sellers must add up to the same volume; per-second
                 venue volume is saved for the replay-fidelity comparison

Usage:  python analysis/verify_raw_data.py [--days 20251201 20251202 ...] [--out name]
Output: analysis/output/verification_raw[_name].json  and  verification_venue_volume[_name].npz
"""
from __future__ import annotations

import argparse
import gzip
import json
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[1]
RAW = REPO / "src" / "data" / "order_statuses" / "sol"
CKPT = REPO / "results" / "sol" / "checkpoint.txt"
OUT = Path(__file__).parent / "output"
PRICE_SCALE = 1_000_000

DTYPE = np.dtype([
    ("ts", "<u8"), ("userId", "<u4"), ("isBuilder", "u1"), ("statusId", "u1"), ("isAsk", "u1"),
    ("limitPx", "<u4"), ("sz", "<u4"), ("oid", "<u8"), ("timestampDiff", "<u4"),
    ("triggerCondition", "<u4"), ("triggered", "u1"), ("isTrigger", "u1"), ("hasChildren", "u1"),
    ("isPositionTpsl", "u1"), ("reduceOnly", "u1"), ("orderTypeId", "u1"), ("tifId", "u1"),
    ("triggerPx", "<u4"), ("origSz", "<u4"),
])
assert DTYPE.itemsize == 54
BOOL_COLS = ["isBuilder", "isAsk", "triggered", "isTrigger", "hasChildren", "isPositionTpsl", "reduceOnly"]
CANCEL = np.array([2, 7, 10, 11, 12, 13, 14, 16])
REJECTED = np.array([0, 3, 4, 6, 8, 15, 17])       # statuses.csv: the *Rejected codes (order never opened)
ROWS = 31 * 86_400


def scaled(enc: np.ndarray) -> np.ndarray:
    """SCHEMA.md fixed point -> integer count of 1e-6 units (7-decimal values round half up)."""
    dec = (enc >> 29).astype(np.int64)
    val = (enc & 0x1FFFFFFF).astype(np.int64)
    out = np.where(dec <= 6, val * 10 ** np.maximum(6 - dec, 0), 0)
    return np.where(dec == 7, (val + 5) // 10, out)


def anchor_ns() -> int:
    for line in CKPT.read_text().splitlines():
        if line.startswith("anchor "):
            return int(line.split()[1])
    raise RuntimeError("no anchor in checkpoint")


class Totals(dict):
    def add(self, key, value):
        self[key] = self.get(key, 0) + int(value)


def read_file(path: Path):
    raw = gzip.open(path, "rb").read()
    size = len(raw)
    a = np.frombuffer(raw[: size - size % 54], dtype=DTYPE)
    return size, a


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--days", nargs="*")
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    days = args.days or sorted(p.name for p in RAW.iterdir())
    anchor = anchor_ns()
    T = Totals()
    venue_buy = np.zeros(ROWS, dtype=np.int64)     # scaled units of size, buyer side of every fill
    venue_sell = np.zeros(ROWS, dtype=np.int64)
    venue_events = np.zeros(ROWS, dtype=np.int32)
    taker_buy = np.zeros(ROWS, dtype=np.int64)         # `filled` records written at the order's own submission time
    taker_sell = np.zeros(ROWS, dtype=np.int64)
    venue_buy_other = np.zeros(ROWS, dtype=np.int64)   # size reductions seen on non-`filled` records
    venue_sell_other = np.zeros(ROWS, dtype=np.int64)
    prev_file_last_ts = None
    carry_oid = np.zeros(0, dtype=np.int64)         # orders still live at the end of the previous day
    carry_sz = np.zeros(0, dtype=np.int64)
    status_hist = np.zeros(256, dtype=np.int64)
    max_backward = 0
    max_out_of_hour = 0
    t0 = time.time()

    for day in days:
        files = sorted((RAW / day).glob("sol_*.data.gz"))
        cols = {k: [] for k in ("ts", "oid", "st", "ask", "sz", "orig", "td", "trig")}
        for f in files:
            size, a = read_file(f)
            T.add("files", 1)
            T.add("bytes_decompressed", size)
            T.add("files_size_not_multiple_of_54", size % 54 != 0)
            T.add("records", len(a))
            for b in BOOL_COLS:
                T.add("bool_bytes_not_0_or_1", (a[b] > 1).sum())
            T.add("status_above_17", (a["statusId"] > 17).sum())
            T.add("order_type_above_6", (a["orderTypeId"] > 6).sum())
            T.add("tif_above_5", (a["tifId"] > 5).sum())
            status_hist += np.bincount(a["statusId"], minlength=256)
            ts = a["ts"].astype(np.int64)
            d = np.diff(ts)
            T.add("backward_steps_within_files", (d < 0).sum())
            for j in np.flatnonzero(d < 0):
                T.setdefault("backward_step_list", []).append([f"{day}/{f.name}", int(-d[j])])
            big = np.flatnonzero(d > 60_000_000_000)              # silences longer than one minute
            T.add("gaps_over_60s_within_files", len(big))
            for j in big:
                T.setdefault("long_gap_list", []).append([f"{day}/{f.name}", int(ts[j]), int(d[j])])
            if len(d):
                T["max_gap_within_files_ns"] = max(T.get("max_gap_within_files_ns", 0), int(d.max()))
            if len(d):
                max_backward = max(max_backward, int(-d.min()) if d.min() < 0 else 0)
            if prev_file_last_ts is not None and len(ts):
                T.add("backward_steps_across_files", ts[0] < prev_file_last_ts)
                gap = int(ts[0]) - prev_file_last_ts
                if gap < 0:
                    T.setdefault("backward_step_list", []).append([f"{day}/{f.name} (file start)", -gap])
                if gap > 60_000_000_000:
                    T.add("gaps_over_60s_across_files", 1)
                    T.setdefault("long_gap_list", []).append([f"{day}/{f.name} (file start)", prev_file_last_ts, gap])
                T["max_gap_across_files_ns"] = max(T.get("max_gap_across_files_ns", 0), gap)
            if len(ts):
                prev_file_last_ts = int(ts[-1])
            hour = int(f.stem.split("_")[1].split(".")[0])
            h0 = int(np.datetime64(f"{day[:4]}-{day[4:6]}-{day[6:]}T{hour:02d}:00:00", "ns").astype(np.int64))
            out = np.maximum(h0 - ts, ts - (h0 + 3_600_000_000_000))
            T.add("records_outside_file_hour", (out > 0).sum())
            max_out_of_hour = max(max_out_of_hour, int(out.max()) if len(out) else 0)
            sz = scaled(a["sz"])
            orig = scaled(a["origSz"])
            T.add("records_size_above_original", (sz > orig).sum())
            rounded = (orig + PRICE_SCALE // 2) // PRICE_SCALE
            T.add("records_size_rounds_to_zero", (rounded == 0).sum())
            st = a["statusId"]
            keep = rounded > 0
            live = ((st == 1) & (a["isTrigger"] == 0)) | (st == 9)
            T.add("new_live_orders", (live & keep).sum())
            T.add("cancellations", (np.isin(st, CANCEL) & keep).sum())
            T.add("other_records", (~live & ~np.isin(st, CANCEL) & keep).sum())
            cols["ts"].append(ts)
            cols["oid"].append(a["oid"].astype(np.int64))
            cols["st"].append(st.copy())
            cols["ask"].append(a["isAsk"].copy())
            cols["sz"].append(sz)
            cols["orig"].append(orig)
            cols["td"].append(a["timestampDiff"].astype(np.int64))
            cols["trig"].append(a["isTrigger"].copy())
            del a
        c = {k: np.concatenate(v) for k, v in cols.items()}
        del cols

        # ---------------- order lifecycle, oid-sorted, original order kept within an oid
        order = np.argsort(c["oid"], kind="stable")
        oid, ts, st, ask, sz, orig, td = (c[k][order] for k in ("oid", "ts", "st", "ask", "sz", "orig", "td"))
        del c, order
        n = len(oid)
        first = np.ones(n, dtype=bool)
        first[1:] = oid[1:] != oid[:-1]
        last = np.ones(n, dtype=bool)
        last[:-1] = first[1:]
        gid = np.cumsum(first) - 1
        T.add("orders_seen", first.sum())

        sub = ts - td * 1_000_000                       # submission time implied by the record
        gmin = np.minimum.reduceat(sub, np.flatnonzero(first))
        gmax = np.maximum.reduceat(sub, np.flatnonzero(first))
        spread = gmax - gmin
        T.add("orders_submission_time_spread_over_2ms", (spread > 2_000_000).sum())
        T.add("orders_submission_time_spread_over_1s", (spread > 1_000_000_000).sum())
        has9 = np.maximum.reduceat((st == 9).astype(np.int8), np.flatnonzero(first)) > 0
        T.add("orders_spread_over_1s_with_triggered_event", ((spread > 1_000_000_000) & has9).sum())
        T.add("orders_spread_over_1s_without_triggered_event", ((spread > 1_000_000_000) & ~has9).sum())
        T["max_submission_time_spread_ns"] = max(T.get("max_submission_time_spread_ns", 0), int(spread.max()))
        open_rec = st == 1
        T.add("open_records_with_nonzero_timestampDiff", (open_rec & (td != 0)).sum())

        prev_sz = np.empty(n, dtype=np.int64)
        prev_sz[1:] = sz[:-1]
        f_idx = np.flatnonzero(first)
        # first record of each order: orders opened earlier are looked up in the carry
        fst = st[f_idx]
        start_ok = (fst == 1) | (fst == 9) | np.isin(fst, REJECTED)
        T.add("orders_first_record_open_or_triggered", ((fst == 1) | (fst == 9)).sum())
        T.add("orders_first_record_rejection", np.isin(fst, REJECTED).sum())
        orph = f_idx[~start_ok]
        T.add("orders_first_record_not_open", len(orph))
        pos = np.searchsorted(carry_oid, oid[orph])
        pos = np.minimum(pos, max(len(carry_oid) - 1, 0))
        found = (carry_oid[pos] == oid[orph]) if len(carry_oid) else np.zeros(len(orph), dtype=bool)
        T.add("first_record_not_open_explained_by_carry", found.sum())
        unexplained = orph[~found]
        pre_month = (sub[unexplained] < anchor)
        T.add("first_record_not_open_unexplained", len(unexplained))
        T.add("unexplained_but_submitted_before_month_start", pre_month.sum())
        prev_sz[orph[found]] = carry_sz[pos[found]]
        # remaining first records: previous size is the original size (or unknown for unexplained)
        prev_sz[f_idx[start_ok]] = orig[f_idx[start_ok]]
        prev_sz[unexplained] = orig[unexplained]
        # size must never increase within an order (open -> partial fills -> ...)
        cont = ~first
        T.add("size_increases_within_order", (cont & (sz > prev_sz)).sum())
        # nothing should follow a terminal event of the same order
        terminal = np.isin(st, CANCEL) | ((st == 5) & (sz == 0))
        after_terminal = np.zeros(n, dtype=bool)
        after_terminal[1:] = terminal[:-1] & cont[1:]
        T.add("events_after_terminal_event", after_terminal.sum())
        T.add("events_after_terminal_that_are_cancels", (after_terminal & np.isin(st, CANCEL)).sum())
        T.add("open_record_after_earlier_record_of_same_order", ((st == 1) & ~first).sum())

        # size reductions that are NOT `filled` events (a partial fill can leave the order open)
        red = (st != 5) & (sz < prev_sz)
        T.add("size_reductions_without_fill_event", red.sum())
        T.add("size_reduction_without_fill_event_volume_1e6", (prev_sz - sz)[red].sum())
        for code, cnt in zip(*np.unique(st[red], return_counts=True)):
            T.add(f"size_reductions_status_{int(code)}", cnt)

        for code in np.unique(st[red]):
            for side_flag, nm in ((0, "buy"), (1, "sell")):
                mm = red & (st == code) & (ask == side_flag)
                T.add(f"reduction_volume_status_{int(code)}_{nm}_1e6", (prev_sz - sz)[mm].sum())
        # the same reductions, by second and side (delayed: seen when the order is cancelled)
        sec_o = (ts[red] - anchor) // 1_000_000_000
        in_o = (sec_o >= 0) & (sec_o < ROWS)
        red_amt, red_ask = (prev_sz - sz)[red][in_o], ask[red][in_o]
        venue_buy_other += np.bincount(sec_o[in_o][red_ask == 0], weights=red_amt[red_ask == 0], minlength=ROWS).astype(np.int64)[:ROWS]
        venue_sell_other += np.bincount(sec_o[in_o][red_ask == 1], weights=red_amt[red_ask == 1], minlength=ROWS).astype(np.int64)[:ROWS]

        # ---------------- venue fills
        fill = np.where(st == 5, prev_sz - sz, 0)
        T.add("fill_records", (st == 5).sum())
        T.add("fill_records_with_negative_size_change", ((st == 5) & (fill < 0)).sum())
        T.add("fill_records_with_zero_size_change", ((st == 5) & (fill == 0)).sum())
        fm = (st == 5) & (fill > 0)
        sec = (ts[fm] - anchor) // 1_000_000_000
        inside = (sec >= 0) & (sec < ROWS)
        T.add("fill_records_outside_month_grid", (~inside).sum())
        s_ = sec[inside]
        f_ = fill[fm][inside]
        a_ = ask[fm][inside]
        tk = (td[fm] == 0)[inside]                       # fill at the moment of submission = the taker's own fill
        rounded_f = ((f_ + PRICE_SCALE // 2) // PRICE_SCALE) * PRICE_SCALE   # the simulator's whole-unit rounding
        T.add("venue_taker_rounded_total_1e6", rounded_f[tk].sum())
        T.add("venue_taker_total_1e6", f_[tk].sum())
        T.add("fill_records_taker_side", tk.sum())
        T.add("fill_records_maker_side", (~tk).sum())
        for arr, msk in ((venue_buy, (a_ == 0)), (venue_sell, (a_ == 1))):
            arr += np.bincount(s_[msk], weights=f_[msk], minlength=ROWS).astype(np.int64)[:ROWS]
        for arr, msk in ((taker_buy, (a_ == 0) & tk), (taker_sell, (a_ == 1) & tk)):
            arr += np.bincount(s_[msk], weights=f_[msk], minlength=ROWS).astype(np.int64)[:ROWS]
        venue_events += np.bincount(s_, minlength=ROWS).astype(np.int32)[:ROWS]

        # ---------------- carry: orders still live at the end of the day
        l_idx = np.flatnonzero(last)
        alive = ~terminal[l_idx] & np.isin(st[l_idx], [1, 5, 9]) & (sz[l_idx] > 0)
        # pending conditional orders (open with isTrigger) stay in the carry too
        new_oid, new_sz = oid[l_idx][alive], sz[l_idx][alive]
        if len(carry_oid):
            touched = np.searchsorted(oid[l_idx], carry_oid)
            touched = np.minimum(touched, len(l_idx) - 1)
            keep_old = oid[l_idx][touched] != carry_oid
            carry_oid, carry_sz = np.concatenate([carry_oid[keep_old], new_oid]), np.concatenate([carry_sz[keep_old], new_sz])
        else:
            carry_oid, carry_sz = new_oid, new_sz
        srt = np.argsort(carry_oid)
        carry_oid, carry_sz = carry_oid[srt], carry_sz[srt]
        T.add("days", 1)
        print(f"[{day}] {len(oid):,} records, {first.sum():,} orders, carry {len(carry_oid):,}, "
              f"{time.time() - t0:.0f}s", flush=True)
        del oid, ts, st, ask, sz, orig, td

    T["max_backward_step_ns"] = max_backward
    T["max_ns_outside_file_hour"] = max_out_of_hour
    T["status_histogram"] = {int(i): int(v) for i, v in enumerate(status_hist) if v}
    T["venue_buy_total_1e6"] = int(venue_buy.sum())
    T["venue_sell_total_1e6"] = int(venue_sell.sum())
    T["venue_fill_events"] = int(venue_events.sum())
    T["venue_taker_buy_total_1e6"] = int(taker_buy.sum())
    T["venue_taker_sell_total_1e6"] = int(taker_sell.sum())
    T["venue_other_buy_total_1e6"] = int(venue_buy_other.sum())
    T["venue_other_sell_total_1e6"] = int(venue_sell_other.sum())
    name = f"_{args.out}" if args.out else ""
    OUT.mkdir(exist_ok=True)
    (OUT / f"verification_raw{name}.json").write_text(json.dumps(T, indent=1))
    np.savez_compressed(OUT / f"verification_venue_volume{name}.npz", buy=venue_buy, sell=venue_sell, events=venue_events,
                        buy_other=venue_buy_other, sell_other=venue_sell_other,
                        taker_buy=taker_buy, taker_sell=taker_sell)
    print(json.dumps(T, indent=1))


if __name__ == "__main__":
    main()

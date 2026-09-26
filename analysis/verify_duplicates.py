"""Exact duplicate records in the raw order-status archive.

A record is an exact repeat when all 54 bytes equal those of an earlier record of the
same file. For every hourly file this counts repeats, splits them by status, and counts
how many *live* order records (status open/triggered) are repeats, because a repeated
`open` creates a second copy of the same order in a replay.

Output: analysis/output/verification_duplicates.json
"""
from __future__ import annotations

import gzip
import json
import time
import warnings
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[1]
RAW = REPO / "src" / "data" / "order_statuses" / "sol"
OUT = Path(__file__).parent / "output"


def record_keys(raw: bytes) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    n = len(raw) // 54
    buf = np.frombuffer(raw[: n * 54], dtype=np.uint8).reshape(n, 54)
    pad = np.zeros((n, 56), dtype=np.uint8)
    pad[:, :54] = buf
    words = pad.view("<u8")                    # (n, 7)
    key = words[:, 0].copy()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        for i in range(1, 7):
            key = key * np.uint64(1_000_003) ^ words[:, i]
    return key, buf[:, 13].copy(), buf[:, 40].copy()   # key, statusId, isTrigger


def main():
    per_file = []
    tot = {"records": 0, "repeats": 0, "live_records": 0, "live_repeats": 0, "files_with_repeats": 0}
    by_status: dict[int, int] = {}
    t0 = time.time()
    for day in sorted(p.name for p in RAW.iterdir()):
        for f in sorted((RAW / day).glob("sol_*.data.gz")):
            raw = gzip.open(f, "rb").read()
            key, st, trig = record_keys(raw)
            _, first_idx = np.unique(key, return_index=True)
            is_first = np.zeros(len(key), dtype=bool)
            is_first[first_idx] = True
            rep = ~is_first
            live = ((st == 1) & (trig == 0)) | (st == 9)
            row = {"file": f"{day}/{f.name}", "records": int(len(key)), "repeats": int(rep.sum()),
                   "live_records": int(live.sum()), "live_repeats": int((live & rep).sum())}
            per_file.append(row)
            tot["records"] += row["records"]
            tot["repeats"] += row["repeats"]
            tot["live_records"] += row["live_records"]
            tot["live_repeats"] += row["live_repeats"]
            tot["files_with_repeats"] += row["repeats"] > 0
            for s, c in zip(*np.unique(st[rep], return_counts=True)):
                by_status[int(s)] = by_status.get(int(s), 0) + int(c)
        print(f"[{day}] cumulative repeats {tot['repeats']:,} of {tot['records']:,} ({time.time() - t0:.0f}s)", flush=True)
    tot["repeats_by_status"] = by_status
    worst = sorted(per_file, key=lambda r: -r["repeats"])[:10]
    OUT.mkdir(exist_ok=True)
    (OUT / "verification_duplicates.json").write_text(json.dumps({"totals": tot, "worst_files": worst, "per_file": per_file}, indent=1))
    print(json.dumps({"totals": tot, "worst_files": worst}, indent=1))


if __name__ == "__main__":
    main()

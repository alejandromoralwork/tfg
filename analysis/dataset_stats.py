"""Dataset-level descriptive stats that aren't in the per-second timeseries
CSVs -- distinct participant count and inter-arrival time -- computed
directly from the raw 54-byte binary order-status records (schema:
data/SCHEMA.md). Message-type counts (new/cancellation/other) come from
`market_sim scan sol`'s own classification instead of being re-derived here
in Python, since that logic already lives once, correctly, in the Rust
engines (`Order::is_new_live_order`/`is_cancellation`) and re-implementing
it independently risks silently disagreeing with it -- see
analysis/output/scan_stats.json (produced by `cargo run --release -- scan
sol` from src/, saved once; re-run that if the raw data changes).

~640M records / 6.2GB compressed across 744 files -- takes several minutes.
"""

import gzip
import json
import time

import numpy as np

import config

RECORD_DTYPE = np.dtype([
    ("ts", "<u8"), ("userId", "<u4"), ("isBuilder", "?"), ("statusId", "<u1"),
    ("isAsk", "?"), ("limitPx", "<u4"), ("sz", "<u4"), ("oid", "<u8"),
    ("timestampDiff", "<u4"), ("triggerCondition", "<i4"), ("triggered", "?"),
    ("isTrigger", "?"), ("hasChildren", "?"), ("isPositionTpsl", "?"),
    ("reduceOnly", "?"), ("orderTypeId", "<u1"), ("tifId", "<u1"),
    ("triggerPx", "<u4"), ("origSz", "<u4"),
])
assert RECORD_DTYPE.itemsize == 54

# Sample every Nth inter-arrival gap (within a file, where ts is monotonic)
# rather than keeping all ~639M diffs in memory -- a systematic sample this
# size gives a median accurate to well within reporting precision.
_SAMPLE_STRIDE = 200


def compute():
    files = sorted(config.RAW_DATA_DIR.glob("*/*.data.gz"))
    if not files:
        raise FileNotFoundError(f"no .data.gz files under {config.RAW_DATA_DIR}")

    t0 = time.time()
    total_records = 0
    distinct_users = set()
    order_type_counts = np.zeros(256, dtype=np.int64)
    status_counts = np.zeros(256, dtype=np.int64)
    diff_samples = []
    ts_min, ts_max = None, None

    for i, path in enumerate(files):
        with gzip.open(path, "rb") as f:
            buf = f.read()
        n = len(buf) // RECORD_DTYPE.itemsize
        arr = np.frombuffer(buf, dtype=RECORD_DTYPE, count=n)

        total_records += n
        distinct_users.update(np.unique(arr["userId"]).tolist())
        order_type_counts += np.bincount(arr["orderTypeId"], minlength=256)
        status_counts += np.bincount(arr["statusId"], minlength=256)

        ts = arr["ts"].astype(np.int64)
        if n:
            file_min, file_max = ts[0], ts[-1]
            ts_min = file_min if ts_min is None else min(ts_min, file_min)
            ts_max = file_max if ts_max is None else max(ts_max, file_max)
        if n > 1:
            diffs = np.diff(ts)
            diff_samples.append(diffs[::_SAMPLE_STRIDE])

        if (i + 1) % 100 == 0:
            print(f"  ... {i+1}/{len(files)} files, {total_records:,} records so far "
                  f"({time.time()-t0:.0f}s elapsed)")

    all_diffs = np.concatenate(diff_samples) if diff_samples else np.array([], dtype=np.int64)
    # negative diffs would only occur at file-boundary joins we didn't stitch;
    # within-file diffs are guaranteed >=0 since ts is monotonic per file.
    if len(all_diffs):
        median_inter_arrival_ns = float(np.median(all_diffs))
        mean_inter_arrival_ns = float(np.mean(all_diffs))
        zero_gap_fraction = float(np.mean(all_diffs == 0))
        pct_labels = [10, 25, 50, 75, 90, 95, 99, 99.9]
        percentiles_ns = {str(p): float(np.percentile(all_diffs, p)) for p in pct_labels}
    else:
        median_inter_arrival_ns = mean_inter_arrival_ns = zero_gap_fraction = None
        percentiles_ns = {}

    result = {
        "files_scanned": len(files),
        "total_records": int(total_records),
        "distinct_participants": len(distinct_users),
        "ts_min_ns": int(ts_min),
        "ts_max_ns": int(ts_max),
        "median_inter_arrival_ns": median_inter_arrival_ns,
        "median_inter_arrival_ms": (median_inter_arrival_ns / 1e6
                                     if median_inter_arrival_ns is not None else None),
        "mean_inter_arrival_ns": mean_inter_arrival_ns,
        "mean_inter_arrival_ms": (mean_inter_arrival_ns / 1e6
                                   if mean_inter_arrival_ns is not None else None),
        "zero_gap_fraction": zero_gap_fraction,
        "inter_arrival_percentiles_ns": percentiles_ns,
        "inter_arrival_sample_size": int(len(all_diffs)),
        "order_type_counts": {int(k): int(v) for k, v in enumerate(order_type_counts) if v},
        "status_counts": {int(k): int(v) for k, v in enumerate(status_counts) if v},
        "elapsed_secs": time.time() - t0,
    }
    return result


if __name__ == "__main__":
    stats = compute()
    out_path = config.OUTPUT_DIR / "dataset_stats.json"
    out_path.write_text(json.dumps(stats, indent=2))
    print(json.dumps(stats, indent=2))
    print(f"wrote {out_path}")

    scan_stats = json.loads((config.OUTPUT_DIR / "scan_stats.json").read_text())
    if stats["total_records"] != scan_stats["total_records"]:
        raise AssertionError(
            f"record count mismatch: python parser saw {stats['total_records']}, "
            f"Rust scan saw {scan_stats['total_records']} -- investigate before trusting either"
        )
    print("cross-check OK: record count matches `market_sim scan sol` exactly")

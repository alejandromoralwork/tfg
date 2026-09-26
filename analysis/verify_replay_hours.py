"""Replay fidelity on a random sample of whole hours, with the independent CDA reference engine.

For each sampled hour of the raw month the reference CDA (analysis/verify_reference_engines.py,
proved equal to the Rust CDA on four full hours) is replayed from an empty book, and its executed
volume is compared with the volume the venue itself recorded: the sum of the `filled` records of
TAKER orders (records written at the order's own submission time, timestampDiff == 0). Every trade has
exactly one taker, so this sum is the venue's traded volume.

It also splits the simulated volume by the kind of taker:
  post-only (tifId == 0, "Alo")  such an order can never execute as a taker on the venue: it is
                                 rejected if it would cross. The replay does not enforce time-in-force.
  same-user trades               the replay does not prevent self-trades.
and reports the remainder ("plausible" volume) against the venue volume.

Output: analysis/output/verification_replay_hours.json
"""
from __future__ import annotations

import json
import random
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from verify_reference_engines import load_hour, reference_cda  # noqa: E402

REPO = Path(__file__).resolve().parents[1]
RAW = REPO / "src" / "data" / "order_statuses" / "sol"
OUT = Path(__file__).parent / "output"


def one_hour(path: str):
    d = load_hour(Path(path))
    anchor = d["ts"][0]
    trades, _dup, _snaps, _eff, self_vol, alo_vol = reference_cda(d, anchor)
    sim = float(sum(q for _, q, _ in trades))
    sim_sec = {}
    for b, q, _p in trades:
        sim_sec[b] = sim_sec.get(b, 0) + q
    ven_sec = d["venue_taker_by_sec"]
    gaps = sorted(((sim_sec.get(b, 0) - ven_sec.get(b, 0)) for b in set(sim_sec) | set(ven_sec)), reverse=True)
    excess = sum(g for g in gaps if g > 0)
    top15 = sum(gaps[:15])
    worst_b = max(set(sim_sec) | set(ven_sec), key=lambda b: sim_sec.get(b, 0) - ven_sec.get(b, 0))
    return {"file": Path(path).parent.name + "/" + Path(path).name, "records": d["n_all"], "sim_volume": sim,
            "venue_taker_volume": float(d["venue_taker_volume"]), "sim_taker_post_only": float(alo_vol),
            "sim_self_trades": float(self_vol), "sim_trades": len(trades),
            "excess_total": float(excess), "excess_in_top15_seconds": float(top15),
            "largest_excess_second": {"sim": float(sim_sec.get(worst_b, 0)), "venue": float(ven_sec.get(worst_b, 0))}}


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 24
    files = sorted(RAW.glob("*/sol_*.data.gz"))
    rng = random.Random(20251201)
    pick = sorted(rng.sample(files, n))
    with ProcessPoolExecutor(max_workers=3) as ex:
        rows = []
        for r in ex.map(one_hour, [str(p) for p in pick]):
            rows.append(r)
            print(r, flush=True)
    tot = {k: sum(r[k] for r in rows) for k in ("sim_volume", "venue_taker_volume", "sim_taker_post_only", "sim_self_trades", "records", "sim_trades")}
    tot["excess_total"] = sum(r["excess_total"] for r in rows)
    tot["excess_top15_share"] = sum(r["excess_in_top15_seconds"] for r in rows) / tot["excess_total"]
    tot["sim_ex_post_only"] = tot["sim_volume"] - tot["sim_taker_post_only"]
    tot["ratio_sim_over_venue"] = tot["sim_volume"] / tot["venue_taker_volume"]
    tot["ratio_ex_post_only_over_venue"] = tot["sim_ex_post_only"] / tot["venue_taker_volume"]
    tot["post_only_share_of_sim"] = tot["sim_taker_post_only"] / tot["sim_volume"]
    for r in rows:
        r["ratio_sim_over_venue"] = r["sim_volume"] / r["venue_taker_volume"] if r["venue_taker_volume"] else None
        r["ratio_ex_post_only_over_venue"] = (r["sim_volume"] - r["sim_taker_post_only"]) / r["venue_taker_volume"] if r["venue_taker_volume"] else None
    (OUT / "verification_replay_hours.json").write_text(json.dumps({"hours": rows, "totals": tot}, indent=1))
    print(json.dumps(tot, indent=1))


if __name__ == "__main__":
    main()

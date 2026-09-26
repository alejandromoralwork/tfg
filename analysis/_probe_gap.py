import gzip, sys
import numpy as np
sys.path.insert(0, "analysis")
from verify_reference_engines import load_hour, reference_cda
from verify_raw_data import DTYPE, scaled
gz = "C:/Users/pc/AppData/Local/Temp/claude/c--Users-pc-other/f5341f23-a7d7-4982-b5d7-aeeb2901ca0a/scratchpad/e3/hour_quiet/20251225/sol_04.data.gz"
d = load_hour(__import__("pathlib").Path(gz))
anchor = d["ts"][0]
tr, *_ = reference_cda(d, anchor)
sim = {}
for b, q, p in tr:
    sim[b] = sim.get(b, 0) + q
a = np.frombuffer(gzip.open(gz, "rb").read(), dtype=DTYPE)
last = {}
ven = {}
for oid, st, sz, orig, td, ts in zip(a["oid"].tolist(), a["statusId"].tolist(), scaled(a["sz"]).tolist(), scaled(a["origSz"]).tolist(), a["timestampDiff"].tolist(), a["ts"].tolist()):
    if st == 5:
        prev = last.get(oid, orig)
        if td == 0 and prev > sz:
            b = (ts - anchor) // 10**9
            ven[b] = ven.get(b, 0) + (prev - sz) / 1e6
    last[oid] = sz
print("sec: venue_taker  sim  (all seconds where either > 0, buckets 1800-1900)")
for b in range(1800, 1900):
    v, s = ven.get(b, 0), sim.get(b, 0)
    if v or s:
        print(b, round(v, 2), s)
tot_v = sum(ven.values()); tot_s = sum(sim.values())
print("hour totals venue", round(tot_v, 1), "sim", tot_s)
# how many seconds where sim > 0 and venue == 0, and vice versa
both = sum(1 for b in set(ven) | set(sim) if ven.get(b, 0) and sim.get(b, 0))
only_v = sum(1 for b in set(ven) | set(sim) if ven.get(b, 0) and not sim.get(b, 0))
only_s = sum(1 for b in set(ven) | set(sim) if sim.get(b, 0) and not ven.get(b, 0))
print("seconds with both", both, "venue only", only_v, "sim only", only_s)
print("--- largest gaps")
diffs = sorted(((sim.get(b, 0) - ven.get(b, 0), b) for b in set(ven) | set(sim)), reverse=True)[:8]
for dlt, b in diffs:
    print("sec", b, "sim", sim.get(b, 0), "venue_taker", round(ven.get(b, 0), 2))
print("sum of top-15 gaps:", sum(x for x, _ in sorted(((sim.get(b, 0) - ven.get(b, 0), b) for b in set(ven) | set(sim)), reverse=True)[:15]))

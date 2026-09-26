import sys, pathlib, collections
sys.path.insert(0, "analysis")
import verify_reference_engines as R
gz = pathlib.Path("C:/Users/pc/AppData/Local/Temp/claude/c--Users-pc-other/f5341f23-a7d7-4982-b5d7-aeeb2901ca0a/scratchpad/e3/hour_busy/20251215/sol_14.data.gz")
d = R.load_hour(gz); anchor = d["ts"][0]
bt = R.Batch()
info = collections.defaultdict(list)     # bucket -> [(close_bucket, price, batch_ts_bucket)]
ts_list = d["ts"]
state = {"open": None, "next": None}
def close(close_ts):
    if bt.n_live == 0: return
    res = bt.clear()
    if res is None: return
    p, dd, ss, fills, batch_ts = res
    if fills:
        info[(batch_ts - anchor)//R.W].append(((close_ts-anchor)//R.W, p, len(fills)))
for ts, oid, is_ask, px, qty, live, cancel, market in zip(ts_list, d["oid"], d["is_ask"], d["px"], d["qty"], d["live"], d["cancel"], d["market"]):
    if state["next"] is None: state["next"], state["open"] = ts + R.W, ts
    while ts >= state["next"]:
        close(state["next"]); state["open"], state["next"] = state["next"], state["next"] + R.W
    if live: bt.add(ts, oid, is_ask, px, qty, market)
    elif cancel: bt.cancel(oid)
multi = {b: v for b, v in info.items() if len({x[1] for x in v}) > 1 or len(v) > 1}
print("buckets with trades", len(info), "with >1 batch or >1 price", len(multi))
for b, v in list(multi.items())[:6]: print(b, v)

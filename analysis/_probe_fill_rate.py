import gzip, sys
import numpy as np
sys.path.insert(0, "analysis")
from verify_raw_data import DTYPE, scaled

start = 1766221553867476878
import datetime
t = datetime.datetime.utcfromtimestamp(start // 10**9)
day, hour = t.strftime("%Y%m%d"), t.hour
f = f"src/data/order_statuses/sol/{day}/sol_{hour:02d}.data.gz"
a = np.frombuffer(gzip.open(f, "rb").read(), dtype=DTYPE)
m = (a["ts"] >= start) & (a["ts"] < start + 10**9)
sub = a[m]
print(f, "records in the second:", len(sub))
oids = set(sub["oid"].tolist())
st_names = {0:"badAlo",1:"open",2:"canceled",3:"marginRej",4:"iocRej",5:"filled",6:"minNtlRej",7:"roCanceled",9:"triggered"}
print("statuses:", {st_names.get(k, k): int((sub["statusId"] == k).sum()) for k in np.unique(sub["statusId"])})
live = sub[((sub["statusId"] == 1) & (sub["isTrigger"] == 0)) | (sub["statusId"] == 9)]
print("live records:", len(live))
for r in live:
    o = int(r["oid"])
    print(" oid", o, "ask", int(r["isAsk"]), "px", scaled(np.array([r["limitPx"]]))[0] / 1e6, "orig", scaled(np.array([r["origSz"]]))[0] / 1e6,
          "type", int(r["orderTypeId"]), "trig", int(r["isTrigger"]), "ts_off_ms", (int(r["ts"]) - start) / 1e6)

print("--- byte-identical duplicates in this file:")
raw = gzip.open(f, "rb").read()
recs = np.frombuffer(raw, dtype=np.dtype((np.void, 54)))
u, c = np.unique(recs, return_counts=True)
print("records", len(recs), "distinct", len(u), "records that are exact repeats of an earlier record:", len(recs) - len(u))
dup_oids = np.frombuffer(u[c > 1].tobytes(), dtype=DTYPE)
print("status of repeated records:", {int(k): int((dup_oids["statusId"] == k).sum()) for k in np.unique(dup_oids["statusId"])})
print("sample:", [(int(r["oid"]), int(r["statusId"]), int(r["ts"])) for r in dup_oids[:6]])

import gzip, sys, heapq
import numpy as np
sys.path.insert(0, "analysis")
from verify_reference_engines import load_hour
from verify_raw_data import DTYPE, scaled
gz = "C:/Users/pc/AppData/Local/Temp/claude/c--Users-pc-other/f5341f23-a7d7-4982-b5d7-aeeb2901ca0a/scratchpad/e3/hour_quiet/20251225/sol_04.data.gz"
a = np.frombuffer(gzip.open(gz, "rb").read(), dtype=DTYPE)
anchor = int(a["ts"][0])
B = int(sys.argv[1])
m = ((a["ts"].astype(np.int64) - anchor) // 10**9 == B)
sub = a[m]
names = {1: "open", 2: "canceled", 3: "marginRej", 4: "iocRej", 5: "filled", 6: "minNtl", 7: "roCanc", 9: "triggered"}
print("records in second", B, len(sub))
for r in sub:
    if int(r["statusId"]) in (1, 9, 5):
        print(f"  +{(int(r['ts'])-anchor-B*10**9)/1e6:8.2f}ms oid {int(r['oid'])} {names.get(int(r['statusId']))} {'ASK' if r['isAsk'] else 'BID'} px {scaled(np.array([r['limitPx']]))[0]/1e6} sz {scaled(np.array([r['sz']]))[0]/1e6} orig {scaled(np.array([r['origSz']]))[0]/1e6} type {int(r['orderTypeId'])} tif {int(r['tifId'])} td {int(r['timestampDiff'])} trig {int(r['isTrigger'])}")

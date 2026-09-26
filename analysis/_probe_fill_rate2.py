import gzip, datetime, sys
import numpy as np
sys.path.insert(0, "analysis")
from verify_raw_data import DTYPE
starts = [1765802842867476878, 1766221553867476878, 1767122370867476878, 1767122371867476878, 1767122381867476878]
for st in starts:
    t = datetime.datetime.fromtimestamp(st // 10**9, datetime.timezone.utc)
    f = f"src/data/order_statuses/sol/{t:%Y%m%d}/sol_{t.hour:02d}.data.gz"
    raw = gzip.open(f, "rb").read()
    a = np.frombuffer(raw, dtype=DTYPE)
    m = (a["ts"] >= st) & (a["ts"] < st + 10**9)
    sub = a[m]
    live = sub[((sub["statusId"] == 1) & (sub["isTrigger"] == 0)) | (sub["statusId"] == 9)]
    recs = np.frombuffer(np.ascontiguousarray(live).tobytes(), dtype=np.dtype((np.void, 54)))
    u = len(np.unique(recs))
    print(f"{t:%Y-%m-%d %H:%M:%S}: live records in that second {len(live)}, distinct {u}, exact repeats {len(live)-u}")

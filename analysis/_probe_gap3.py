import sys, pathlib
sys.path.insert(0, "analysis")
from verify_reference_engines import load_hour, reference_cda
gz = pathlib.Path("C:/Users/pc/AppData/Local/Temp/claude/c--Users-pc-other/f5341f23-a7d7-4982-b5d7-aeeb2901ca0a/scratchpad/e3/hour_quiet/20251225/sol_04.data.gz")
d = load_hour(gz); anchor = d["ts"][0]
B = int(sys.argv[1])
def tr(ts, kind, oid, user, moid, muser, px, f):
    if (ts - anchor) // 10**9 == B:
        print(f"+{(ts-anchor-B*10**9)/1e6:8.2f}ms {kind:10} oid {oid} user {user}  vs maker oid {moid} user {muser}  px {px/1e6}  qty {f}")
reference_cda(d, anchor, trace=tr)

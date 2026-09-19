"""Single entrypoint: dataset_stats (skipped if already run -- it's the
slow step, ~10min over 6GB of gzipped binary) -> make_tables -> make_figures.
"""

import json
import time

import config
import dataset_stats
import make_tables
import make_figures


def main():
    t0 = time.time()

    ds_path = config.OUTPUT_DIR / "dataset_stats.json"
    if ds_path.exists():
        print(f"[1/3] dataset_stats.json already present, skipping raw-binary parse "
              f"(delete {ds_path} to force a re-run)")
    else:
        print("[1/3] parsing raw order-status binary files (slow, ~10min)...")
        stats = dataset_stats.compute()
        ds_path.write_text(json.dumps(stats, indent=2))
        print(f"wrote {ds_path}")

    print("[2/3] building figures...")
    figinfo = make_figures.main()

    print("[3/3] computing tables...")
    make_tables.main(extra_macros={
        "PricepathsDay": figinfo["pricepaths_day"],
        "PriceDiffMedianBps": f"{figinfo['median_diff_bps']:.2f}",
        "PriceDiffMeanBps": f"{figinfo['mean_diff_bps']:.2f}",
        "PriceDiffStdBps": f"{figinfo['std_diff_bps']:.2f}",
        "PriceDiffN": f"{figinfo['n']:,}",
    })

    print(f"done in {time.time()-t0:.0f}s. "
          f"generated_numbers.tex + figures/*.pdf are ready for ch5.")
    print(figinfo)


if __name__ == "__main__":
    main()

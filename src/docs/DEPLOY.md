# Running `market_sim` on Google Cloud Batch

`simulate` is a days-long job on a full coin. It is **resumable** — after every
input file it appends the settled metric rows to
`output/<slug>/{fba,cda}_timeseries.csv` and rewrites `output/<slug>/checkpoint.txt`
— so if the output directory lives on durable storage, a preempted or retried
Batch task picks up where it left off instead of restarting.

## 1. Non-interactive invocation

With arguments, `market_sim` runs one command and exits with its status; with no
arguments it opens the interactive prompt. The container's `ENTRYPOINT` is
`market_sim`, so:

```
market_sim simulate sol 1          # replay data/order_statuses/sol at 1s intervals
market_sim simulate /work/data/order_statuses/sol 1   # explicit path form
market_sim scan sol                # just count records
```

Exit codes: `0` ok (including "already fully processed"), `1` a run-time failure
(streaming / IO), `2` a bad request (no data found, incompatible checkpoint).
Interactive-only commands (`add`, `metrics`, …) print an error and exit `2`.

## 2. Build & push the image

From the repo root (the `Dockerfile` is there):

```bash
REGION=europe-west1
PROJECT=your-project
REPO=market-sim            # an Artifact Registry Docker repo

docker build -t "$REGION-docker.pkg.dev/$PROJECT/$REPO/market_sim:latest" .
docker push  "$REGION-docker.pkg.dev/$PROJECT/$REPO/market_sim:latest"
```

Runtime image is ~80–120 MB (Debian slim + the static-ish binary; `simulate`
needs no external tools).

## 3. Stage the data bucket (once)

`simulate sol` reads `data/order_statuses/sol/<date>/*.data.gz` **relative to the
working directory** (`/work` in the container). Mirror that layout into a GCS
bucket. From any machine with `curl` + `tar`:

```bash
./download_data.sh                                   # repo-root script → ./order_statuses/sol/...
gcloud storage cp -r order_statuses "gs://YOUR_DATA_BUCKET/"
# → gs://YOUR_DATA_BUCKET/order_statuses/sol/20251201/sol_00.data.gz ...
```

(The `mapdir` lookup tables are **not** needed by `simulate` — it reads the
numeric ids straight from the binary records.)

## 4. The Batch job

Mount the data bucket read-only at `/work/data` and a writable output bucket at
`/work/output`. Both via gcsfuse.

```jsonc
{
  "taskGroups": [{
    "taskSpec": {
      "runnables": [{
        "container": {
          "imageUri": "europe-west1-docker.pkg.dev/PROJECT/REPO/market_sim:latest",
          "commands": ["simulate", "sol", "1"],
          "volumes": ["/mnt/disks/data:/work/data:ro", "/mnt/disks/out:/work/output:rw"]
        }
      }],
      "volumes": [
        { "gcs": { "remotePath": "YOUR_DATA_BUCKET" }, "mountPath": "/mnt/disks/data",
          "mountOptions": ["implicit-dirs", "only-dir=."] },
        { "gcs": { "remotePath": "YOUR_OUTPUT_BUCKET" }, "mountPath": "/mnt/disks/out" }
      ],
      "computeResource": { "cpuMilli": 2000, "memoryMib": 6144 },
      "maxRetryCount": 3,
      "lifecyclePolicies": [{ "action": "RETRY_TASK", "actionCondition": { "exitCodes": [1, 50] } }]
    },
    "taskCount": 1
  }],
  "allocationPolicy": {
    "instances": [{ "policy": { "machineType": "e2-standard-2", "provisioningModel": "SPOT" } }]
  },
  "logsPolicy": { "destination": "CLOUD_LOGGING" }
}
```

Notes:

- **Memory** — the recorder retains roughly one input file's span of events
  (~1 h at hourly files); `6 GiB` is comfortable for SOL at 1 s intervals.
  Raise it for a smaller interval or a denser coin.
- **Disk** — data is fuse-mounted, not downloaded, so the boot disk can stay
  small. No large scratch needed.
- **`SPOT` + `maxRetryCount`** — preemption is safe: on retry the same
  `commands` re-run, find `checkpoint.txt` in `/work/output/sol/`, skip the
  finished files, and continue. Exit `2` (bad request) is *not* retried
  above — fix the job spec instead.
- **The resume seam** — a resumed run leaves a short gap of empty interval
  rows (~one file's span) where the in-flight window was when it stopped;
  everything after is exact. This is expected (see `ARCHITECTURE.md` §4).
- **Progress in logs** — when stdout is not a terminal (every container run)
  the progress bar emits one line per ~1 s update instead of a `\r`-redraw,
  so Cloud Logging stays readable.
- **CWD** — the container's `WORKDIR` is `/work`; `simulate sol` therefore
  resolves to `/work/data/order_statuses/sol` and writes `/work/output/sol/`.
  Line the mount paths up with that, or pass an absolute path.

## 5. Local smoke test

```bash
docker build -t market_sim .
mkdir -p out
docker run --rm \
  -v "$PWD/data:/work/data:ro" -v "$PWD/out:/work/output" \
  market_sim simulate data/sample/order_statuses/20251201 1
ls out/20251201/     # fba_timeseries.csv  cda_timeseries.csv  summary.txt  checkpoint.txt
```

Kill it mid-run (`docker kill`) and re-run the same command against the same
`out/` mount to see it resume.

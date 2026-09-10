# Deploying `market_sim` on Google Cloud Batch

A start-to-finish guide for running a full-coin `simulate` in the cloud, written
for someone who has **not** used Google Cloud before. Every step is a copy‑paste
command. Shell blocks are **PowerShell** (Windows) unless noted; the one or two
places bash differs are called out.

---

## 0. The mental model — what you're about to build

```
  YOUR PC                          GOOGLE CLOUD (one "project")
  ───────                          ───────────────────────────────────────────
  Dockerfile ──build&push──▶  Artifact Registry   (private image store)
                                        │  market_sim:v2
  src/data/order_statuses/sol ─upload─▶  GCS bucket  <proj>-data     (read-only, ~6 GB)
                                        ▲                  │
                                        │ gcsfuse mount     │ gcsfuse mount
                              ┌─────────┴──────────┐        ▼
                              │  Batch task on a   │   GCS bucket <proj>-output
                              │  Compute Engine VM │   /work/output/sol/
                              │  runs:             │     ├─ fba_timeseries.csv   (grows per file)
                              │  market_sim        │     ├─ cda_timeseries.csv
                              │    simulate sol 1  │     ├─ summary.txt          (at the end)
                              └────────────────────┘     └─ checkpoint.txt       (after every file)
                                        │
          VM preempted (exit 50001) / task fails (exit 1)  ──▶  Batch retries the task
                                        │                 market_sim reads checkpoint.txt
                                        ▼                 and RESUMES from the last finished file
                            (retry codes MUST include 50001 — see §6)
```

The pieces you'll create, once each:

| Thing | What it is | Why |
|---|---|---|
| **Project** | the billing + permissions boundary; everything lives inside one | Google groups all resources under a project id like `market-sim-470112` |
| **Region** | a datacenter location, e.g. `europe-west1` | pick one and use it for *everything* — image, buckets, VMs |
| **Artifact Registry repo** | a private Docker registry | Batch pulls your image from here |
| **`<proj>-data` bucket** | Cloud Storage (GCS) folder for the dataset | mounted read‑only into the container |
| **`<proj>-output` bucket** | GCS folder for results | mounted read‑write; the checkpoint lives here so retries can resume |
| **Batch job** | "run this container on a VM, then clean up" | the actual run; described by one JSON file |
| **`gcloud`** | the command‑line tool that drives all of the above | you'll install it in step 1 |

`market_sim` itself needs nothing cloud‑specific: with arguments it runs one
command and exits with a status (`0` ok, `1` a run‑time failure that is safe to
retry/resume, `2` a bad request that won't get better on retry). The container's
`ENTRYPOINT` is `market_sim`, so a Batch `commands: ["simulate","sol","1"]`
becomes `market_sim simulate sol 1`.

---

## 1. Install the `gcloud` CLI and sign in

**Install** (pick one):

```powershell
winget install --id Google.CloudSDK -e
# then close and reopen PowerShell so `gcloud` is on PATH
```

or download **`GoogleCloudSDKInstaller.exe`** from
<https://cloud.google.com/sdk/docs/install> and run it.

**Sign in and pick a project:**

```powershell
gcloud init          # interactive: opens a browser to log in, then lets you pick/create a project
```

If you don't have a project yet, `gcloud init` offers to make one, or:

```powershell
gcloud projects create market-sim-$(Get-Random) --name="market sim"
gcloud config set project market-sim-XXXXXX          # use the id it printed
```

A project needs **billing enabled** before Batch/Compute will run. Easiest in the
web console: **console.cloud.google.com → Billing → link a billing account** to
your project. (New accounts get $300 free credit; a multi‑day run here costs a
few dollars — see §10.)

**Set your defaults** so you don't repeat them:

```powershell
gcloud config set compute/region europe-west1
gcloud config list                    # sanity check: account + project + region
```

Throughout this doc these two variables stand in for your values:

```powershell
$PROJECT = gcloud config get-value project
$REGION  = "europe-west1"
```

(bash: `PROJECT=$(gcloud config get-value project); REGION=europe-west1`)

---

## 2. One‑time project setup: enable APIs

Google APIs are off by default. Turn on the ones this needs:

```powershell
gcloud services enable `
  batch.googleapis.com `
  compute.googleapis.com `
  artifactregistry.googleapis.com `
  storage.googleapis.com `
  logging.googleapis.com `
  cloudbuild.googleapis.com
```

(In PowerShell the backtick `` ` `` continues a line. In bash use `\`.)

---

## 3. Build the container image and push it to Artifact Registry

You don't need Docker installed locally — **Cloud Build** builds it for you.

**3a. Create the registry repo (once):**

```powershell
gcloud artifacts repositories create market-sim `
  --repository-format=docker `
  --location=$REGION `
  --description="market_sim images"
```

**3b. Build + push from the repo root** (where the `Dockerfile` is —
`c:\Users\pc\other`):

```powershell
cd c:\Users\pc\other
$TAG = "v2"        # bump every time the source changes — Batch pins one tag
gcloud builds submit `
  --tag "$REGION-docker.pkg.dev/$PROJECT/market-sim/market_sim:$TAG" `
  .
```

This uploads the build context (a repo `.gcloudignore` excludes `src/data`,
`src/target`, `src/output`, `.git`, docs — so the upload is a few hundred KB),
builds the multi‑stage `Dockerfile` remotely, and pushes `…/market_sim:$TAG`.
Takes ~3–5 minutes. The final line prints the image digest.

**If you *do* have Docker Desktop** and prefer building locally:

```powershell
gcloud auth configure-docker "$REGION-docker.pkg.dev"
docker build -t "$REGION-docker.pkg.dev/$PROJECT/market-sim/market_sim:$TAG" .
docker push       "$REGION-docker.pkg.dev/$PROJECT/market-sim/market_sim:$TAG"
```

`deploy/batch-sol.json` currently pins **`:v2`** (the build with the audited
metric formulas + `test engine metrics`). Bump both the build `$TAG` and the
`imageUri` in the spec together whenever you rebuild. Sanity-check a fresh image
with `docker run --rm <image> test engine all` (needs Docker locally) — it exits
`0` only if every built-in checklist passes.

---

## 4. Create the storage buckets and upload the dataset

Bucket names are **globally unique** — prefix with your project id.

```powershell
gcloud storage buckets create "gs://$PROJECT-data" `
  --location=$REGION --uniform-bucket-level-access
gcloud storage buckets create "gs://$PROJECT-output" `
  --location=$REGION --uniform-bucket-level-access
```

**Upload the SOL order‑status data.** Layout matters: inside the container the
working directory is `/work`, and `simulate sol` looks for
`data/order_statuses/sol/<date>/*.data.gz`. We'll mount `gs://$PROJECT-data` at
`/work/data`, so the bucket must contain `order_statuses/sol/<date>/…`.

You already have the data locally (from a previous `download sol` / the
`download_data.sh` script) at `src\data\order_statuses\sol\` (~6 GB). Upload it:

```powershell
gcloud storage cp -r "src\data\order_statuses\sol" `
  "gs://$PROJECT-data/order_statuses/"
```

Result: `gs://$PROJECT-data/order_statuses/sol/20251201/sol_00.data.gz`, …
Verify:

```powershell
gcloud storage ls "gs://$PROJECT-data/order_statuses/sol/**" | Select-Object -First 5
```

### 4b. Create directory markers (required)

`gcloud storage cp -r` only uploads the real `.data.gz` files — it does **not**
create objects for the `order_statuses/`, `order_statuses/sol/`,
`order_statuses/sol/<date>/` folders in between. Batch mounts the bucket with
**gcsfuse**, and the gcsfuse version on the Batch VM image can't use the
`implicit-dirs` option through Batch's mount layer (passing it makes the mount
fail — see §6 and Troubleshooting). Without `implicit-dirs`, gcsfuse only shows a
folder if a real `"<path>/"` object exists, so `market_sim` can't list the date
folders unless you create those markers:

```powershell
.\deploy\make-gcs-dir-markers.ps1 -Bucket "$PROJECT-data" -Coin sol
```

This creates ~33 zero-byte marker objects (`order_statuses/`,
`order_statuses/sol/`, one per date folder). It's idempotent — re-run it whenever
you upload more date folders. Verify:

```powershell
gcloud storage ls "gs://$PROJECT-data/order_statuses/sol/" | Select-Object -First 3
# each line should end in a date folder like .../20251201/
```

(The output bucket needs no markers — `market_sim` creates `sol/…` itself via a
normal `mkdir`, which gcsfuse writes as a real folder object.)

Notes:
- The `.data.gz` files are already compressed — don't add `--gzip-in-flight`.
- ~6 GB over a home connection can take a while. `gcloud storage cp -r` uploads
  each object atomically, so if it's interrupted, re‑running the same command
  skips what already uploaded (it compares, uploads only the missing/changed).
- **Faster alternative:** create a tiny VM in `$REGION`, `git clone` the repo
  there, run `./download_data.sh` (fast cloud bandwidth from Zenodo), then
  `gcloud storage cp -r order_statuses gs://$PROJECT-data/` from the VM
  (same‑region, free, fast). Delete the VM after.
- The `mapdir` lookup tables are **not** needed — `simulate` reads the numeric
  ids straight from the binary records.
- The `<proj>-output` bucket starts empty; Batch fills it.

---

## 5. Permissions

Batch runs the task as a **service account** (a robot identity). The default one
is the Compute Engine default service account. Grant it read on the data bucket,
write on the output bucket, and pull on the image repo:

```powershell
$PNUM = gcloud projects describe $PROJECT --format="value(projectNumber)"
$SA   = "$PNUM-compute@developer.gserviceaccount.com"

gcloud storage buckets add-iam-policy-binding "gs://$PROJECT-data" `
  --member="serviceAccount:$SA" --role="roles/storage.objectViewer"

gcloud storage buckets add-iam-policy-binding "gs://$PROJECT-output" `
  --member="serviceAccount:$SA" --role="roles/storage.objectAdmin"

gcloud artifacts repositories add-iam-policy-binding market-sim `
  --location=$REGION `
  --member="serviceAccount:$SA" --role="roles/artifactregistry.reader"
```

If `gcloud batch jobs submit` later complains that **your** account can't create
jobs, grant yourself the Batch editor role. `$ME` is whatever account `gcloud`
is currently logged in as — don't type an email, let `gcloud` fill it in:

```powershell
$ME = gcloud config get-value account
gcloud projects add-iam-policy-binding $PROJECT `
  --member="user:$ME" --role="roles/batch.jobsEditor"
gcloud projects add-iam-policy-binding $PROJECT `
  --member="serviceAccount:$SA" --role="roles/batch.agentReporter"
```

(If you own the project — you created it — you're already `roles/owner` and can
skip the `batch.jobsEditor` line; the `batch.agentReporter` one for `$SA` is
still needed.)

---

## 6. The Batch job spec

The repo ships **`deploy/batch-sol.json`**, already filled in for this project
(`market-sim-26072001`, region `europe-west1`, buckets `market-sim-26072001-data`
/ `-output`). It looks like this:

```jsonc
{
  "taskGroups": [
    {
      "taskCount": 1,
      "taskSpec": {
        "runnables": [
          {
            "container": {
              "imageUri": "europe-west1-docker.pkg.dev/market-sim-26072001/market-sim/market_sim:v2",
              "commands": ["simulate", "sol", "1"],
              "volumes": [
                "/mnt/disks/data:/work/data:ro",
                "/mnt/disks/out:/work/output:rw"
              ]
            }
          }
        ],
        "volumes": [
          {
            "gcs": { "remotePath": "market-sim-26072001-data" },
            "mountPath": "/mnt/disks/data"
          },
          {
            "gcs": { "remotePath": "market-sim-26072001-output" },
            "mountPath": "/mnt/disks/out"
          }
        ],
        "computeResource": { "cpuMilli": 4000, "memoryMib": 8192 },
        "maxRetryCount": 3,
        "maxRunDuration": "604800s",
        "lifecyclePolicies": [
          { "action": "RETRY_TASK", "actionCondition": { "exitCodes": [1, 50001] } }
        ]
      }
    }
  ],
  "allocationPolicy": {
    "location": { "allowedLocations": ["regions/europe-west1"] },
    "instances": [
      {
        "policy": {
          "machineType": "e2-standard-4",
          "provisioningModel": "STANDARD",
          "bootDisk": { "sizeGb": 30 }
        }
      }
    ]
  },
  "logsPolicy": { "destination": "CLOUD_LOGGING" }
}
```

**What each part does:**

- **`runnables[].container.commands`** — appended to the image `ENTRYPOINT`
  (`market_sim`), so this runs `market_sim simulate sol 1`. CWD is `/work`, so it
  reads `/work/data/order_statuses/sol/…` and writes `/work/output/sol/…`. To run
  a different coin or interval, change these (e.g. `["simulate","sol","5"]`), or
  point at an explicit path (`["simulate","/work/data/order_statuses/sol","1"]`).
- **Two mount layers** — `taskSpec.volumes[]` gcsfuse‑mounts each bucket onto a
  path *on the VM* (`/mnt/disks/data`, `/mnt/disks/out`); the runnable's
  `container.volumes[]` then bind‑mounts those VM paths *into the container*
  — `/work/data` with `:ro` (read‑only; the service account also only has
  *view* on that bucket) and `/work/output` with `:rw`.
- **No `mountOptions`** — deliberately absent. Batch's gcsfuse mount layer
  mangles `mountOptions: ["implicit-dirs"]` into an invalid command
  (`gcsfuse implicit-dirs -implicit-dirs …`) and the mount fails before the
  container starts. Instead of `implicit-dirs`, the directory‑marker objects from
  §4b let gcsfuse see the `order_statuses/sol/<date>/` folders.
- **`computeResource`** — `market_sim` keeps only ~one input file's span of
  events in memory (the streaming/prune design), so 8 GiB is generous for SOL at
  a 1‑second interval. Raise `memoryMib` for a sub‑second interval or the denser
  `btc` coin; `cpuMilli: 4000` = 4 vCPU (the replay is single‑threaded but the
  gzip decode + engines keep one core busy — 2 vCPU also works).
- **`bootDisk.sizeGb: 30`** — the dataset is fuse‑mounted, not downloaded, so no
  big scratch disk is needed.
- **`provisioningModel: "STANDARD"`** — on‑demand VM. ~3× the price of Spot
  (`e2-standard-4` ≈ $0.13/hr vs ~$0.04/hr) but **not preemptible** — one clean
  ~13 h pass for ≈ $2. The spec ships with STANDARD because Spot proved
  unusable for this workload: in `europe-west1` the first two full runs were
  preempted **repeatedly** — one burned all 10 retries in ~2 h, each VM reclaimed
  after only 2–24 min. Spot is only worth it if you can tolerate a run that may
  never finish. To try Spot anyway: set `"provisioningModel": "SPOT"` **and keep
  `50001` in the retry `exitCodes` below** (Batch reports a preemption as task
  exit code `50001`; without it in the list the whole job fails on the first
  preemption), and consider adding more zones to
  `allocationPolicy.location.allowedLocations` so retries can land elsewhere.
- **`lifecyclePolicies` → RETRY_TASK on exit `1` and `50001`** — a run‑time
  failure (`1`: transient IO, a gcsfuse hiccup) and a Spot preemption (`50001`)
  are both safe to retry because of the checkpoint. **A `lifecyclePolicies` block
  overrides Batch's default retry behaviour** — an exit code not listed here is
  *not* retried even though `maxRetryCount` is set. **Exit `2` is deliberately
  left out** — it means a bad request (bucket empty, wrong path, incompatible
  checkpoint version); retrying won't help, so the task fails and you fix the
  spec. (This works, but only masks Spot instability — it does not create
  capacity. If every retry is preempted, the job still fails; go STANDARD.)
- **`maxRetryCount: 3`** — for STANDARD, a small margin for a genuine transient
  failure. On retry the same `commands` re‑run; `market_sim` finds
  `checkpoint.txt` in `/work/output/sol/`, skips the finished input files, and
  **resumes** (each resume leaves a short gap of empty interval rows — ~one
  file's span — at the seam, documented in `ARCHITECTURE.md` §4). Raise to the
  Batch maximum of `10` only if you switch back to Spot.
- **`maxRunDuration: "604800s"`** — 7 days; raise it for a whole month of data.
- **`logsPolicy: CLOUD_LOGGING`** — stdout/stderr go to Cloud Logging. Because
  stdout isn't a terminal, the progress bar prints one line per ~1 s update
  instead of a `\r`‑redraw, so the logs stay readable.

`deploy/batch-sol.json` is already filled in for this project, so there's nothing
to substitute — use it as‑is in step 7. (If you ever fork this for a *different*
project or region, the values to change are the `imageUri`, both
`gcs.remotePath`s, and `allowedLocations`.)

---

## 7. Submit the job and watch it

```powershell
gcloud batch jobs submit sol-1s `
  --location=$REGION `
  --config=deploy\batch-sol.json
```

`sol-1s` is the job name (lowercase, digits, `-`). Watch it:

```powershell
# high-level state: QUEUED -> SCHEDULED -> RUNNING -> SUCCEEDED / FAILED
gcloud batch jobs describe sol-1s --location=$REGION --format="value(status.state)"

# per-task state + any status events (why it's queued, retries, ...)
gcloud batch jobs describe sol-1s --location=$REGION --format="yaml(status)"

# the application's stdout/stderr (the [OK] / -> [n/N] / progress lines)
$JUID = gcloud batch jobs describe sol-1s --location=$REGION --format="value(uid)"
gcloud logging read ('labels."batch.googleapis.com/job_uid"="' + $JUID + '"') `
  --project=$PROJECT --order=asc --limit=200 --format="value(textPayload)"
```

(bash: `JUID=$(gcloud batch jobs describe sol-1s --location=$REGION --format='value(uid)'); gcloud logging read "labels.\"batch.googleapis.com/job_uid\"=\"$JUID\"" --project=$PROJECT --order=asc --limit=200 --format='value(textPayload)'`)

Easiest by far, though: the **web console** — <https://console.cloud.google.com/batch>
→ **sol-1s** → the **Logs** tab shows the live output; and **Cloud Storage** →
`<proj>-output` → `sol/` shows `fba_timeseries.csv` / `checkpoint.txt` growing in
real time (one file's worth of rows per finished input file).

---

## 8. Collect the results

```powershell
gcloud storage cp -r "gs://$PROJECT-output/sol" .\results\
# results\sol\{fba,cda}_timeseries.csv  summary.txt  checkpoint.txt
```

In `checkpoint.txt`, `complete true` means the whole coin finished and
`summary.txt` is final. If a job ended (`FAILED`, cancelled, retries exhausted)
with `complete false`, just **submit the job again** (delete the old one first,
§9) — it resumes from where the checkpoint left off.

---

## 9. Re-running, resuming, starting over

```powershell
# a job name can't be reused while the old job exists — delete it first
gcloud batch jobs delete sol-1s --location=$REGION --quiet

# resume: re-submit the SAME config. market_sim sees the checkpoint in the
# output bucket and continues from the last finished file.
gcloud batch jobs submit sol-1s --location=$REGION --config=deploy\batch-sol.json

# start completely fresh: wipe the output prefix first
gcloud storage rm -r "gs://$PROJECT-output/sol"
```

A different coin or interval writes to a different `output/<slug>/` prefix
(`output/eth/`, and `output/<last path component>/` for an explicit path), so
runs don't collide.

---

## 10. Cost and cleanup

- **Compute** — `e2-standard-4` STANDARD (the spec default) ≈ **$0.13/hr**; the
  full ~13 h SOL run ≈ **$2**. Spot would be ~$0.04/hr but keeps getting
  preempted here (see §6). You only pay while a task is `RUNNING`.
- **Storage** — 6 GB in GCS ≈ **$0.12/month**. Downloading a few hundred MB of
  results is negligible egress.
- **Cleanup when done:**
  ```powershell
  gcloud batch jobs delete sol-1s --location=$REGION --quiet
  gcloud storage rm -r "gs://$PROJECT-data" "gs://$PROJECT-output"
  gcloud artifacts repositories delete market-sim --location=$REGION --quiet
  ```

---

## 11. Test locally first (no cloud, needs Docker Desktop)

Prove the image works on the bundled 1‑hour sample before spending cloud time:

```powershell
docker build -t market_sim .
mkdir out -Force
docker run --rm `
  -v "${PWD}\data:/work/data:ro" -v "${PWD}\out:/work/output" `
  market_sim simulate data/sample/order_statuses/20251201 1
Get-ChildItem out\20251201    # fba_timeseries.csv cda_timeseries.csv summary.txt checkpoint.txt
```

Kill it mid‑run (`docker kill $(docker ps -q --filter ancestor=market_sim)`) and
re‑run the same command against the same `out\` mount — it prints
`==> Resuming …` and finishes.

---

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `PERMISSION_DENIED` pulling the image | service account `$SA` missing `roles/artifactregistry.reader` on the `market-sim` repo (§5) |
| `PERMISSION_DENIED` / `403` on a bucket | `$SA` missing `storage.objectViewer` (data) or `storage.objectAdmin` (output) (§5) |
| `you do not have permission to submit jobs` | your user missing `roles/batch.jobsEditor` on the project (§5) |
| Job stuck in `QUEUED` for a long time | no SPOT capacity in the region — switch `provisioningModel` to `STANDARD`, or add more zones under `allocationPolicy.location.allowedLocations` (`zones/europe-west1-b`, …), or another region |
| Job `FAILED` with `Spot VM preemption with exit code 50001` | a Spot VM was reclaimed. If `50001` isn't in the RETRY_TASK `exitCodes`, the whole job fails on the first one — add it (§6). If it *is* in the list and the job still fails, every retry got preempted too (Spot capacity crunch in the region) — the spec now uses **`STANDARD`** for this reason. Re-submit; it resumes from `checkpoint.txt`. |
| `gcloud.batch.jobs.submit ... OUT_OF_RANGE: max_retry_count ... not in between 0 and 10` | `maxRetryCount` must be 0–10. Set it to 10. |
| Task fails at the mount step, `gcsfuse … accepts between 2 and 3 arg(s), received 4` | `mountOptions` in the job spec — remove them entirely (§6). Batch double‑emits each option and gcsfuse v3 rejects it. Use the §4b directory markers instead of `implicit-dirs`. |
| Task fails immediately, exit `2` | bad request: data bucket empty / wrong layout (`gcloud storage ls "gs://$PROJECT-data/order_statuses/sol/**"`), or the date folders have no marker objects (run `deploy\make-gcs-dir-markers.ps1`, §4b), or an incompatible `checkpoint.txt` from an older image version — wipe the output prefix (§9) |
| Task fails with exit `~137` | out of memory — raise `computeResource.memoryMib` |
| `input/output error` writing to `/work/output` | confirm `$SA` has `storage.objectAdmin` on the output bucket. `market_sim` `mkdir`s its own output subfolders, which gcsfuse handles without `implicit-dirs`. |
| CSVs never grow, only `checkpoint.txt` | on a single input file nothing flushes until the end (there's no "next file" to bound against). Multi‑file inputs grow per file. Also check the logs for `[ERROR]` |
| `Cargo.lock` / edition error during `gcloud builds submit` | the `Dockerfile` pins `rust:1.83`; if a future `Cargo.lock` needs newer, bump that tag |

---

## Appendix — `all` in one job vs. one job per coin

`market_sim simulate all 1` runs `btc`, then `eth`, then `sol` in sequence inside
one task (each writes its own `output/<coin>/`, each independently resumable). For
parallelism instead, submit three jobs (`commands: ["simulate","btc","1"]`, etc.)
pointing at the same buckets — they don't interfere.

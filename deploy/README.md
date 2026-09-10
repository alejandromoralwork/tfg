# `deploy/`

Google Cloud Batch job spec for running `market_sim simulate` in the cloud.

- **`batch-sol.json`** — the job spec, filled in for:
  - project `market-sim-26072001`
  - region `europe-west1` (same region as bucket `market-sim-26072001-data`)
  - image `europe-west1-docker.pkg.dev/market-sim-26072001/market-sim/market_sim:v2`
  - data bucket `market-sim-26072001-data`, output bucket `market-sim-26072001-output`

  For a different coin or interval, edit
  `taskGroups[0].taskSpec.runnables[0].container.commands`
  (e.g. `["simulate","eth","5"]`). Bump the `:v2` image tag when you rebuild.
  The GCS volumes have **no `mountOptions`** on purpose — Batch mangles
  `implicit-dirs` into a broken gcsfuse command. See `make-gcs-dir-markers.ps1`.
  `provisioningModel` is **`STANDARD`** (on-demand): Spot in `europe-west1`
  preempted every retry of the first two runs, burning all 10 attempts in ~2 h.
  STANDARD is ~3× the price (~$2 for the full run) but finishes in one pass.
  `lifecyclePolicies` still retries on exit `1` (resumable app failure) and
  `50001` (Spot preemption, in case you switch back); `maxRetryCount` is 3.

- **`make-gcs-dir-markers.ps1`** — run once after uploading data (and again after
  adding date folders). Creates the zero-byte `order_statuses/…/<date>/` marker
  objects gcsfuse needs to list the folders, since `implicit-dirs` can't be used:

  ```powershell
  .\deploy\make-gcs-dir-markers.ps1 -Bucket market-sim-26072001-data -Coin sol
  ```

Full walkthrough — installing `gcloud`, buckets, permissions, submit/monitor,
resume, cost — is in [`../src/docs/DEPLOY.md`](../src/docs/DEPLOY.md).

Submit, from the repo root:

```powershell
gcloud batch jobs submit sol-1s --location=europe-west1 --config=deploy/batch-sol.json
gcloud batch jobs describe sol-1s --location=europe-west1 --format="value(status.state)"
```

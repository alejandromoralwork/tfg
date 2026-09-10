# Create zero-byte "directory marker" objects in the data bucket.
#
# WHY: `gcloud storage cp -r` uploads only the real files
# (order_statuses/sol/<date>/sol_NN.data.gz). It does NOT create objects for the
# "folders" in between. Google Cloud Batch mounts the bucket with gcsfuse, and
# gcsfuse (v3, as shipped on the Batch VM image) does NOT support the
# `implicit-dirs` mount option through Batch's mount layer — passing it makes the
# mount fail outright. Without `implicit-dirs`, gcsfuse only shows a folder if a
# real object named "<path>/" exists. So `market_sim` can't list
# /work/data/order_statuses/sol/<date>/ unless we create those marker objects.
#
# This script creates exactly the folders market_sim needs to walk:
#   order_statuses/
#   order_statuses/<coin>/
#   order_statuses/<coin>/<date>/   (one per date folder already in the bucket)
#
# Idempotent - safe to re-run (re-uploading a marker is a no-op overwrite).
# Re-run it after you add more date folders to the bucket.
#
# Usage:
#   .\deploy\make-gcs-dir-markers.ps1 -Bucket market-sim-26072001-data -Coin sol

param(
  [Parameter(Mandatory = $true)] [string] $Bucket,
  [string] $Coin = "sol"
)

$ErrorActionPreference = "Stop"
$token = (gcloud auth print-access-token).Trim()

function New-Marker([string] $name) {
  $enc = $name.Replace("/", "%2F")
  $uri = "https://storage.googleapis.com/upload/storage/v1/b/$Bucket/o?uploadType=media&name=$enc"
  Invoke-RestMethod -Method Post -Uri $uri -Headers @{ Authorization = "Bearer $token" } `
    -ContentType "application/octet-stream" -Body ([byte[]]@()) | Out-Null
  Write-Host "  marker: $name"
}

New-Marker "order_statuses/"
New-Marker "order_statuses/$Coin/"

# enumerate the date folders that already exist as prefixes in the bucket
$dates = gcloud storage ls "gs://$Bucket/order_statuses/$Coin/" |
  ForEach-Object { if ($_ -match "/order_statuses/$Coin/(\d{8})/\s*$") { $Matches[1] } }

foreach ($d in $dates) { New-Marker "order_statuses/$Coin/$d/" }

Write-Host ""
Write-Host ("Done - {0} date folder(s) marked in gs://{1}" -f $dates.Count, $Bucket)

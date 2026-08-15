#!/usr/bin/env bash
#
# Downloads and extracts the Hyperliquid L4 order book dataset
# (Albers, Cucuringu, Howison & Shestopaloff, 2026) from Zenodo:
#   https://zenodo.org/records/18184441
#
# These files are excluded from git (see ../.gitignore) because they are far
# too large for GitHub. Run this script to (re)populate data/order_statuses/
# and data/mapdir/ locally.
#
# Usage:
#   ./download_data.sh                 # download, extract, delete archives afterwards
#   ./download_data.sh --keep-archives # keep the downloaded .tar.xz files too
#
# Requires: curl, tar (with xz support)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

KEEP_ARCHIVES=false
if [[ "${1:-}" == "--keep-archives" ]]; then
  KEEP_ARCHIVES=true
fi

log() { printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$1"; }

mkdir -p order_statuses trades

# name|url|extract_dir
FILES=(
  "mapdir.tar.xz|https://zenodo.org/records/18184441/files/mapdir.tar.xz?download=1|."
  "sol_orders_202512.tar.xz|https://zenodo.org/records/18184441/files/sol_orders_202512.tar.xz?download=1|order_statuses"
  "sol_rejected_202512.tar.xz|https://zenodo.org/records/18184441/files/sol_rejected_202512.tar.xz?download=1|order_statuses"
)

for entry in "${FILES[@]}"; do
  IFS='|' read -r fname url dest <<< "$entry"

  log "Downloading $fname ..."
  # -C - resumes a partial download; if the file is already complete, curl skips the transfer.
  curl -L -C - --fail --retry 3 -o "$fname" "$url"

  log "Extracting $fname -> $dest/"
  tar -xf "$fname" -C "$dest"

  if [[ "$KEEP_ARCHIVES" == false ]]; then
    log "Removing $fname to save disk space"
    rm -f "$fname"
  fi
done

log "Done."
log "Note: trade data (trades_2025_12.tar and other months) has no public Zenodo"
log "link supplied yet. Add a FILES entry above with its URL once you have one,"
log "or place the archive in data/ and extract it manually into data/trades/."

du -sh order_statuses mapdir 2>/dev/null || true

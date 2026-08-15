#!/usr/bin/env bash
set -e
cd /c/Users/pc/other/data

log() { echo "[$(date '+%H:%M:%S')] $1"; }

log "=== Extracting sol_orders_202512.tar.xz -> order_statuses/ ==="
tar -xf sol_orders_202512.tar.xz -C order_statuses/
log "done. removing archive."
rm -f sol_orders_202512.tar.xz
df -h /c

log "=== Extracting sol_rejected_202512.tar.xz -> order_statuses/ ==="
tar -xf sol_rejected_202512.tar.xz -C order_statuses/
log "done. removing archive."
rm -f sol_rejected_202512.tar.xz
df -h /c

log "=== Extracting trades_2025_12.tar -> trades/ ==="
tar -xf trades_2025_12.tar -C trades/
log "done. removing archive."
rm -f trades_2025_12.tar
df -h /c

log "=== Extracting mapdir.tar.xz ==="
tar -xf mapdir.tar.xz
log "done. removing archive."
rm -f mapdir.tar.xz
df -h /c

log "=== ALL DONE ==="
du -sh order_statuses trades mapdir 2>/dev/null

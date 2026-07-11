#!/bin/sh

set -e

if [ "${1:-}" = "upgrade" ]; then
    exit 0
fi

ROOT=/var/local/ksubstrate/tweaks
ID=com.bd452.ksubstratedemo
DEST="$ROOT/$ID"
RETIRED="$ROOT/.$ID.retired.$$"
if [ -e "$DEST" ]; then mv "$DEST" "$RETIRED"; fi
if ! /mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/app.sh post-package-change; then
    [ -e "$RETIRED" ] && mv "$RETIRED" "$DEST"
    exit 1
fi
rm -rf "$RETIRED"
rm -f /mnt/us/documents/com.bd452.ksubstratedemo.sh

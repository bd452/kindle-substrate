#!/bin/sh

set -e

PKG="$(CDPATH= cd "$(dirname "$0")" && pwd)"
ID=com.bd452.ksubstratedemo
ROOT=/var/local/ksubstrate/tweaks
SRC="$PKG/tweaks/$ID"
DEST="$ROOT/$ID"
STAGE="$ROOT/.$ID.staging.$$"
OLD="$ROOT/.$ID.retired.$$"

test -f "$SRC/manifest.json"
test -f "$SRC/tweak.so"
mkdir -p "$ROOT"
mkdir "$STAGE"
cp "$SRC/manifest.json" "$STAGE/manifest.json"
cp "$SRC/tweak.so" "$STAGE/tweak.so"
test -s "$STAGE/manifest.json"
test -s "$STAGE/tweak.so"

rollback() {
    if [ -e "$OLD" ] && [ ! -e "$DEST" ]; then mv "$OLD" "$DEST" || true; fi
    rm -rf "$STAGE"
}
trap rollback EXIT HUP INT TERM
if [ -e "$DEST" ]; then mv "$DEST" "$OLD"; fi
mv "$STAGE" "$DEST"
trap - EXIT HUP INT TERM
if ! /mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/app.sh post-package-change; then
    rm -rf "$DEST"
    [ -e "$OLD" ] && mv "$OLD" "$DEST"
    exit 1
fi
rm -rf "$OLD"
rm -f /mnt/us/documents/com.bd452.ksubstratedemo.sh
echo "Kindle Substrate Demo installed and registered. Open KUAL, then Kindle Substrate, to run it."

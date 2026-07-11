#!/bin/sh

set -e

PKG="$(CDPATH= cd "$(dirname "$0")" && pwd)"
ID=com.bd452.ksubstratedemo
ROOT=/var/local/ksubstrate/tweaks
SRC="$PKG/tweaks/$ID"
DEST="$ROOT/$ID"
STAGE="$ROOT/.$ID.staging.$$"
OLD="$ROOT/.$ID.retired.$$"

if [ -f /lib/ld-linux-armhf.so.3 ]; then
    PLAT=kindlehf
else
    PLAT=kindlepw2
fi
LIB="$SRC/lib/$PLAT/tweak.so"

test -f "$SRC/manifest.json"
test -f "$LIB"
mkdir -p "$ROOT"
mkdir "$STAGE"
cp "$SRC/manifest.json" "$STAGE/manifest.json"
cp "$LIB" "$STAGE/tweak.so"
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
echo "Kindle Substrate Demo installed and registered. KUAL can launch it."

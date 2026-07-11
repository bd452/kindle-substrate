#!/bin/sh

set -e

cd "$(dirname "$0")" || exit 1

if [ -f /lib/ld-linux-armhf.so.3 ]; then
    PLAT=kindlehf
else
    PLAT=kindlepw2
fi

RUNTIME="/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate"
KSUB="${RUNTIME}/bin/${PLAT}/ksubstrate"
TARGET="./bin/${PLAT}/ksubstrate-demo-target"

if [ ! -x "$KSUB" ]; then
    echo "ksubstrate CLI not found for ${PLAT} at ${KSUB}." >&2
    exit 1
fi

if [ ! -x "$TARGET" ]; then
    echo "ksubstrate demo target not found for ${PLAT} at ${TARGET}." >&2
    exit 1
fi

KSUBSTRATE_TWEAKS_DIR="$(pwd)/tweaks" exec "$KSUB" run "$TARGET"

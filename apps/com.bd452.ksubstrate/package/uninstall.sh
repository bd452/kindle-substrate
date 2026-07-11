#!/bin/sh

set -e

if [ "$1" = "upgrade" ]; then
    exit 0
fi

if [ -x ./app.sh ]; then
    ./app.sh disable 2>/dev/null || true
fi

rm -f /mnt/us/documents/com.bd452.ksubstrate-enable.sh
rm -f /mnt/us/documents/com.bd452.ksubstrate-disable.sh

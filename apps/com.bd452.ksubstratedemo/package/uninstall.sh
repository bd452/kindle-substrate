#!/bin/sh

set -e

if [ "${1:-}" = "upgrade" ]; then
    exit 0
fi

rm -f /mnt/us/documents/com.bd452.ksubstratedemo.sh

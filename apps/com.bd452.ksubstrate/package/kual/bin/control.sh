#!/bin/sh
set -u
TMP="/tmp/ksubstrate-kual.$$"
trap 'rm -f "$TMP"' EXIT HUP INT TERM

if /mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/app.sh "$@" >"$TMP" 2>&1; then
    rc=0
else
    rc=$?
fi
while IFS= read -r line; do
    logger -t ksubstrate "KUAL $*: $line"
done < "$TMP"
exit "$rc"

#!/bin/sh
set -e
if [ "${1:-}" = upgrade ]; then
    [ -x ./app.sh ] && ./app.sh disable || { echo "Unable to disable old session; reboot required." >&2; exit 1; }
    exit 0
fi
[ -x ./app.sh ] && ./app.sh disable || true
rm -f /mnt/us/documents/com.bd452.ksubstrate-enable.sh /mnt/us/documents/com.bd452.ksubstrate-disable.sh /mnt/us/documents/com.bd452.ksubstrate-reframe.sh
rm -f /var/local/kmc/ksubstrate-assets/wrapper.sh
# Third-party payloads under /var/local/kmc/tweaks deliberately survive.

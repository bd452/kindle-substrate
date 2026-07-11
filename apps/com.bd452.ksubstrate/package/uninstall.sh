#!/bin/sh
set -e
if [ "${1:-}" = upgrade ]; then
    [ -x ./app.sh ] && ./app.sh disable || { echo "Unable to disable old session; reboot required." >&2; exit 1; }
    exit 0
fi
[ -x ./app.sh ] && ./app.sh disable || true
rm -rf /mnt/us/extensions/KindleSubstrate
rm -f /mnt/us/documents/com.bd452.ksubstrate-enable.sh \
    /mnt/us/documents/com.bd452.ksubstrate-disable.sh \
    /mnt/us/documents/com.bd452.ksubstrate-status.sh \
    /mnt/us/documents/com.bd452.ksubstrate-reframe.sh \
    /mnt/us/documents/com.bd452.ksubstratedemo.sh
rm -f /var/local/ksubstrate/assets/wrapper.sh
rm -rf /var/local/ksubstrate/tweaks/com.bd452.ksubstrate.homeapps
# Third-party payloads under /var/local/ksubstrate/tweaks deliberately survive.

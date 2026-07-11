#!/bin/sh

set -e

cd "$(dirname "$0")" || exit 1

if [ -f /lib/ld-linux-armhf.so.3 ]; then
    PLAT=kindlehf
else
    PLAT=kindlepw2
fi

DAEMON="./bin/${PLAT}/ksubstrated"
if [ ! -x "$DAEMON" ]; then
    echo "ksubstrated binary not found for ${PLAT} at ${DAEMON}." >&2
    exit 1
fi

# Wrap Kindle UI roots by default so both `enable` and the default KPM launch
# (`toggle`) arm the session. Set KSUBSTRATE_SYSTEM_WRAP=0 to enable the daemon
# without touching framework processes (safe smoke test).
export KSUBSTRATE_SYSTEM_WRAP="${KSUBSTRATE_SYSTEM_WRAP:-1}"

case "${1:-toggle}" in
    enable)
        exec "$DAEMON" --enable
        ;;
    disable)
        exec "$DAEMON" --disable
        ;;
    status)
        exec "$DAEMON" --status
        ;;
    toggle)
        exec "$DAEMON" --toggle
        ;;
    *)
        echo "usage: $0 [enable|disable|status|toggle]" >&2
        exit 64
        ;;
esac

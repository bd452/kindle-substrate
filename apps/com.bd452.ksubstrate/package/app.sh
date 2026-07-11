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

case "${1:-toggle}" in
    enable)
        exec "$DAEMON" enable
        ;;
    disable)
        exec "$DAEMON" disable
        ;;
    status)
        exec "$DAEMON" status
        ;;
    reframe)
        exec "$DAEMON" reframe
        ;;
    reframe-if-active)
        exec "$DAEMON" reframe-if-active
        ;;
    post-package-change)
        exec "$DAEMON" post-package-change
        ;;
    prepare-target-package-change)
        shift
        exec "$DAEMON" prepare-target-package-change "$@"
        ;;
    finish-target-package-change)
        shift
        exec "$DAEMON" finish-target-package-change "$@"
        ;;
    toggle)
        exec "$DAEMON" toggle
        ;;
    *)
        echo "usage: $0 [enable|disable|status|reframe|reframe-if-active|post-package-change|prepare-target-package-change <package>|finish-target-package-change <package>|toggle]" >&2
        exit 64
        ;;
esac

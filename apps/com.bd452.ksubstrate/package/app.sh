#!/bin/sh

set -e

cd "$(dirname "$0")" || exit 1

if [ -f /lib/ld-linux-armhf.so.3 ]; then
    PLAT=kindlehf
else
    PLAT=kindlepw2
fi

DAEMON="./bin/${PLAT}/ksubstrated"

notify() {
    MESSAGE="$1"
    logger -t ksubstrate-home-demo "$MESSAGE" 2>/dev/null || true
    if command -v lipc-set-prop >/dev/null 2>&1; then
        lipc-set-prop com.lab126.system toaster "$MESSAGE" >/dev/null 2>&1 || true
    fi
    echo "$MESSAGE"
}

home_demo() {
    RESULT="${KSUBSTRATE_HOME_DEMO_RESULT:-/mnt/us/ksubstrate-home-demo-result.txt}"
    {
        echo "status=ok"
        echo "package=com.bd452.ksubstrate"
        echo "platform=$PLAT"
        echo "pid=$$"
        date '+timestamp=%Y-%m-%dT%H:%M:%S%z'
    } > "$RESULT"
    notify "Kindle Substrate Home launch succeeded"
    echo "details: $RESULT"
}

home_status() {
    if [ ! -x "$DAEMON" ]; then
        notify "Kindle Substrate status unavailable"
        return 1
    fi
    if STATUS=$("$DAEMON" status 2>&1); then
        notify "Kindle Substrate: $STATUS"
    else
        notify "Kindle Substrate status failed: $STATUS"
        return 1
    fi
}

require_daemon() {
    if [ ! -x "$DAEMON" ]; then
        echo "ksubstrated binary not found for ${PLAT} at ${DAEMON}." >&2
        exit 1
    fi
}

case "${1:-toggle}" in
    home-demo)
        home_demo
        ;;
    home-status)
        home_status
        ;;
    enable)
        require_daemon
        exec "$DAEMON" enable
        ;;
    disable)
        require_daemon
        exec "$DAEMON" disable
        ;;
    status)
        require_daemon
        exec "$DAEMON" status
        ;;
    reframe)
        require_daemon
        exec "$DAEMON" reframe
        ;;
    reframe-if-active)
        require_daemon
        exec "$DAEMON" reframe-if-active
        ;;
    post-package-change)
        require_daemon
        exec "$DAEMON" post-package-change
        ;;
    prepare-target-package-change)
        require_daemon
        shift
        exec "$DAEMON" prepare-target-package-change "$@"
        ;;
    finish-target-package-change)
        require_daemon
        shift
        exec "$DAEMON" finish-target-package-change "$@"
        ;;
    toggle)
        require_daemon
        exec "$DAEMON" toggle
        ;;
    *)
        echo "usage: $0 [home-demo|home-status|enable|disable|status|reframe|reframe-if-active|post-package-change|prepare-target-package-change <package>|finish-target-package-change <package>|toggle]" >&2
        exit 64
        ;;
esac

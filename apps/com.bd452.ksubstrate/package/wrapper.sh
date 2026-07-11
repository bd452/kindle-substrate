#!/bin/sh
# Installed once by KPM.  The daemon only bind-mounts this immutable asset.
PKG=/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate
if [ -f /lib/ld-linux-armhf.so.3 ]; then PLAT=kindlehf; else PLAT=kindlepw2; fi
exec "$PKG/bin/$PLAT/ksubstrate" wrapped-exec "$0" "$@"

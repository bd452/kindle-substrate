#!/bin/sh
set -e
PKG="/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate"
ROOT=/var/local/ksubstrate
ASSETS="$ROOT/assets"
RUNTIME="$ROOT/runtime"
TWEAKS="$ROOT/tweaks"

for d in "$RUNTIME/mounts" "$RUNTIME/state"; do
    mkdir -p "$d"
    if grep -F " $d " /proc/self/mountinfo >/dev/null 2>&1 || [ -n "$(find "$d" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
        echo "Runtime mount point is active or non-empty: $d; reboot required." >&2
        exit 1
    fi
done
# /var/local/kmc is KPM-owned and recursively immutable outside KPM's own
# installation transaction.  All Substrate-owned persistent files live here.
mkdir -p "$ASSETS" "$TWEAKS"
cp "$PKG/wrapper.sh" "$ASSETS/wrapper.sh"
chmod 0555 "$ASSETS/wrapper.sh"
EXTENSION=/mnt/us/extensions/KindleSubstrate
rm -rf "$EXTENSION"
mkdir -p "$EXTENSION"
cp -R "$PKG/kual/." "$EXTENSION/"
chmod 755 "$EXTENSION/bin/control.sh" "$EXTENSION/bin/demo.sh"

# First-party registry backing synthetic Home entries. It reads KPM manifests,
# but never creates app launchers in Documents or writes Kindle's catalog.
HOME_APPS="$TWEAKS/com.bd452.ksubstrate.homeapps"
HOME_STAGE="${HOME_APPS}.staging.$$"
rm -rf "$HOME_STAGE"
mkdir -p "$HOME_STAGE"
if [ -f /lib/ld-linux-armhf.so.3 ]; then PLAT=kindlehf; else PLAT=kindlepw2; fi
cp "$PKG/homeapps/$PLAT/tweak.so" "$HOME_STAGE/tweak.so"
cp "$PKG/homeapps/manifest.json" "$HOME_STAGE/manifest.json"
test -s "$HOME_STAGE/tweak.so" && test -s "$HOME_STAGE/manifest.json"
rm -rf "$HOME_APPS"
mv "$HOME_STAGE" "$HOME_APPS"

rm -f /mnt/us/documents/com.bd452.ksubstrate-enable.sh \
    /mnt/us/documents/com.bd452.ksubstrate-disable.sh \
    /mnt/us/documents/com.bd452.ksubstrate-status.sh \
    /mnt/us/documents/com.bd452.ksubstrate-reframe.sh \
    /mnt/us/documents/com.bd452.ksubstratedemo.sh
echo "Kindle Substrate installed (disabled). Open KUAL to control it."

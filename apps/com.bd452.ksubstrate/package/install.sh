#!/bin/sh
set -e
PKG="/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate"
ASSETS=/var/local/kmc/ksubstrate-assets
RUNTIME=/var/local/kmc/ksubstrate-runtime

# An upgrade must leave no old daemon owning mounts made from old package files.
for legacy in /var/local/kmc/ksubstrate/run/orig /var/local/kmc/ksubstrate/run/wrappers; do
    if grep -F " $legacy" /proc/self/mountinfo >/dev/null 2>&1; then
        echo "Legacy Substrate mount remains at $legacy; reboot required." >&2
        exit 1
    fi
done
for d in "$RUNTIME/mounts" "$RUNTIME/state"; do
    mkdir -p "$d"
    if grep -F " $d " /proc/self/mountinfo >/dev/null 2>&1 || [ -n "$(find "$d" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
        echo "Runtime mount point is active or non-empty: $d; reboot required." >&2
        exit 1
    fi
done
mkdir -p "$ASSETS" /var/local/kmc/tweaks
rm -rf /var/local/kmc/ksubstrate/run/orig /var/local/kmc/ksubstrate/run/wrappers
cp "$PKG/wrapper.sh" "$ASSETS/wrapper.sh"
chmod 0555 "$ASSETS/wrapper.sh"
for action in enable disable status reframe; do
    cat > "/mnt/us/documents/com.bd452.ksubstrate-$action.sh" <<EOF
#!/bin/sh
exec "$PKG/app.sh" $action
EOF
    chmod 755 "/mnt/us/documents/com.bd452.ksubstrate-$action.sh"
done
echo "Kindle Substrate installed (disabled)."

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
for action in enable disable status reframe; do
    cat > "/mnt/us/documents/com.bd452.ksubstrate-$action.sh" <<EOF
#!/bin/sh
exec "$PKG/app.sh" $action
EOF
    chmod 755 "/mnt/us/documents/com.bd452.ksubstrate-$action.sh"
done
echo "Kindle Substrate installed (disabled)."

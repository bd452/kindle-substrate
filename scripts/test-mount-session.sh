#!/usr/bin/env bash
# Privileged Linux integration test for the exact bind-mount safety pattern.
set -euo pipefail

if [[ "$(uname -s)" != Linux ]]; then
    echo "skipped: Linux mount namespaces are required"
    exit 0
fi

if [[ "${KSUB_MOUNT_NAMESPACE:-}" != 1 ]]; then
    if ! command -v unshare >/dev/null 2>&1; then
        echo "skipped: unshare is unavailable"
        exit 0
    fi
    exec unshare --user --map-root-user --mount --fork env KSUB_MOUNT_NAMESPACE=1 "$0"
fi

mount --make-rprivate /

work="$(mktemp -d)"
cleanup() {
    mountpoint -q "$work/original" && umount -l "$work/original" || true
    mountpoint -q "$work/mounts" && umount -l "$work/mounts" || true
    mountpoint -q "$work/state" && umount -l "$work/state" || true
    rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$work/bin" "$work/mounts" "$work/state"
printf '%s\n' '#!/bin/sh' 'printf "original:%s\n" "$*"' > "$work/bin/fixture"
chmod 755 "$work/bin/fixture"
original_hash="$(sha256sum "$work/bin/fixture" | awk '{print $1}')"

mount -t tmpfs -o nodev,nosuid,exec,mode=0700,size=4m tmpfs "$work/mounts"
mount -t tmpfs -o nodev,nosuid,noexec,mode=0700,size=4m tmpfs "$work/state"
mkdir -p "$work/mounts/original/bin"
grep -F " $work/mounts " /proc/self/mountinfo | grep -q nodev
grep -F " $work/state " /proc/self/mountinfo | grep -q noexec

alias="$work/mounts/original/bin/fixture"
: > "$alias"
mount --bind "$work/bin/fixture" "$alias"
mount -o remount,bind,ro "$alias"
printf '%s\n' '#!/bin/sh' "exec '$alias' \"\$@\"" > "$work/wrapper"
chmod 755 "$work/wrapper"
mount --bind "$work/wrapper" "$work/bin/fixture"
mount -o remount,bind,ro "$work/bin/fixture"

test "$("$work/bin/fixture" hello)" = "original:hello"
test "$(sha256sum "$alias" | awk '{print $1}')" = "$original_hash"
if (set -C; : > "$alias") 2>/dev/null; then
    echo "alias creation unexpectedly overwrote an existing target" >&2
    exit 1
fi

umount "$work/bin/fixture"
umount "$alias"
test "$(sha256sum "$work/bin/fixture" | awk '{print $1}')" = "$original_hash"
echo "mount-session integration passed"

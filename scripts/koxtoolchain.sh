#!/usr/bin/env bash
# Resolve KindleModding koxtoolchain compiler prefixes for FBInk builds.
set -euo pipefail

KOX_BASE="${KOXTOOLCHAIN_ROOT:-$HOME/x-tools}"
export KOX_BASE

kox_prefix() {
    case "$1" in
        kindlehf) echo "arm-kindlehf-linux-gnueabihf" ;;
        kindlepw2) echo "arm-kindlepw2-linux-gnueabi" ;;
        *)
            echo "unknown platform: $1" >&2
            return 1
            ;;
    esac
}

kox_gcc() {
    local prefix
    prefix="$(kox_prefix "$1")"
    echo "$KOX_BASE/x-tools/${prefix}/bin/${prefix}-gcc"
}

# Rust target triple for a platform (used by the Rust-based packages).
kox_rust_target() {
    case "$1" in
        kindlehf) echo "armv7-unknown-linux-gnueabihf" ;;
        kindlepw2) echo "armv7-unknown-linux-gnueabi" ;;
        *)
            echo "unknown platform: $1" >&2
            return 1
            ;;
    esac
}

# Cargo env var used to force the linker for a Rust target. This avoids relying
# on where Cargo starts searching for `.cargo/config.toml`.
kox_rust_linker_env() {
    case "$1" in
        kindlehf) echo "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER" ;;
        kindlepw2) echo "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_LINKER" ;;
        *)
            echo "unknown platform: $1" >&2
            return 1
            ;;
    esac
}

# Directory holding a platform's cross-compiler binaries (prepend to PATH so
# cargo's configured linker and FBInk's Makefile find the toolchain).
kox_tool_bin() {
    echo "$KOX_BASE/x-tools/$(kox_prefix "$1")/bin"
}

require_kox() {
    local platform missing=0
    for platform in kindlehf kindlepw2; do
        if [[ ! -x "$(kox_gcc "$platform")" ]]; then
            echo "error: missing koxtoolchain for $platform" >&2
            echo "  expected: $(kox_gcc "$platform")" >&2
            missing=1
        fi
    done
    if [[ "$missing" -ne 0 ]]; then
        echo "Install with: ./scripts/setup-koxtoolchain.sh" >&2
        return 1
    fi
}

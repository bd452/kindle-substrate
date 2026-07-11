#!/usr/bin/env bash
# Cross-compile and pack the Kindle Substrate runtime.
set -euo pipefail

APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$APP_ROOT/../.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"
DOBBY_SRC="$APP_ROOT/vendor/Dobby"

# shellcheck source=../../scripts/koxtoolchain.sh
source "$REPO_ROOT/scripts/koxtoolchain.sh"
require_kox

if [[ ! -f "$DOBBY_SRC/include/dobby.h" ]]; then
    echo "error: Dobby source missing at $DOBBY_SRC" >&2
    echo "Run: git submodule update --init --recursive" >&2
    exit 1
fi

# Upstream master deleted core/arch/Cpu.h without updating includes. We pin the
# submodule to a pre-refactor commit that still builds; refuse broken checkouts.
if [[ ! -f "$DOBBY_SRC/source/core/arch/Cpu.h" ]]; then
    echo "error: Dobby checkout at $DOBBY_SRC is missing source/core/arch/Cpu.h" >&2
    echo "This usually means the submodule floated to a broken upstream commit." >&2
    echo "Reset to the pinned commit: git -C $DOBBY_SRC checkout e9fe7fbecae47a2287e761080f8b1133cc22e8fa" >&2
    exit 1
fi

mkdir -p "$APP_ROOT/package/lib/kindlehf" "$APP_ROOT/package/lib/kindlepw2" \
    "$APP_ROOT/package/bin/kindlehf" "$APP_ROOT/package/bin/kindlepw2" \
    "$APP_ROOT/package/include" "$APP_ROOT/package/tweaks" \
    "$APP_ROOT/package/diagnostics/com.bd452.ksubstrateprobe"

build_platform() {
    local platform=$1
    local cross_tc tool_bin rust_target linker_env target_dir release_dir lib_dir
    cross_tc="$(kox_prefix "$platform")"
    tool_bin="$(kox_tool_bin "$platform")"
    rust_target="$(kox_rust_target "$platform")"
    linker_env="$(kox_rust_linker_env "$platform")"
    lib_dir="$APP_ROOT/package/lib/${platform}"

    echo "==> Building Kindle Substrate runtime for $platform ($rust_target)"
    env CROSS_TC="$cross_tc" PATH="$tool_bin:$PATH" \
        "$linker_env=$tool_bin/${cross_tc}-gcc" \
        cargo build --manifest-path "$RUST_DIR/Cargo.toml" --release --target "$rust_target" \
        -p ksubstrate -p ksubstrate-bootstrap -p ksubstrate-cli -p ksubstrated

    target_dir="${CARGO_TARGET_DIR:-$RUST_DIR/target}"
    release_dir="$target_dir/${rust_target}/release"

    install -m 644 "$release_dir/libksubstrate.so" \
        "$lib_dir/libksubstrate.so"
    install -m 644 "$release_dir/libksubstrate_bootstrap.so" \
        "$lib_dir/libksubstrate-bootstrap.so"
    install -m 755 "$release_dir/ksubstrate" \
        "$APP_ROOT/package/bin/${platform}/ksubstrate"
    install -m 755 "$release_dir/ksubstrated" \
        "$APP_ROOT/package/bin/${platform}/ksubstrated"

    # Probe needs the just-staged runtime lib (KSUBSTRATE_LIB_DIR → dynamic link).
    echo "==> Building inheritance probe for $platform"
    env CROSS_TC="$cross_tc" PATH="$tool_bin:$PATH" KSUBSTRATE_LIB_DIR="$lib_dir" \
        "$linker_env=$tool_bin/${cross_tc}-gcc" \
        cargo build --manifest-path "$RUST_DIR/Cargo.toml" --release --target "$rust_target" \
        -p ksubstrate-probe-tweak

    # Opt-in diagnostic (A§6.4): * filter would match every process, so it ships
    # under diagnostics/ — copy into the live tweaks dir only when probing.
    install -m 644 "$release_dir/libksubstrate_probe_tweak.so" \
        "$APP_ROOT/package/diagnostics/com.bd452.ksubstrateprobe/tweak.so"
}

build_platform kindlehf
build_platform kindlepw2

cp "$RUST_DIR/ksubstrate/include/ksubstrate.h" "$APP_ROOT/package/include/ksubstrate.h"
install -m 644 "$RUST_DIR/ksubstrate-probe-tweak/tweak.ksfilter" \
    "$APP_ROOT/package/diagnostics/com.bd452.ksubstrateprobe/tweak.ksfilter"
install -m 644 "$RUST_DIR/ksubstrate-probe-tweak/manifest.json" \
    "$APP_ROOT/package/diagnostics/com.bd452.ksubstrateprobe/manifest.json"

"$REPO_ROOT/scripts/pack-app.sh" "$APP_ROOT"

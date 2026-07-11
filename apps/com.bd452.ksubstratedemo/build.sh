#!/usr/bin/env bash
# Cross-compile and pack the Kindle Substrate demo target and sample tweak.
set -euo pipefail

APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$APP_ROOT/../.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"
RUNTIME_APP="$REPO_ROOT/apps/com.bd452.ksubstrate"

# shellcheck source=../../scripts/koxtoolchain.sh
source "$REPO_ROOT/scripts/koxtoolchain.sh"
require_kox

mkdir -p "$APP_ROOT/package/bin/kindlehf" "$APP_ROOT/package/bin/kindlepw2" \
    "$APP_ROOT/package/tweaks/com.bd452.ksubstratedemo"

build_platform() {
    local platform=$1
    local cross_tc tool_bin rust_target linker_env target_dir release_dir runtime_lib_dir
    cross_tc="$(kox_prefix "$platform")"
    tool_bin="$(kox_tool_bin "$platform")"
    rust_target="$(kox_rust_target "$platform")"
    linker_env="$(kox_rust_linker_env "$platform")"
    runtime_lib_dir="$RUNTIME_APP/package/lib/${platform}"

    if [[ ! -f "$runtime_lib_dir/libksubstrate.so" ]]; then
        echo "error: missing runtime library at $runtime_lib_dir/libksubstrate.so" >&2
        echo "Build apps/com.bd452.ksubstrate first." >&2
        exit 1
    fi

    echo "==> Building Kindle Substrate demo for $platform ($rust_target)"
    env CROSS_TC="$cross_tc" PATH="$tool_bin:$PATH" KSUBSTRATE_LIB_DIR="$runtime_lib_dir" \
        "$linker_env=$tool_bin/${cross_tc}-gcc" \
        cargo build --manifest-path "$RUST_DIR/Cargo.toml" --release --target "$rust_target" \
        -p ksubstrate-demo-target -p ksubstrate-sample-tweak

    target_dir="${CARGO_TARGET_DIR:-$RUST_DIR/target}"
    release_dir="$target_dir/${rust_target}/release"

    install -m 755 "$release_dir/ksubstrate-demo-target" \
        "$APP_ROOT/package/bin/${platform}/ksubstrate-demo-target"
    install -m 644 "$release_dir/libksubstrate_sample_tweak.so" \
        "$APP_ROOT/package/tweaks/com.bd452.ksubstratedemo/tweak.so"
}

build_platform kindlehf
build_platform kindlepw2

"$REPO_ROOT/scripts/pack-app.sh" "$APP_ROOT"

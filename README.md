# Kindle Substrate

Session-scoped native hooking runtime and developer toolchain for jailbroken
Kindles. The project builds libksubstrate, its preload bootstrap and session
daemon, host-side tweak tooling, and KPM runtime/demo packages.

## Layout

- `rust/ksubstrate*` and `rust/ksubstrated` - on-device runtime
- `rust/ksub`, `rust/ksub-logos`, `rust/ksub-syms` - host tooling
- `apps/com.bd452.ksubstrate` - runtime KPM package
- `apps/com.bd452.ksubstratedemo` - self-contained validation package
- `apps/com.bd452.ksubstrate/vendor/Dobby` - pinned inline-hook engine
- `docs/` - architecture and implementation audit
- `analysis/` - firmware acquisition and symbol-analysis work

## Build

Clone recursively so the pinned Dobby source is present:

    git clone --recurse-submodules https://github.com/bd452/kindle-substrate.git
    cd kindle-substrate
    ./scripts/build-in-container.sh

Host tests:

    cargo test --manifest-path rust/Cargo.toml --workspace

The package build writes .kpkg artifacts under each app's dist directory.
kinstaller-repo consumes this repository as a submodule and publishes those
artifacts in its package index.

## Status

The implementation is experimental. Cross-build and host tests are available,
but inline hooking, GOT rewriting, framework wrappers, and recovery still
require validation across the supported Kindle firmware/device matrix.

The implementation was extracted from bd452/kinstaller-repo at commit
b4a1c65.

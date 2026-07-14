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
- `docs/` - architecture, strategy, and implementation audit
- `analysis/` - firmware acquisition and symbol-analysis work

## Build

Clone recursively so the pinned Dobby source is present:

    git clone --recurse-submodules https://github.com/bd452/kindle-substrate.git
    cd kindle-substrate
    ./scripts/build-in-container.sh

Host tests:

    cargo test --manifest-path rust/Cargo.toml --workspace

The local Dockerfile and CI inherit the pinned
`ghcr.io/bd452/kindle-kpm-build:v0.1.0` environment from
`kindle-kpm-devkit`. This repository retains only the Substrate-specific build
and packaging commands.

The package build writes `.kpkg` artifacts under each app's `dist` directory.
Packaging is provided by Kindle KPM devkit 0.1.0, resolved through `KPM_DEV`,
`PATH`, or a sibling `../kindle-kpm-devkit` checkout. This repository is the
source of truth for the runtime, toolchain, and its released package artifacts.
The KPM registry consumes the release descriptor rather than rebuilding this
repository as a source submodule.

The supported runtime publishing and third-party tweak authoring workflows are
documented in [`docs/build-and-publish.md`](docs/build-and-publish.md).

## Hook contract

Hooks are append-only for the life of a target process. Repeated registrations
of an inline target or a specific-image import form one chain: the most recently
loaded tweak is outermost, and its `original` continuation advances exactly one
step toward the true implementation. Hook calls make no registry lock or
signature-aware dispatch. Runtime removal is deliberately unsupported;
`kh_unhook_function` remains only as a deprecated ABI symbol and returns
`KH_ERROR_UNSUPPORTED`.

Tweak manifests are validated before any library is loaded. Dependencies load
before dependents; otherwise enabled tweaks are sorted by ascending `order` and
then `id`, making constructor registration deterministic.

## Status

The implementation is experimental. Cross-build and host tests are available,
but inline hooking, GOT rewriting, framework wrappers, and recovery still
require validation across the supported Kindle firmware/device matrix.

Forward plan (milestones M0–M3, layer ownership, exit gates):
[`docs/architecture-strategy.md`](docs/architecture-strategy.md).

The implementation was extracted from `bd452/kinstaller-repo` at commit
`b4a1c65`.

# Building, Authoring, and Publishing

This document defines the two supported workflows: publishing the Kindle
Substrate runtime through the KPM repository and authoring a separate KPM tweak
that depends on that runtime.

## Build and publish the runtime packages

Kindle Substrate builds two KPM artifacts in dependency order:

1. `com.bd452.ksubstrate` provides the runtime, bootstrap, daemon, CLI, and SDK.
2. `com.bd452.ksubstratedemo` depends on `com.bd452.ksubstrate >= 0.1.5` and
   validates inline and import hooks in a self-contained target.

From a recursive clone of this repository, run:

```sh
./scripts/build-in-container.sh
```

The artifacts are written to:

```text
apps/com.bd452.ksubstrate/dist/*.kpkg
apps/com.bd452.ksubstratedemo/dist/*.kpkg
```

`kinstaller-repo` is the distribution owner. It pins this repository at
`components/kindle-substrate`, builds the runtime before the demo, and adds both
artifacts to its `manifest.json` and `packages/` tree. From a recursive clone of
that repository, the intended production command is:

```sh
./scripts/build-in-container.sh
```

The resulting repository manifest must retain the demo dependency on
`com.bd452.ksubstrate`. Publishing is performed by the downstream repository's
GitHub Pages workflow; this repository does not publish a package index.

## Create and build a tweak package

Build the runtime once so its per-platform SDK libraries are staged, then build
the host CLI:

```sh
./scripts/build-in-container.sh apps/com.bd452.ksubstrate/build.sh
cargo build --manifest-path rust/Cargo.toml -p ksub --release
./scripts/build-in-container.sh bash -lc \
  'cargo build --manifest-path rust/Cargo.toml -p ksub --release'
```

Create a project outside this repository and enter it:

```sh
./rust/target/release/ksub new tweak ../my-tweak
cd my-tweak
```

Edit `package/manifest.json` before publishing: replace the example package ID,
name, author, and description. Keep the structured dependency on
`com.bd452.ksubstrate` and update its minimum version when the runtime ABI
requirement changes.

Run cross-builds inside the Kindle Substrate container because the KindleModding
cross toolchains are Linux amd64 binaries. Mount the new project and invoke the
CLI built from this SDK checkout:

```sh
docker run --rm --platform linux/amd64 \
  -v /absolute/path/to/kindle-substrate:/sdk \
  -v "$PWD":/tweak \
  -e KOXTOOLCHAIN_ROOT=/opt/x-tools \
  -e KSUBSTRATE_SDK_ROOT=/sdk \
  -w /tweak kindle-substrate-build:latest \
  /sdk/rust/target-kindle/release/ksub build --platform kindlehf

docker run --rm --platform linux/amd64 \
  -v /absolute/path/to/kindle-substrate:/sdk \
  -v "$PWD":/tweak \
  -e KOXTOOLCHAIN_ROOT=/opt/x-tools \
  -e KSUBSTRATE_SDK_ROOT=/sdk \
  -w /tweak kindle-substrate-build:latest \
  /sdk/rust/target-kindle/release/ksub build --platform kindlepw2

/absolute/path/to/kindle-substrate/rust/target/release/ksub package
```

`ksub build` links against the matching runtime SDK library and stages
`package/lib/<platform>/tweak.so`. `ksub package` refuses to package until both
ABIs exist, then writes `dist/*.kpkg`.

The generated KPM package has an explicit runtime dependency. On install, it
selects the device ABI and copies the library and manifest to
`/var/local/ksubstrate/tweaks/<package-id>/`. This is deliberately outside
KPM's immutable `/var/local/kmc` namespace. Install stages payloads in a hidden
same-filesystem directory and atomically renames them into the visible registry;
uninstall first renames the active directory away. Both operations defer
`post-package-change`, which synchronously reconciles an active session and
never enables a disabled runtime. Hidden staging and retired
directories are excluded from the runtime registry. On uninstall, it removes that registered
tweak. Disable and re-enable Kindle Substrate after installing, upgrading, or
removing a tweak so the UI session is restarted with the new set.

## Device validation

Install `com.bd452.ksubstrate` and then `com.bd452.ksubstratedemo` through KPM.
The dependency resolver must install or require the runtime first. Run the demo
launcher and capture its output and `/tmp/ksubstrate-demo-result`; the expected
hooked value is `42`, with `tweaks.log` showing both the inline hook and the
`write` import hook.

For framework-session validation, first create `/mnt/us/DISABLE_KSUBSTRATE` and
confirm the sentinel prevents loading. Remove it, use the Enable Tweaks launcher,
check `app.sh status`, then use Disable Tweaks and confirm the stock UI returns.
Hard reboot must also return to an unhooked session because all wrappers are
volatile bind mounts.

Record model, firmware, ABI, package SHA-256 values, logs, and recovery outcomes
under `analysis/evidence/<device>-<firmware>/`. Passing host tests and cross-builds
does not replace this physical-device evidence.

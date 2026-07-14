# Building, Authoring, and Publishing

This document defines the two supported workflows: releasing Kindle Substrate
artifacts for consumption by a KPM repository and authoring a separate KPM
tweak that depends on that runtime.

## Build and publish the runtime packages

Kindle Substrate builds two KPM artifacts in dependency order:

1. `com.bd452.ksubstrate` provides the runtime, bootstrap, daemon, CLI, and SDK.
2. `com.bd452.ksubstratedemo` depends on `com.bd452.ksubstrate >= 0.1.6` and
   validates inline and import hooks in a self-contained target.

From a recursive clone of this repository, run:

```sh
./scripts/build-in-container.sh
```

The repository pins Kindle KPM devkit 0.1.0 in `.kpm-devkit-version`. The
`scripts/kpm-dev` resolver uses `KPM_DEV`, a `kpm-dev` executable on `PATH`, or
a sibling `../kindle-kpm-devkit/bin/kpm-dev`, in that order. The compatibility
`scripts/pack-app.sh` wrapper stages each package and delegates manifest
validation, deterministic packing, and archive verification to that devkit.

The verified artifacts are written to:

```text
apps/com.bd452.ksubstrate/dist/*.kpkg
apps/com.bd452.ksubstratedemo/dist/*.kpkg
```

This source repository owns those builds and their immutable release artifacts.
For tags beginning with `v`, GitHub Actions publishes both `.kpkg` files and one
`release-metadata.json` descriptor. The descriptor records the source commit
and tag plus each artifact's URL, SHA-256, size, manifest fields, and dependency
metadata. Runtime and demo intentionally share one descriptor because they are
versioned and validated as a coupled package set.

To generate the same descriptor locally after a package build:

```sh
./scripts/generate-release-metadata.sh \
  --base-url https://github.com/bd452/kindle-substrate/releases/download/v0.1.6 \
  --repository bd452/kindle-substrate \
  --commit "$(git rev-parse HEAD)" \
  --tag v0.1.6 \
  --output dist/release-metadata.json
```

The downstream KPM registry pins and verifies this descriptor, then incorporates
the two package records into its catalog and published index. It owns catalog
composition and index hosting; it does not compile Kindle Substrate or vendor
this repository as a source submodule. The generated catalog must retain the
demo dependency on `com.bd452.ksubstrate`.

A successful host test, cross-build, devkit verification, or GitHub release is
build evidence only. It does not prove the runtime safe on a physical Kindle;
the device validation requirements below remain release-quality evidence gates.

## Fileless Home apps

Any KPM package can opt into one or more Kindle Home entries without placing
launchers in `/mnt/us/documents`. Add a `kindle_home` array to its package
manifest. Every entry has an ID unique within that package:

```json
{
  "id": "com.example.chess",
  "kindle_home": [
    {
      "id": "play",
      "name": "Chess",
      "subtitle": "Board game",
      "icon": "assets/cover.pgm",
      "executable": "app.sh",
      "arguments": ["--quick"]
    },
    {
      "id": "puzzles",
      "name": "Chess Puzzles",
      "icon": "assets/puzzles.pgm",
      "executable": "app.sh",
      "arguments": ["--puzzles"]
    }
  ]
}
```

All declared paths must be package-relative regular files or directories.
No path component may be a symlink. `executable` must be executable;
`{platform}` may appear in a path and expands to `kindlehf` or `kindlepw2`.
Invalid declarations are omitted individually, so one bad entry does not hide
valid siblings. Duplicate or invalid app IDs are omitted. The runtime assigns
each entry a stable
`kpm-app://<package-id>/<app-id>` ID, reads the icon from its package location,
and starts the executable directly with the declared arguments—never through a
shell.

The runtime package installs the `com.bd452.ksubstrate.homeapps` registry tweak
into the persistent Substrate registry. It exposes a C bridge for a
firmware-specific Home adapter to enumerate, render, and activate those
synthetic entries. The bridge deliberately does not create files in Documents
or modify Kindle's content database. Package lifecycle hooks refresh an active
session automatically; the KUAL **Reframe Active Session** action is the manual
recovery path. Until a firmware-specific Home adapter is installed, KUAL remains
the supported way to launch the package.

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

Installing `com.bd452.ksubstrate` through KPM also installs two fileless Home
entries: **Kindle Substrate Test** and **Kindle Substrate Status**. Neither has a
matching file under `/mnt/us/documents`. Test dispatches `app.sh home-demo`,
shows a success notification, and writes diagnostic details to
`/mnt/us/ksubstrate-home-demo-result.txt`. Status dispatches `app.sh home-status`
and shows the current runtime state.

For generic target-hook validation, install `com.bd452.ksubstratedemo` through
KPM. The dependency resolver must install or require the runtime first. Run the
demo launcher and capture its output and `/mnt/us/ksubstrate-demo-result.txt`;
the expected hooked value is `42`, with `tweaks.log` showing both the inline
hook and the `write` import hook.

For framework-session validation, first create `/mnt/us/DISABLE_KSUBSTRATE` and
confirm the sentinel prevents loading. Remove it, use the Enable Tweaks launcher,
check `app.sh status`, then use Disable Tweaks and confirm the stock UI returns.
Hard reboot must also return to an unhooked session because all wrappers are
volatile bind mounts.

Record model, firmware, ABI, package SHA-256 values, logs, and recovery outcomes
under `analysis/evidence/<device>-<firmware>/`. Passing host tests and cross-builds
does not replace this physical-device evidence.

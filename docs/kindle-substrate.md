# Kindle Substrate Architecture

This repository carries the Kindle Substrate implementation: the runtime engine
ABI, bootstrap, daemon, device CLI, demo target, sample tweak, host toolchain,
and KPM package sources. `bd452/kinstaller-repo` consumes this project as a
submodule and remains the package distribution surface.

For the forward roadmap (milestones, decisions, exit gates), see
[architecture-strategy.md](architecture-strategy.md). This document is the
as-built contract.

## Repository Boundary

```text
kindle-substrate/
  rust/ksubstrate/             # libksubstrate.so + ksubstrate.h
  rust/ksubstrate-bootstrap/   # LD_PRELOAD loader
  rust/ksubstrated/            # session daemon
  rust/ksubstrate-cli/         # device helper CLI
  rust/ksubstrate-demo-target/ # self-contained demo process
  rust/ksubstrate-sample-tweak/# self-contained demo tweak
  rust/ksub/                   # host CLI: new/build/deploy/package/pull/analyze
  rust/ksub-logos/             # Logos-style preprocessor
  rust/ksub-syms/              # symbol DB compiler helpers
  apps/com.bd452.ksubstrate/   # KPM package source for runtime artifacts
  apps/com.bd452.ksubstratedemo/
  analysis/                    # firmware acquisition and symbol work
  docs/                        # architecture, strategy, and implementation audit
```

The Rust workspace and KPM package sources are independently buildable here.
Kinstaller consumes the resulting `.kpkg` artifacts through its pinned
`components/kindle-substrate` submodule.

## Published Packages

### `com.bd452.ksubstrate`

Library/control package. It owns the on-device session runtime and public tweak
ABI.

```text
package/
  manifest.json
  install.sh
  uninstall.sh
  app.sh
  launch.sh
  lib/
    kindlehf/
      libksubstrate.so
      libksubstrate-bootstrap.so
    kindlepw2/
      libksubstrate.so
      libksubstrate-bootstrap.so
  bin/
    kindlehf/
      ksubstrated
      ksubstrate
    kindlepw2/
      ksubstrated
      ksubstrate
  include/
    ksubstrate.h
  tweaks/
```

`install.sh` installs a KUAL extension with enable, disable, reframe, status,
and demo actions. It does not place executable shell scripts in Documents,
where Kindle treats them as books rather than launchers.
`app.sh` is the stable package-local control entry point:

```text
app.sh enable    # start ksubstrated, install session wrappers, restart UI
app.sh disable   # remove wrappers, restart stock UI, exit daemon
app.sh status    # report daemon/session state
app.sh toggle    # default for KPM launch
```

The package is built by `scripts/build-repo.sh`. Its local `build.sh`
cross-compiles the runtime crates for `kindlehf` and `kindlepw2`, stages the
artifacts, and then uses the repository's normal `scripts/pack-app.sh` path.

### `com.bd452.ksubstratedemo`

Demo package. It depends on `com.bd452.ksubstrate` and proves the loading path
without touching Kindle framework processes.

```text
package/
  manifest.json
  install.sh
  uninstall.sh
  app.sh
  launch.sh
  bin/
    kindlehf/ksubstrate-demo-target
    kindlepw2/ksubstrate-demo-target
  tweaks/
    com.bd452.ksubstratedemo/
      tweak.so
      manifest.json
```

The target binary exports a `compute` symbol that returns an unhooked value and
calls it through a runtime-resolved pointer. The package launches the target
through the installed `ksubstrate` CLI, which preloads the bootstrap; the
bootstrap `dlopen`s the sample tweak, and the tweak installs a **real inline
hook** on `compute` via `kh_hook_function` before `main` runs. The target then
prints and writes the hooked value. This exercises the actual hooking engine
(not a cooperative dispatch table) while staying self-contained and recoverable.

The demo manifest explicitly names its KPM executable target. The CLI creates
a one-shot launch plan for that target, so the demo remains self-contained
without relying on process-name matching.

## Runtime Model

Kindle Substrate is session-scoped. A hard reboot always returns to stock
behavior because bind mounts and state tmpfs are the hooked session.

```text
CLEAN boot
  user opens Enable Tweaks launcher
  ksubstrated controller installs a complete target set and exits
  controller soft-restarts required profiled framework roots
  wrapped processes exec with LD_PRELOAD=libksubstrate-bootstrap.so
  bootstrap verifies target identity and loads the committed plan entries
HOOKED session
  user opens Disable Tweaks launcher or device reboots
  controller removes wrappers and restarts framework stock
CLEAN again
```

Design invariants:

- Hard reboot is a clean boot.
- No userspace daemon owns the session; verified mounts plus a committed tmpfs
  session plan define active state.
- Enable means restart the UI into a known home-screen state, not live PID
  injection. Both the `enable` launcher and the default KPM launch (`toggle`)
  install session wrappers.
- Hooks are installed at process exec by `LD_PRELOAD`, not by global
  `/etc/ld.so.preload`.
- Targets are explicit manifest-v2 built-ins or opted-in KPM executable paths.
- `powerd`, `sshd`, `dbus`, OTA, storage, and networking core remain blacklisted
  from default wrapping.

## Device Filesystem Contract

The runtime package treats its KPM package directory as the immutable asset
anchor. Runtime state is strictly ephemeral: after KPM has installed the
wrapper and tweak payloads, the daemon writes only beneath two independently
verified tmpfs mounts. It never opens a system executable or its original alias
for writing.

```text
/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/
  app.sh
  bin/<platform>/ksubstrated
  bin/<platform>/ksubstrate
  lib/<platform>/libksubstrate.so
  lib/<platform>/libksubstrate-bootstrap.so
  include/ksubstrate.h
  wrapper.sh               # generic immutable wrapper source
  diagnostics/            # opt-in tools (e.g. inheritance probe); not auto-loaded
```

Persistent Substrate data lives outside KPM's immutable `/var/local/kmc`
namespace. Tweak payloads live at `/var/local/ksubstrate/tweaks/<id>/`; they are
atomically installed only by KPM lifecycle hooks and are read-only to runtime
code. Session state lives on two daemon-owned tmpfs mounts:

```text
/var/local/ksubstrate/runtime/
  mounts/               # tmpfs (exec): original/usr/bin/pillowd, etc.
  state/                # distinct tmpfs (noexec): control.sock, journal, logs
```

`app.sh reframe` reconciles the installed tweak set and restarts the Kindle
framework; `reframe-if-active` is a no-op when disabled, so package operations
never enable a session. KPM hooks use the deferred variant internally, allowing
the control request to disconnect before reframe begins.

The emergency USB sentinel is:

```text
/mnt/us/DISABLE_KSUBSTRATE
```

The bootstrap checks it first and no-ops if present.

## Runtime Components

`libksubstrate.so` exposes the C ABI used by tweaks:

```c
int kh_hook_function(void *target, void *replacement, void **original);
int kh_hook_function_checked(
    void *target,
    void *replacement,
    void **original,
    const void *expected_prologue,
    size_t expected_len
);
int kh_unhook_function(void *target);
int kh_hook_import(const char *image, const char *symbol, void *replacement, void **original);
void *kh_find_symbol(const char *image, const char *name);
void *kh_resolve_rva(const char *image, size_t rva);

#define MSHookFunction kh_hook_function
#define MSFindSymbol   kh_find_symbol
```

The engine prefers PLT/GOT hooks for imported call surfaces (`kh_hook_import`):
it parses the loaded image's ELF dynamic table, finds `R_*_JUMP_SLOT`
relocations for the named symbol, and rewrites the GOT entry. No inline
prologue patch is involved. Prefer this over inline hooks whenever the target
is an import (libc, liblipc, etc.).

Inline hooks (`kh_hook_function`) use the vendored [Dobby](https://github.com/jmpews/Dobby)
engine on Kindle ARM targets. Dobby relocates ARM/Thumb-2 prologues, allocates
trampolines, and handles branch veneers and cache flushing. Host builds use a
mock backend so the ABI and registry stay testable without the cross toolchain.

`kh_hook_function_checked` performs the symbol-DB safety check before patching:
the current target bytes must match the expected prologue supplied by the caller.
This is the safer entrypoint for firmware-private RVAs resolved via
`kh_resolve_rva` (module load base from `/proc/self/maps` + recorded RVA). The
caller must describe at least an 8-byte window (`expected_len >= 8`) so the
entire region under consideration is verified before Dobby patches.

`libksubstrate-bootstrap.so` is loaded by the dynamic linker. Its constructor:

1. Checks `/mnt/us/DISABLE_KSUBSTRATE`.
2. Verifies the wrapper-supplied target and original-alias identity.
3. Reads the committed session or one-shot launch plan.
4. `dlopen`s only the plan-listed tweak libraries.
5. Fails closed per tweak without aborting the host process.

`ksubstrated` is a short-lived transactional controller:

1. Validates manifest-v2 registry entries, resolves explicit profile/KPM
   targets, and rejects recovery-critical targets.
2. Wraps each root with a **volatile bind mount**: the real binary is bind-mounted
   to a stable path under the session dir, then the original path is shadowed by
   a wrapper that re-execs under `LD_PRELOAD`. Nothing on the rootfs is modified;
   a reboot drops the mounts (A§14.1). The wrapped set is recorded to
   the tmpfs journal. If any wrap fails, every root wrapped so far is rolled
   back so a UI root is never left missing.
3. Publishes a committed session plan and restarts required profiled framework
   classes after mount installation.
4. On disable, reframe failure, or recovery, unmounts wrappers and restarts
   stock UI, restoring
   exactly the roots in the tmpfs journal. A reboot is still clean even if the
   manifest is missing, because the mounts are volatile.

## Toolchain Relationship

The host-side `ksub` toolchain lives in the Rust workspace and produces
artifacts that match the package contract above.

See [build-and-publish.md](build-and-publish.md) for the reproducible downstream
KPM publishing flow and the standalone third-party tweak authoring flow.

Command status (this repo is an MVP; not every command is a finished pipeline):

- `ksub new tweak` — implemented (scaffolds a buildable KPM dependency consumer).
- `ksub new library|tool` — placeholder README scaffolds only.
- `ksub build --platform kindlehf|kindlepw2` — implemented for scaffolded tweaks
  (cross-compiles and stages the selected ABI).
- `ksub package` — implemented for scaffolded tweaks and this repository's two
  built-in packages.
- `ksub deploy [--dest <path>]` — copies built `.kpkg` artifacts to a local or
  USB-mounted destination; for SSH transports, copy the printed artifacts with
  your own tooling.
- `ksub sym lookup|header` — implemented (parses the YAML symbol DB, emits a C
  header).
- `ksub pull`, `ksub analyze`, `ksub sym propose|promote` — **scaffolding.** They
  create working directories / template YAML and document the manual steps; they
  do not yet extract symbols from binaries.
- `ksub-logos` — **experimental.** Expands `KSYM`, `%hookf`, `%orig`, and
  `%ctor/%init`. Single-line `%hookf` signatures only; not a full Logos
  preprocessor.

The analysis pipeline is intended to compile versioned symbol databases from
exported symbols, imports, strings, Ghidra output, cross-version fingerprints, AI
proposals, and human-promoted overrides. That extraction is not yet built — the
symbol DB is authored by hand today. Runtime inline hooks must still verify
prologue signatures because firmware-specific RVAs can drift.

## Build Plan For This Repo

1. Build `apps/com.bd452.ksubstrate` first; it stages the runtime libraries,
   daemon, CLI, and header.
2. Build `apps/com.bd452.ksubstratedemo` second; it links the demo target and
   sample tweak against the staged runtime library.
3. Run `./build.sh` or `scripts/build-in-container.sh` to produce `.kpkg`
   artifacts under each app's `dist/` directory.
4. Downstream package indexes (for example `kinstaller-repo`) pin a commit of
   this repository and publish those artifacts; this repo does not own the
   index `packages/` tree or `manifest.json`.

## Recovery Ladder

1. Disable launcher: daemon unmounts wrappers and restarts stock UI, restoring
   exactly the roots recorded in the tmpfs journal.
2. USB sentinel: create `/mnt/us/DISABLE_KSUBSTRATE`, then reboot. The bootstrap
   checks it first and loads no tweaks.
3. Crash-loop / health guard: three UI-process deaths (or monitor restarts)
   within 120s return the session to stock automatically instead of re-arming.
4. Partial-wrap rollback: if wrapping fails midway, the daemon unmounts every
   root it already wrapped before giving up, so no UI root is left missing.
5. Hard reboot: default clean state — bind mounts are volatile, and no
   persistent global preload is armed.

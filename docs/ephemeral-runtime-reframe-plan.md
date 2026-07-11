# Ephemeral Runtime and Reframe Implementation Plan

> Superseded by [explicit-target-controller-plan.md](explicit-target-controller-plan.md).
> The original socket-daemon and filter-based portions are retained here only as
> historical design context.

## Purpose

Replace the current runtime-generated wrapper and persistent session-state model
with a strictly ephemeral session architecture. KPM installation owns persistent
package and tweak files. The running Kindle Substrate daemon owns only kernel
mounts and files created on verified tmpfs instances.

This plan also introduces `reframe`, the Kindle-native equivalent of an iOS
respring: reconcile the installed tweak set and restart the Kindle framework
without rebooting or enabling a disabled session.

## Non-Negotiable Invariants

1. Kindle Substrate never opens an original system executable for writing.
2. Kindle Substrate never opens an alias bound to an original executable for
   writing.
3. After KPM installation completes, runtime writes are permitted only beneath
   verified tmpfs mount points.
4. Persistent tweak payloads are modified only by KPM install, upgrade, and
   uninstall hooks.
5. Runtime code treats `/var/local/kmc/tweaks` as read-only.
6. Runtime wrapper assets are installed once and treated as read-only.
7. Cleanup is useful for disable and reframe, but hard-reboot recovery does not
   depend on cleanup, a journal, or a running daemon.
8. A hard reboot removes all wrapper mounts, original aliases, control state,
   and logs and leaves Kindle Substrate disabled.
9. Installing, upgrading, or removing a tweak never enables a disabled session.
10. Recovery-critical processes remain unconditionally blacklisted.

## Final Filesystem Contract

Persistent files written only by KPM lifecycle hooks:

```text
/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/
  app.sh
  wrapper.sh
  bin/<platform>/ksubstrate
  bin/<platform>/ksubstrated
  lib/<platform>/libksubstrate.so
  lib/<platform>/libksubstrate-bootstrap.so

/var/local/kmc/ksubstrate-assets/
  wrapper.sh

/var/local/kmc/tweaks/
  <tweak-id>/
    tweak.so
    tweak.ksfilter
    manifest.json

/var/local/kmc/ksubstrate-runtime/
  mounts/  # permanently empty mount point outside an active session
  state/   # permanently empty mount point outside an active session
```

Runtime-only views mounted by the daemon:

```text
/var/local/kmc/ksubstrate-runtime/mounts/  # tmpfs, exec
  original/usr/bin/pillow
  original/usr/bin/appmgrd

/var/local/kmc/ksubstrate-runtime/state/   # separate tmpfs, noexec
  control.sock
  mounts.journal
  session.env
  log/ksubstrated.log
  log/tweaks.log
```

The two tmpfs instances must be separately mounted and independently verified
through `/proc/self/mountinfo` before any runtime file is created.

## Scope

### In scope

- Strict path validation and typed mount paths.
- Direct `mount(2)` and `umount2(2)` integration.
- Separate mounts and state tmpfs instances.
- One package-installed generic wrapper.
- `ksubstrate wrapped-exec`.
- Transactional wrapper installation and best-effort cleanup.
- Tmpfs-only mount journal and control socket.
- Atomic tweak install, upgrade, and uninstall hooks.
- `reframe` and `reframe-if-active`.
- Runtime upgrade migration from the current basename-based layout.
- Host, privileged Linux, cross-build, package, and device tests.
- Documentation updates.

### Out of scope

- Changes to Dobby inline-hook implementation.
- Changes to the public hook ABI unrelated to logging.
- Live PID injection.
- Persistent boot-time enabling.
- Global `/etc/ld.so.preload` use.
- Automatic recovery that writes to system executables.

## Target Module Layout

Refactor `rust/ksubstrated/src/main.rs` into:

```text
rust/ksubstrated/src/
  main.rs       # CLI dispatch only
  control.rs    # Unix control socket and readiness protocol
  framework.rs  # Kindle framework restart and health checks
  journal.rs    # tmpfs-only intent/completion journal
  layout.rs     # typed and validated filesystem paths
  mounts.rs     # mount syscalls and mountinfo reconciliation
  session.rs    # enable, disable, reframe state machine
  tweaks.rs     # read-only tweak registry and root computation
```

## Phase 0: Freeze Safety Contracts

### Files

- `docs/kindle-substrate.md`
- `docs/architecture-strategy.md`
- `docs/project-context.md`
- `rust/ksubstrated/src/layout.rs` (new)

### Work

1. Add the non-negotiable invariants above to the as-built contract.
2. Define typed path wrappers:

   ```rust
   struct SystemExecutable(PathBuf);
   struct OriginalAlias(PathBuf);
   struct WrapperAsset(PathBuf);
   struct MountTmpfs(PathBuf);
   struct StateTmpfs(PathBuf);
   ```

3. Make constructors private and expose only validated creation functions.
4. Require system executables to be absolute regular executable files under
   `/usr/bin`, `/usr/sbin`, `/bin`, or `/sbin`.
5. Reject `..`, relative paths, unsupported roots, directories, and recovery
   blacklist entries.
6. Use `symlink_metadata`; initially reject symlinks. Device profiles may later
   name a verified canonical target explicitly.
7. Mirror the complete system path beneath the original-alias tmpfs:

   ```text
   /usr/bin/pillow  -> mounts/original/usr/bin/pillow
   /usr/sbin/pillow -> mounts/original/usr/sbin/pillow
   ```

### Exit criteria

- Unit tests prove same-basename paths do not collide.
- Invalid path classes are rejected.
- No raw `PathBuf` can be passed directly into a mount operation.

## Phase 1: Install Static Assets and Empty Mount Points

### Files

- `apps/com.bd452.ksubstrate/package/wrapper.sh` (new)
- `apps/com.bd452.ksubstrate/package/install.sh`
- `apps/com.bd452.ksubstrate/package/uninstall.sh`
- `apps/com.bd452.ksubstrate/package/app.sh`
- `apps/com.bd452.ksubstrate/build.sh`

### Work

1. Add one generic wrapper to the runtime package:

   ```sh
   #!/bin/sh
   PKG=/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate
   if [ -f /lib/ld-linux-armhf.so.3 ]; then
       PLAT=kindlehf
   else
       PLAT=kindlepw2
   fi
   exec "$PKG/bin/$PLAT/ksubstrate" wrapped-exec "$0" "$@"
   ```

2. During install, copy the wrapper to
   `/var/local/kmc/ksubstrate-assets/wrapper.sh` with mode `0555`.
3. During install, create `/var/local/kmc/tweaks` and the two empty runtime
   mount-point directories.
4. Assert the mount-point directories are not mounted and contain no files
   before installation writes into them.
5. Add enable, disable, status, and reframe Documents launchers.
6. During runtime package upgrade, use the old package's `app.sh disable` before
   replacing the daemon or wrapper asset.
7. Abort upgrade if the active old session cannot be disabled; require a reboot
   instead of continuing with mixed runtime versions.
8. During uninstall, disable first, then remove the wrapper asset and launchers.
9. Preserve third-party tweaks unless package policy explicitly chooses to
   remove them.

### Exit criteria

- Package install creates only documented persistent assets.
- Install does not enable a session.
- Upgrade cannot leave an old daemon managing new package files.
- Wrapper source is never generated or modified by the daemon.

## Phase 2: Tmpfs and Mount Syscall Layer

### Files

- `rust/ksubstrated/src/mounts.rs` (new)
- `rust/ksubstrated/src/layout.rs`
- `rust/ksubstrated/src/main.rs`
- `rust/ksubstrated/Cargo.toml`

### Work

1. Replace shell `mount` and `umount` commands with `libc::mount` and
   `libc::umount2`.
2. Mount separate tmpfs instances:

   ```text
   mounts: nodev,nosuid,exec,mode=0700,size=4m
   state:  nodev,nosuid,noexec,mode=0700,size=4m
   ```

3. Parse `/proc/self/mountinfo` and verify filesystem type, mount point, and
   relevant flags before creating runtime objects.
4. Create alias parent directories only after the mounts tmpfs is verified.
5. Create alias targets with `O_CREAT | O_EXCL | O_NOFOLLOW`; never use
   `fs::write` for alias creation.
6. Close the alias target descriptor before binding.
7. Bind the validated original executable onto the alias.
8. Remount the original alias bind read-only.
9. Bind the installed wrapper asset over the original executable path.
10. Remount the wrapper bind read-only.
11. Model alias progression with typestate:

    ```text
    AliasTarget<Unbound>
      -> AliasTarget<OriginalBound>
      -> AliasTarget<ReadOnly>
    ```

12. Do not expose a write-capable descriptor or generic path after binding.
13. Add mountinfo-based detection for existing wrapper and alias mounts.
14. If an unexpected target exists, reconcile or refuse activation. Never
    truncate, overwrite, copy over, or unlink a bound alias blindly.

### Exit criteria

- The mount module has no code path that opens a system executable or bound
  alias for writing.
- Original and wrapper binds are read-only.
- A second enable cannot truncate or overwrite an existing mount target.
- Original executable hashes remain unchanged across all host mount tests.

## Phase 3: Tmpfs Journal and Control Plane

### Files

- `rust/ksubstrated/src/journal.rs` (new)
- `rust/ksubstrated/src/control.rs` (new)
- `rust/ksubstrated/src/session.rs` (new)
- `rust/ksubstrated/src/main.rs`

### Work

1. Move all runtime state into the state tmpfs.
2. Replace persistent PID, disable marker, wrapper list, start history, and
   environment files with state-tmpfs equivalents.
3. Add a Unix control socket at `state/control.sock`.
4. Define commands: `status`, `disable`, `reframe`, and `shutdown`.
5. Add a readiness response so `enable` returns success only after tmpfs setup,
   wrapper installation, and framework restart complete.
6. Journal intent before and completion after each mount transition:

   ```text
   PrepareAlias
   BindOriginal
   ProtectOriginal
   BindWrapper
   ProtectWrapper
   Complete
   ```

7. On startup or control-process recovery, reconcile journal entries against
   `/proc/self/mountinfo`.
8. If reconciliation is ambiguous, refuse to enable and instruct the user to
   reboot. Do not attempt destructive repair.
9. Handle `SIGTERM`, `SIGINT`, and `SIGHUP` as best-effort disable requests.
10. Keep hard reboot as the only unconditional recovery guarantee.

### Exit criteria

- Killing the daemon at every journal stage never modifies an original file.
- A replacement daemon can normally disable a crashed session.
- Missing or contradictory state produces a safe refusal, not an overwrite.
- No runtime state survives reboot.

## Phase 4: Device CLI `wrapped-exec`

### Files

- `rust/ksubstrate-cli/src/main.rs`

### Work

1. Add:

   ```text
   ksubstrate wrapped-exec <invoked-path> [args...]
   ```

2. Reuse the existing preload and library-path construction from `run`.
3. Validate the invoked path using the same path rules as the daemon.
4. Derive and verify its original alias beneath the mounts tmpfs.
5. Confirm the alias is a read-only bind mount using mountinfo.
6. Set:

   ```text
   LD_PRELOAD=<bootstrap>
   LD_LIBRARY_PATH=<runtime lib directory>
   KSUBSTRATE_TWEAKS_DIR=/var/local/kmc/tweaks
   KSUBSTRATE_LOG=<state tmpfs>/log/tweaks.log
   ```

7. Use `CommandExt::exec` to replace the wrapper process with the original
   executable.
8. Preserve ordinary arguments. Document that the shell wrapper cannot preserve
   an intentionally forged original `argv[0]`; verify that Kindle framework
   launches use normal executable paths.

### Exit criteria

- A synthetic target launched through a bind-mounted wrapper receives its
  original arguments and preload environment.
- Invalid or unbound invoked paths are refused.
- The wrapper does not recurse into itself.

## Phase 5: Read-Only Tweak Registry

### Files

- `rust/ksubstrated/src/tweaks.rs` (new)
- `rust/ksubstrate-bootstrap/src/lib.rs`
- `rust/ksubstrate/src/lib.rs`

### Work

1. Make `/var/local/kmc/tweaks` the single persistent registry.
2. During runtime, scan and read only. Do not create, rename, delete, or repair
   tweak files.
3. Validate package ID, manifest, platform, filter, library, dependencies,
   conflicts, and duplicate IDs before enabling or reframing.
4. Ensure discovered paths remain beneath the tweak registry.
5. Keep the USB sentinel check first in bootstrap initialization.
6. Add deterministic one-time bootstrap initialization.
7. Define one initialization contract so a tweak is not initialized twice by
   both `.init_array` and `ksubstrate_init`.
8. Point runtime logs into the state tmpfs.
9. If `KSUBSTRATE_LOG` is absent, log to stderr or syslog instead of creating a
   persistent/default file.

### Exit criteria

- Runtime source has no write operation against the tweak registry.
- Bootstrap loads only validated registry entries.
- Tweak initialization occurs at most once per process.

## Phase 6: Atomic Tweak Package Lifecycle

### Files

- `rust/ksub/src/main.rs`
- generated tweak `package/install.sh`
- generated tweak `package/uninstall.sh`
- `docs/build-and-publish.md`

### Work

1. Replace direct copies into the final tweak directory with same-filesystem
   staging:

   ```text
   /var/local/kmc/tweaks/.<id>.staging.<pid>
   ```

2. Select and stage the correct ABI library.
3. Validate all staged files before activation.
4. Move an existing version to a hidden retired path.
5. Rename staging to the final package ID.
6. Roll back the retired version if activation fails.
7. After successful install or upgrade, request deferred
   `reframe-if-active`.
8. On uninstall, atomically rename the active directory out of the visible
   registry, request deferred reframe, then remove the retired payload.
9. Keep all persistent mutations inside KPM lifecycle hooks.
10. Never enable Substrate from a tweak lifecycle hook.

### Exit criteria

- Bootstrap never observes a partially copied tweak.
- Failed upgrade preserves the previous working tweak.
- Install and uninstall reframe only an already active session.

## Phase 7: Reframe

### Files

- `rust/ksubstrated/src/session.rs`
- `rust/ksubstrated/src/control.rs`
- `rust/ksubstrated/src/framework.rs` (new)
- `apps/com.bd452.ksubstrate/package/app.sh`
- `apps/com.bd452.ksubstrate/package/install.sh`

### Work

1. Add `reframe` and `reframe-if-active` to daemon and package control surfaces.
2. Serialize enable, disable, and reframe through the control socket.
3. Make package-hook reframe requests deferred until the requesting process has
   disconnected and the KPM hook can return.
4. If disabled, `reframe-if-active` returns success without starting anything.
5. For an active session:

   ```text
   rescan and validate tweaks
   compute new roots
   stop/restart framework processes as appropriate
   remove current wrapper overlays
   remove current original aliases
   install the newly required mount set
   restart the framework hooked
   run health checks
   ```

6. If re-arming fails, perform best-effort cleanup and restart the stock
   framework.
7. Keep framework commands and health logic in `framework.rs`.
8. Add a Reframe Kindle UI launcher and update user-facing messages.

### Exit criteria

- Installing a tweak into an active session loads it after reframe.
- Removing a tweak unloads it after reframe.
- Reframe handles added and removed process roots.
- Reframe never enables a disabled session.
- Failed reframe returns the UI to stock or clearly requires reboot.

## Phase 8: Legacy Migration

### Files

- `apps/com.bd452.ksubstrate/package/uninstall.sh`
- `apps/com.bd452.ksubstrate/package/install.sh`
- `rust/ksubstrated/src/mounts.rs`

### Work

1. Before replacing the current runtime, invoke the old `app.sh disable` while
   the old daemon and basename layout are still present.
2. Verify no old wrapper mounts remain under:

   ```text
   /var/local/kmc/ksubstrate/run/orig
   /var/local/kmc/ksubstrate/run/wrappers
   ```

3. If legacy mounts remain, stop the upgrade and require a reboot.
4. Remove stale unmounted legacy files only during install/upgrade, never from
   the running daemon.
5. Do not automatically re-enable after runtime upgrade.

### Exit criteria

- No new daemon attempts to clean old-layout mounts with new-layout paths.
- Upgrade cannot leave an active mixed-version session.

## Phase 9: Verification

### Host unit tests

- Path validation and collision avoidance.
- Mountinfo parser and mount flag validation.
- Journal state transitions and crash reconciliation.
- Session state-machine transitions.
- Root-set diffing for reframe.
- Disabled `reframe-if-active` behavior.
- Atomic tweak lifecycle script generation.
- Bootstrap one-time initialization.

### Privileged Linux integration tests

Add `scripts/test-mount-session.sh` or an equivalent Rust integration harness
that runs inside a disposable privileged container/mount namespace.

Test:

1. Bind a fixture original to an alias.
2. Protect the alias read-only.
3. Bind the static wrapper over the fixture path.
4. Launch through the wrapper and confirm the original executes.
5. Inject process death after every journal stage.
6. Reconcile or disable each partial stage.
7. Confirm original SHA-256 and bytes never change.
8. Confirm a second enable never truncates an alias.
9. Confirm mounts and state are separate tmpfs instances.
10. Confirm unmount or namespace destruction restores the original path.

### Cross-build and package tests

- Run the complete host workspace test suite.
- Cross-build runtime and demo packages for `kindlehf` and `kindlepw2`.
- Inspect ELF ABI and interpreter values.
- Inspect `.kpkg` contents and executable modes.
- Rebuild `kinstaller-repo` and verify dependency metadata remains intact.

### Device validation gates

For one verified device per ABI:

1. Identify model, firmware, ABI, and package hashes.
2. Install runtime without enabling.
3. Verify persistent install layout and empty runtime mount points.
4. Confirm static wrapper `$0` behavior through a harmless fixture target.
5. Confirm tmpfs mount options and read-only bind remounts.
6. Run the self-contained inline/GOT demo.
7. Enable framework wrapping and verify UI, SSH, USB, power, and storage.
8. Install a tweak and verify deferred reframe.
9. Remove a tweak and verify deferred reframe.
10. Kill the daemon at each mount stage and verify safe behavior.
11. Hard reboot with an active session and verify all mounts and state disappear.
12. Capture evidence under `analysis/evidence/<device>-<firmware>/`.

## Required Source-Level Guardrails

Add a CI check that fails if `rust/ksubstrated/src/mounts.rs` introduces calls to
ordinary file mutation APIs outside the single exclusive tmpfs-target creation
function. At minimum, flag:

```text
fs::write
fs::copy
fs::rename
fs::remove_file
OpenOptions::truncate
OpenOptions::create (without create_new)
```

Also require a code review for any change touching:

```text
SystemExecutable
OriginalAlias
mounts.rs
runtime package install/uninstall hooks
```

## Delivery Order

1. Phase 0: safety contract and typed paths.
2. Phase 1: static installed wrapper and mount-point skeleton.
3. Phase 2: tmpfs and safe mount layer.
4. Phase 3: control socket, readiness, and journal.
5. Phase 4: `wrapped-exec`.
6. Phase 5: read-only runtime tweak registry.
7. Phase 6: atomic tweak package lifecycle.
8. Phase 7: reframe.
9. Phase 8: legacy migration.
10. Phase 9: host, container, cross-build, downstream, and device validation.

Do not enable framework wrapping on a physical device until Phases 0 through 8
are implemented and the privileged Linux integration suite proves that original
fixture bytes remain unchanged through every crash-injection case.

## Definition of Done

This plan is complete only when all of the following are true:

1. Runtime code performs no persistent writes after installation.
2. Original executable and alias write access is structurally impossible.
3. Runtime state is confined to two verified tmpfs instances.
4. Static wrapper launch works on both Kindle ABIs.
5. Disable and reframe work without requiring reboot in normal operation.
6. A hard reboot clears all active session state without cleanup assistance.
7. Tweak install, upgrade, and uninstall are atomic and reframe correctly.
8. A disabled session remains disabled through package operations.
9. Runtime package upgrade safely exits the legacy layout.
10. Host, privileged mount, cross-build, downstream package, and both-ABI device
    evidence gates pass.

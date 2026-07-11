# Explicit Targets and Transactional Controller Plan

## Goal

Replace filter and process-name based loading with explicit manifest-v2 targets.
Replace the long-running daemon with a short-lived controller whose verified
tmpfs mounts and committed session plan define whether a session is active.

## Non-negotiable invariants

1. Runtime never opens a system executable or bound original alias for writing.
2. Runtime writes only beneath verified `mounts` and `state` tmpfs instances.
3. KPM lifecycle hooks exclusively mutate the persistent tweak registry.
4. Bootstrap loads only the exact target and libraries in a committed launch
   plan; it never uses `comm`, executable basenames, or live registry scans.
5. Unknown profiles, targets, KPM packages, ambiguous journals, and unsafe
   paths fail closed.
6. A hard reboot drops mounts and state and therefore leaves Substrate disabled.

## Manifest v2

```json
{
  "manifest_version": 2,
  "id": "com.example.reader-tweak",
  "library": "tweak.so",
  "initialization": "constructor",
  "targets": [
    "pillow",
    {
      "kind": "kpm",
      "package": "com.example.reader",
      "path": "bin/{platform}/reader"
    }
  ]
}
```

Manifest v1 and `tweak.ksfilter` are removed. Built-in strings are profile
names. KPM objects resolve only beneath the named package root and require an
explicit target-package lifecycle contract before they can be wrapped.

## Controller model

`ksubstrated` is a compatibility-named short-lived controller:

```text
enable | disable | status | reframe-if-active | post-package-change
prepare-target-package-change <package>
finish-target-package-change <package>
```

It locks the persistent runtime parent directory with `flock` before mounting
tmpfs, reconciles any previous journal, performs one transaction, then exits.
There is no socket, monitor, PID file, or background health loop.

States are `Disabled`, `Transitioning`, `Active`, and `RecoveryRequired`.
`Active` requires both verified tmpfs mounts, a complete journal, and a
committed session plan.

## Session plan and launch identity

The controller writes a tmpfs plan containing canonical target IDs, expected
aliases, generation, restart classes, and exact tweak library identities. A
library identity includes path, device, inode, size, and digest.

The immutable wrapper passes:

```text
KSUBSTRATE_TARGET=<canonical target ID>
KSUBSTRATE_EXPECTED_EXE=<original alias>
KSUBSTRATE_SESSION_GENERATION=<generation>
```

Bootstrap verifies `/proc/self/exe`, reads only the matching committed plan
entry, revalidates each library identity, and loads those exact libraries. A
mismatch loads nothing and clears inherited Substrate variables.

`ksubstrate run` creates a one-shot in-kernel pipe plan for an explicit target;
the bootstrap consumes and closes it. This keeps demos usable while disabled
without persistent runtime state.

## Reconciliation

Every reframe derives the complete target set, tears down all old overlays,
creates a fresh mounts tmpfs, installs the complete new set, publishes a
pending plan, restarts required trusted profile classes, runs health checks,
then commits the plan. Framework restart occurs only when an old or new target
belongs to the framework restart class.

Each mount transition journals intent and completion. Incomplete or
contradictory transactions require reboot rather than guessing.

## KPM lifecycle

Tweak hooks stage and validate on the registry filesystem, atomically activate
or hide an entry, synchronously call `post-package-change`, and roll back the
persistent registry if active-session reconciliation fails. Disabled sessions
return success without mounting anything.

KPM executable targets require target-package opt-in hooks. Their pre-hook
removes affected overlays before KPM mutates package files; their finish hook
reconciles afterward. Arbitrary third-party KPM package binaries are rejected.

## Verification

Host tests cover manifest parsing, profile lookup, resolver rejection cases,
session-plan identity, disabled behavior, and hook rollback. Privileged Linux
tests cover full transactions, crash recovery, hash preservation, target
identity, and namespace cleanup. Both Kindle ABIs must cross-build and device
validation must confirm profile paths and `/proc/self/exe` alias behavior.

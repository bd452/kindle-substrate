# Kindle Substrate Project Context

Last updated: 2026-07-10

## Why This Repository Exists

Kindle Substrate is intended to provide a Substrate/Substitute-class native
hooking platform for jailbroken Kindles. It combines an on-device runtime with
host-side development tooling and KPM packages.

The project began inside `bd452/kinstaller-repo`, where the runtime, toolchain,
demo, documentation, and package sources were implemented together. As the
project grew beyond a package recipe, it became clear that it needed its own
repository, release lifecycle, test surface, and architecture boundary.

This repository is that boundary. The KPM registry remains the distribution
repository, but the integration is artifact-first: Kindle Substrate builds and
releases its two packages plus a verifiable descriptor, and the registry
consumes that descriptor without compiling this source repository.

## Extraction History

The initial implementation entered `kinstaller-repo` in commit `b4a1c65`. Before
the extraction, the complete state was preserved on the local branch
`codex/ksubstrate-pre-extraction`; its snapshot commit is `8861a48`.

The standalone repository's initial import commit is `45e9766`. The import
includes the previously untracked analysis notes so the standalone project has
the full working context, not just the files that happened to be committed in
the parent repository.

This repository is the source of truth for Kindle Substrate runtime and
toolchain work. It is published at
[`bd452/kindle-substrate`](https://github.com/bd452/kindle-substrate).
The original registry integration consumed a pinned submodule commit under
`components/kindle-substrate`. The current boundary supersedes that arrangement
with immutable release artifacts and `release-metadata.json`; registry cutover
and publication remain outside this repository's scope.

## Product Model

The core safety decision is that hooking is session-scoped:

1. A hard reboot starts clean, with no hooks enabled.
2. The user explicitly enables a hooked session.
3. `ksubstrated` installs volatile wrappers for approved process roots.
4. The Kindle framework restarts through those wrappers.
5. `LD_PRELOAD` loads the bootstrap before target code runs.
6. The bootstrap verifies an explicit target identity and loads its planned tweaks.
7. Disable, crash recovery, or reboot returns the device to stock behavior.

The design avoids global `/etc/ld.so.preload`, persistent boot-time arming,
power-button safe modes, and PID-chasing injection as the primary mechanism.
Those choices matter on a one-button device where reboot and USB access are the
most dependable recovery tools.

## What Has Been Implemented

### Runtime

- `libksubstrate.so` with a stable C ABI.
- Dobby-backed ARM and Thumb-2 inline hooking.
- Hook removal and duplicate-hook tracking.
- Checked inline hooks that verify expected prologue bytes.
- ELF PLT/GOT import hooking through `kh_hook_import`.
- Symbol lookup through `dlsym` and Dobby's symbol resolver.
- Firmware-private address resolution through module load base plus RVA.
- A preload bootstrap that checks the USB disable sentinel and loads only the
  exact target entries in the committed session plan.
- `ksubstrated`, a short-lived transactional controller for wrappers,
  framework restart, cleanup, and recovery.
- A device CLI for launching a process with the runtime environment.

### Packages and Diagnostics

- `com.bd452.ksubstrate`, containing the runtime libraries, daemon, CLI,
  public header, enable/disable launchers, and tweak directory contract.
- `com.bd452.ksubstratedemo`, containing a target process and sample tweak.
- A demo inline hook that changes `compute()` from 41 to 42.
- A demo GOT hook for an imported libc call.
- An opt-in inheritance probe that records which child processes receive the
  preload environment.
- Builds for `kindlehf` and `kindlepw2`.

### Host Tooling

- `ksub new`, `build`, `deploy`, `package`, `pull`, `analyze`, and symbol
  command surfaces.
- A small Logos-style preprocessor supporting `KSYM`, `%hookf`, `%orig`,
  `%ctor`, and `%init` in its current constrained syntax.
- A symbol database parser and C-header generator.
- Initial dynamic-symbol extraction through `nm -D`.
- Firmware acquisition notes under `analysis/`.

## Repository Layout

```text
apps/       KPM runtime and demo package sources
analysis/   Firmware acquisition and reverse-engineering notes
docs/       Architecture, audits, and project context
rust/       Runtime, demos, diagnostics, and host-tool crates
scripts/    Cross-toolchain, container, and package-build helpers
```

The pinned Dobby dependency remains a nested submodule at:

```text
apps/com.bd452.ksubstrate/vendor/Dobby
```

The Dobby build now patches an isolated copy under Cargo's build directory.
Building the project therefore does not dirty the pinned submodule.

## Verification Completed During Extraction

- All 24 standalone host tests passed.
- The standalone workspace cross-built for both Kindle ARM targets.
- Both standalone KPM packages were assembled successfully.
- The Dobby source remained clean after the cross-build.
- A complete `kinstaller-repo` container build passed while consuming package
  sources through `components/kindle-substrate`.
- The parent package manifest retained both package IDs and their dependency
  relationship.
- The remaining parent SignalKit test suite passed: 39 tests.
- Generated package archives were excluded from the migration diff when their
  only changes were repacking timestamps.

These checks prove that the repository boundary and build integration work.
They do not substitute for validation on physical Kindle hardware.

## Current Engineering Status

This is an experimental architecture implementation, not a production-ready
hooking platform. The outer system is substantial and buildable, but several
load-bearing behaviors still require correction or device evidence.

The forward plan — north star, layer ownership, milestones M0–M3 with binary
exit gates, design decisions, and target contracts — lives in
[architecture-strategy.md](architecture-strategy.md). Prefer that document when
deciding what to implement next.

Highest-priority work (also expanded in the strategy doc):

1. Validate Dobby inline hooks on real `kindlehf` and `kindlepw2` devices,
   including non-trivial ARM/Thumb prologues and unhooking.
2. Harden GOT hooks: resolve the true original symbol, handle lazy binding,
   prevent recursion and self-hooking, restore page permissions, support
   rollback, and add import unhooking.
3. Add daemon readiness acknowledgment, locking, signal-driven cleanup, and a
   mount journal that survives partial wrapper installation.
4. Replace guessed process paths with verified model/firmware profiles.
5. Make bootstrap initialization deterministic and panic-contained, and avoid
   double initialization when a tweak has both a constructor and explicit init.
6. Define one atomic tweak installation registry and a versioned manifest with
   ordering, dependencies, conflicts, ABI requirements, and firmware support.
7. Connect firmware identity, ELF build IDs, symbol databases, checked RVAs,
   and generated hook descriptors into one end-to-end SDK workflow.
8. Replace or deliberately constrain the text-based Logos preprocessor before
   presenting it as a general tweak language.
9. Add automated ELF fixtures, recursive-clone tests, device smoke tests, and a
   recovery matrix to CI and release procedures.

## Ownership Boundary

This repository owns:

- Runtime and host-tool source.
- Dobby integration and the public hook ABI.
- Device session behavior and recovery logic.
- Tweak format and SDK conventions.
- Runtime/demo package source, independent package builds, and tagged release
  artifacts with a combined registry-facing descriptor.
- Architecture, analysis, and device verification documentation.

The KPM registry owns:

- Pinned, checksum-verified source release descriptors.
- Repository-wide dependency validation and catalog composition.
- The published KPM `manifest.json`.
- GitHub Pages deployment for the KPM repository.

This boundary allows Kindle Substrate to evolve and be tested independently
without turning the package index into the implementation repository again.

## Release Posture

Do not describe the current runtime as production-safe until the physical-device
matrix passes. A first credible release milestone is narrower:

- One verified firmware on each Kindle ABI.
- Repeatable inline and import-hook demos.
- Reliable enable, disable, crash fallback, and hard-reboot recovery.
- A clean recursive clone and independent package build.
- A documented rollback procedure and known-good package pair.

Once those conditions are met, later work can expand firmware coverage, symbol
analysis, developer ergonomics, and more advanced injection modes without
weakening the clean-boot recovery invariant.

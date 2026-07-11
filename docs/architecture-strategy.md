# Kindle Substrate — Architecture & Strategy

Last updated: 2026-07-10

Forward plan for turning the experimental session shell into a Substrate-class
platform **without weakening clean-boot recovery**.

Related docs:

- [kindle-substrate.md](kindle-substrate.md) — as-built architecture and contracts
- [kindle-substrate-divergence.md](kindle-substrate-divergence.md) — historical audit vs original plan
- [project-context.md](project-context.md) — repo boundary, extraction history, current status

---

## Overview

Kindle Substrate already has a substantial outer system: a C hook ABI, Dobby
inline backend, ELF GOT rewriter, LD_PRELOAD bootstrap, session daemon with
volatile bind-mount wrappers, host tooling, and KPM packages. Host tests and
cross-builds pass. What it does **not** yet have is device-proven behavior,
hardened GOT/session edge cases, an honest end-to-end symbol pipeline, or a
constrained-but-documented developer SDK.

This document locks **direction**, **layer ownership**, **milestones with binary
exit gates**, and **design decisions**. Work should not expand surface area ahead
of evidence.

### North star

A jailbroken Kindle can run third-party native tweaks that hook:

- exported symbols,
- imports (PLT/GOT),
- firmware-private RVAs (module base + prologue check),

**only for the duration of an explicitly enabled session.** Disable, crash
guard, USB sentinel, or hard reboot always returns stock behavior.

Out of scope as primary mechanisms:

- global `/etc/ld.so.preload`
- persistent boot-time arming
- power-button safe mode as the main recovery path
- ptrace / live PID injection

### Strategy in one line

**Prove on hardware first → harden the runtime second → build an honest symbol
SDK third → ergonomics last.** Never claim “works” from CI green alone.

| Milestone | Focus | Role |
|-----------|--------|------|
| **M0** | Device proof | Gate for any “works” claim |
| **M1** | Runtime harden | Safe under partial failure |
| **M2** | SDK + symbols | Firmware → DB → checked hooks |
| **M3** | Ergonomics | Third-party tweak authoring |

---

## Non-negotiables

### Do first

1. Prove hooks and recovery on **one firmware per ABI** before expanding surface area.
2. Prefer **PLT/GOT** for imports; use **Dobby inline** only when the target is not an import.
3. Keep session state **volatile** (bind mounts). Never rename rootfs binaries as the primary path.
4. Treat symbol DBs as **versioned, prologue-checked** data — never silent RVA patches.

### Deliberately defer / constrain

1. Full Logos compatibility — ship a documented **dialect**, not a clone.
2. Automated Ghidra/AI naming — manual promote workflow is v1-honest.
3. Broad firmware matrix — one verified pair first, then expand.
4. Advanced injection modes — out of scope until M0–M1 are solid.

---

## System layers

Each layer has one owner contract. **Lower layers must not depend on upper ones.**

| Layer | Owns | Public contract | Current gap |
|-------|------|-----------------|-------------|
| **L0 Engine** | `libksubstrate` + Dobby + ELF GOT | `ksubstrate.h` C ABI | GOT harden; import unhook; device verify |
| **L1 Bootstrap** | LD_PRELOAD loader | sentinel → filter → `dlopen` → optional init | double-init guard; panic containment |
| **L2 Session** | `ksubstrated` | enable/disable/status + volatile wraps | readiness, journal, firmware profiles |
| **L3 Packages** | KPM runtime + demo | `.kpkg` layout + launchers | tweak registry; path convergence |
| **L4 Host SDK** | `ksub` / logos / syms | build → package → deploy → symbols | pull automation; Logos dialect; DB pipeline |
| **L5 Evidence** | CI + device matrix | host fixtures + smoke + recovery | device smoke; recovery matrix |

```text
L5 Evidence
L4 Host SDK (ksub, logos, syms)
L3 Packages (.kpkg)
L2 Session (ksubstrated)
L1 Bootstrap (LD_PRELOAD)
L0 Engine (inline + GOT ABI)
```

---

## Milestone roadmap

Exit criteria are **binary**. Do not start the next milestone until the prior
exit gate passes on hardware where required.

### M0 — Credible device proof

**Goal:** One `kindlehf` + one `kindlepw2` firmware where demo inline + GOT hooks
and the full recovery ladder are **observed**, not inferred.

**Work:**

- [ ] Document device lab: SSH, USBNetwork, package install, log paths
- [ ] Run `com.bd452.ksubstratedemo`: `compute → 42` + GOT `write` log
- [ ] Exercise enable → hooked UI → disable → stock (`KSUBSTRATE_SYSTEM_WRAP=1`)
- [ ] Recovery matrix: USB sentinel, crash guard, hard reboot
- [ ] Capture evidence pack: logs, firmware version, ABI, package SHAs

**Exit:** Evidence under `analysis/evidence/<device>-<fw>/` with PASS/FAIL per
scenario. No production language until both ABIs have a PASS pack.

**Evidence pack shape (suggested):**

```text
analysis/evidence/<device>-<fw>/
  README.md          # device model, firmware, ABI, package SHAs, date
  demo-inline.log
  demo-got.log
  session-enable.log
  session-disable.log
  recovery-usb.md
  recovery-crash.md
  recovery-reboot.md
  RESULTS.md         # PASS/FAIL table
```

---

### M1 — Runtime correctness

**Goal:** Make L0–L2 safe under partial failure, lazy binding, and framework
spawn quirks — still without expanding the SDK story.

#### L0 GOT / inline

- [ ] Resolve true original via `dlsym` / symtab before rewrite
- [ ] Force bind so lazy stubs are not stored as `orig`
- [ ] Restore page `PROT` after `mprotect`; track slots for rollback
- [ ] Add `kh_unhook_import`; refuse self-hook / already-hooked
- [ ] ELF fixture suite: REL/RELA, multi-slot, missing symbol

#### L1–L2 session / bootstrap

- [ ] Ready file + flock so enable waits for wrap complete
- [ ] Mount journal (`src`, `dst`, `stage`) written before each mount
- [ ] `SIGTERM` / `SIGINT` / `SIGHUP` → `cleanup_session`; `atexit` backup
- [ ] Firmware profiles YAML: model → verified spawn roots
- [ ] Converge tweaks dir to `/var/local/kmc/tweaks` (+ package symlink if needed)
- [ ] Bootstrap: once-flag; `catch_unwind` around `dlopen` / init

**Exit:** Host GOT fixtures green; daemon unit tests for journal restore; re-run
M0 evidence pack on the same devices after harden.

---

### M2 — Symbol SDK (honest pipeline)

**Goal:** Connect firmware identity → build IDs → symbol DB → checked hooks
end-to-end, without pretending Ghidra automation exists.

- [ ] DB schema v1: `firmware`, `build_id`, `image`, `name`, `rva`, `prologue`, `source`, `status`
- [ ] `ksub pull --host`: SCP UI bins + `ldd` closure into `analysis/pulled/<fw>/`
- [ ] `ksub analyze`: `nm -D` + `readelf` build-id + optional prologue bytes at RVA
- [ ] `ksub sym promote`: proposed → promoted; refuse empty prologue for private RVAs
- [ ] Codegen: header + descriptors using `kh_resolve_rva` + `kh_hook_function_checked`
- [ ] Docs: manual Ghidra→YAML recipe; `propose` stays human-in-loop

**Exit:** One non-exported function hooked on-device via a promoted DB entry,
with prologue mismatch refusing a wrong firmware.

---

### M3 — Tweak platform ergonomics

**Goal:** Make third-party tweaks installable and authorable without expanding
injection modes.

#### Registry & Logos

- [ ] Tweak manifest v2: `id`, `version`, `abi`, `firmware[]`, `depends`, `conflicts`, `order`, `filter`, `library`
- [ ] Atomic install: stage → validate → rename into tweaks/; `registry.json`
- [ ] Freeze `ksub-logos` dialect; reject unsupported constructs with clear errors
- [ ] `KSYM` via DB (RVA + checked) when image known; else `dlsym`

#### Inheritance & CI

- [ ] Probe report → suggested Tier-2 roots; optional auto-wrap flag
- [ ] `ksub new library|tool` scaffolds real crates; tweak uses manifest v2
- [ ] CI: recursive-clone job + ELF fixture corpus + cbindgen drift check
- [ ] Device smoke workflow consuming the evidence checklist

**Exit:** An external tweak can be authored with the Logos dialect + promoted
symbols, installed atomically, and survive enable/disable on the M0 device pair.

---

## Key design decisions

| Decision | Choice | Why | Reject |
|----------|--------|-----|--------|
| Injection | Volatile bind-mount + `LD_PRELOAD` | Reboot clears mounts → clean boot for free | Rename-in-place; global preload |
| Hook preference | GOT for imports; Dobby for bodies | GOT survives updates better; less prologue risk | Inline-everything; Darwin `ImportTableReplace` |
| Private symbols | RVA + prologue check | Firmware drift fails closed | Blind base+offset patches |
| Symbol pipeline | `nm`/build-id now; Ghidra manual | Honest MVP; no fake automation | Claiming AI/Ghidra extraction in v1 |
| Logos | Constrained dialect | Usable macros without a full preprocessor | Pretending full Theos Logos parity |
| Profiles | Data-driven spawn roots | Kindles differ by model/firmware | Hardcoded `/usr/bin` guesses forever |
| Header | cbindgen-generated | ABI and header cannot drift | Hand-maintained `ksubstrate.h` only |
| Release bar | Evidence packs per ABI | Host tests ≠ device truth | Ship on CI green alone |

---

## Target contracts

Concrete shapes the milestones converge on. These are design targets, not yet
fully implemented.

### GOT hook ownership (L0)

Per-process registry in `libksubstrate`:

```text
ImportHook {
  image, symbol,
  slots: [*mut *mut c_void],
  original: *mut c_void,      // true resolved symbol
  replacement: *mut c_void,
  page_prot_restored: bool,
}
```

- `kh_hook_import` installs and records slots.
- `kh_unhook_import` restores all slots and page protection.
- Process exit is best-effort cleanup.

### Mount journal entry (L2)

Append-only under `run/mounts.journal`:

```text
{ root, orig_mount, wrapper,
  stage: bind_orig | write_wrap | bind_shadow | done,
  ts }
```

On start/cleanup: replay incomplete stages in reverse. Survives crash mid-install
better than `wrappers.list` alone.

### Firmware profile (L2)

`package/profiles/<model>.yaml` (example):

```yaml
model: PW5
abi: kindlehf
firmware: "5.16.2"
spawn_roots:
  - /usr/bin/pillow
  - /usr/bin/appmgrd
blacklist: [powerd, sshd, dbus-daemon, otav3, otaupd, mmcqd, wpa_supplicant, dhcpd]
build_ids:
  pillow: "abcd..."
```

### Symbol DB record (L4)

`analysis/symbols.<fw>.yaml` (example):

```yaml
firmware: "5.16.2"
symbols:
  - name: Reader::openBook
    image: /usr/bin/reader
    build_id: "…"
    rva: 0x1234
    prologue: "00b5…"   # required if not exported
    source: ghidra       # ghidra | nm | manual
    status: promoted     # proposed | promoted
```

---

## Dependency order

Parallelism is allowed only **within** a milestone after its prerequisites.

| From | To | Reason |
|------|----|--------|
| M0 evidence | Any “done” claim | Device truth |
| M1 GOT harden | M0 re-verify | Behavior change needs re-proof |
| M1 profiles | Tier-2 auto-wrap | Must know real paths |
| M1 path converge | Tweak registry | One install root |
| M2 DB schema | Logos `KSYM` → RVA | Macros need promoted data |
| M2 promote | Checked private hooks | Prologue gate |
| M3 registry | Third-party tweaks | Install contract |
| M3 dialect freeze | Public docs | Avoid overclaiming Logos |

```text
M0 (device proof)
  │
  ▼
M1 (GOT + session + bootstrap harden) ──► re-run M0
  │
  ▼
M2 (symbol DB + pull/analyze/promote + codegen)
  │
  ▼
M3 (registry, Logos dialect, inheritance, CI)
```

---

## Verification pyramid

### Host (every PR)

- `cargo test --workspace`
- Synthetic ELF GOT fixtures
- Daemon journal / unit tests
- Logos dialect snapshot tests
- cbindgen drift check

### Cross-build (every PR)

- `kindlehf` + `kindlepw2` packages
- Recursive submodule clone job
- Dobby static-link smoke

### Device (release gate)

- Demo inline + GOT
- Enable / disable / UI wrap
- Recovery ladder (disable, USB sentinel, crash guard, partial-wrap rollback, hard reboot)
- Evidence pack artifact

---

## What “done” means for v1

v1 is complete when **all** of the following are true:

1. Both ABIs have a **PASS** evidence pack.
2. GOT hooks are **reversible** and restore page permissions.
3. The daemon **journals** mounts and cleans up on signal.
4. One **private RVA** hook works via a promoted DB entry.
5. Logos is documented as a **dialect** (not full Logos).
6. Tweak install is **atomic** under a single path.

Everything else (broader firmware coverage, richer analysis, advanced injection)
is post-v1 expansion and must not weaken the clean-boot invariant.

---

## Current state snapshot (as of this doc)

Already in good shape relative to the original divergence audit:

- Dobby-backed ARM/Thumb inline hooks (host mock; device verify pending)
- Native ELF PLT/GOT `kh_hook_import` (host fixtures; harden + device verify pending)
- Volatile bind-mount session wrappers, blacklist, UI health / crash guard
- Bootstrap USB sentinel, filter match (incl. 15-byte `comm` truncation), optional `ksubstrate_init`
- Demo package exercising real inline + GOT paths
- Inheritance probe (opt-in diagnostics)
- Host `ksub` / experimental logos / YAML symbol DB parser

Still open (see milestones above): device evidence, GOT/session hardening, symbol
pipeline beyond scaffolding, Logos dialect freeze, tweak registry, cbindgen.

---

## How to use this document

1. Treat milestone exit gates as the definition of progress.
2. Prefer implementing the next unchecked item in the **lowest incomplete milestone**.
3. When behavior changes in L0–L2, re-run the M0 evidence pack.
4. Keep [kindle-substrate.md](kindle-substrate.md) as the as-built contract; update
   this file when strategy or milestone scope changes.

# Kindle Substrate — Divergence & Remediation Plan

> Historical implementation audit from the initial extraction. Resolved items
> remain useful context; open runtime verification work is tracked in this repo.

This document audits the **as-built** implementation against the intended
architecture and the original implementation plan, and lays out a tracked path
back to spec.

Reference documents:

- **Architecture** — the consolidated design (sections cited below as `A§n`).
- **Original plan** — retained by reference in the source repository history
  (todo IDs cited as `plan:<id>`).
- **Current doc** — [kindle-substrate.md](kindle-substrate.md).

Legend for **Direction**:

- `MISSING` — specified, not built.
- `PLACEHOLDER` — built as a self-contained stand-in that doesn't do the real work.
- `OVERSTATED` — presented as working, is scaffolding.
- `UNSAFE-VARIANT` — built, but chose a riskier option than the spec offered.
- `STRUCTURE` — deviates from the intended repo/process shape.

Legend for **Status**:

- `OPEN` — not addressed.
- `PARTIAL (this session)` — partially addressed in the current working tree.
- `INTRODUCED (this session)` — a divergence added while implementing another fix.

---

## 0. Summary

The build is faithful to the **outer shell** (repo/package shape, bootstrap
loader, daemon state machine, recovery levels 1/2/4, C ABI surface) and diverges
on nearly everything **load-bearing**: the hook engine, the injection mechanism,
symbol resolution, and the Logos toolchain. The cooperative-registry demo
concealed that the engine was never exercised.

Three forces produced the divergences: (1) "self-contained/offline" outranked the
spec (Dobby, Ghidra pipeline, symbol DB all skipped for zero-dependency
stand-ins); (2) the simpler-to-write option won over the safer-specified one
(rename-in-place vs bind-mount; no blacklist; registry vs real hook); (3) the
demo hid #1 and #2.

---

## 1. Divergence register

### D1 — Hook engine: Dobby not integrated
- **Spec:** `A§4.2` ("ARM/Thumb-2 relocator via vendored Dobby"), `A§12` Phase 1
  ("Dobby-sys + libksubstrate"); `plan:submodule-dobby`, `plan:dobby-sys`,
  `plan:ksubstrate-engine`.
- **Built:** vendored Dobby (pinned pre-refactor), `dobby-sys`, ARM backend
  delegates to `DobbyHook`/`DobbyDestroy`. Host keeps the mock backend.
- **Direction:** RESOLVED. **Severity:** was Critical. **Status:** DONE.
- **Remediation:** R1.

### D2 — PLT/GOT hooking (the *preferred* mechanism) missing
- **Spec:** `A§4.2` ("PLT/GOT hooking (preferred for stability)"), `A§9.3`
  ("Prefer stable surfaces: exported symbols and PLT boundaries"), invariant
  `A§14.10`.
- **Built:** `kh_hook_import` + native ELF jump-slot rewriter in
  `ksubstrate::plt`. Dobby's ImportTableReplace is Darwin-only and unused.
- **Direction:** RESOLVED (host-tested parser; device GOT rewrite pending a
  device run). **Severity:** was Critical. **Status:** DONE (code) / VERIFY.
- **Remediation:** R2.

### D3 — Inline relocation missing
- **Spec:** `A§4.2` ("Patch function prologue → trampoline → relocated
  instructions").
- **Built:** Dobby relocates ARM/Thumb-2 prologues (subsumes the old 8-byte
  hand-rolled patcher).
- **Direction:** RESOLVED (via R1). **Severity:** was Critical. **Status:** DONE.
- **Remediation:** R1.

### D4 — Symbol resolution by `base + RVA` missing
- **Spec:** `A§4.2` ("Requires known address (symbol DB + runtime base)"),
  `A§8.3`, `A§9`.
- **Built:** `kh_resolve_rva` (maps base + RVA); `kh_find_symbol` falls back to
  `DobbySymbolResolver` on-device; `kh_hook_function_checked` still gates patches.
- **Direction:** RESOLVED (runtime). Host symbol-DB extraction remains
  scaffolding. **Severity:** was Critical. **Status:** DONE (runtime) / OPEN
  (extraction).
- **Remediation:** R3.

### D5 — Injection mechanism chose the unsafe variant
- **Spec:** `A§6.3` — move binary aside **"(or bind-mount trick)"**; wrappers in
  `/var/local/kmc/wrappers/`. Bind-mount is volatile → satisfies invariant
  `A§14.1` ("hard reboot = clean boot") for free.
- **Built:** volatile bind-mount wrap/restore in `ksubstrated` (no rootfs rename).
- **Direction:** RESOLVED. **Severity:** was High. **Status:** DONE.
- **Remediation:** R4a.

### D6 — Blacklist not enforced (recovery-critical)
- **Spec:** `A§6.3`, `A§10` — never wrap `powerd`, `sshd`, `dbus`, OTA, storage,
  networking core.
- **Built:** `blacklisted_comm` enforced in filter-derived spawn-root resolution.
- **Direction:** RESOLVED. **Severity:** was High. **Status:** DONE.
- **Remediation:** R4b.

### D7 — Crash-loop guard incomplete
- **Spec:** `A§7` Level 3, invariant `A§14.7` — detect "pillow/home dying N times
  in T seconds".
- **Built:** UI-process health guard (falling edges of pillow/appmgrd) plus
  monitor-restart window.
- **Direction:** RESOLVED. **Severity:** was Medium. **Status:** DONE.
- **Remediation:** R4c.

### D8 — Tier-3 inheritance unhandled
- **Spec:** `A§6.4` — booklets inherit `LD_PRELOAD` from preloaded appmgrd; probe
  tweak logs bootstrap load in `/proc/$pid/maps`; add Tier-2 wrapper if env
  stripped.
- **Built:** inheritance probe ships under `diagnostics/` (opt-in `*` filter).
  Auto Tier-2 when env is stripped is still manual/docs.
- **Direction:** PARTIAL. **Severity:** Medium. **Status:** PARTIAL.
- **Remediation:** R4d.

### D9 — Demo masked the missing engine
- **Spec:** `plan:demo` — sample tweak *hooks* `compute`; show original-vs-hooked.
- **Built:** sample tweak calls `kh_hook_function` (inline) and `kh_hook_import`
  (GOT/`write`); demo target resolves `compute` dynamically.
- **Direction:** RESOLVED. **Severity:** was High. **Status:** DONE (device
  verify remaining).
- **Remediation:** folds into R1/R2.

### D10 — Logos missing its essential macros
- **Spec:** `A§8.3` — `%hookf(type, KSYM("name"), args) { … %orig; }`.
- **Built:** `ksub-logos` expands `KSYM`, `%hookf`, and `%orig` (single-line
  signature form). Still experimental, not a full Logos preprocessor.
- **Direction:** PARTIAL. **Severity:** Medium. **Status:** PARTIAL.
- **Remediation:** R5.

### D11 — Analysis toolchain overstated
- **Spec/plan:** symbol tooling explicitly **out of v1** ("documentation only,
  no tooling in v1"); `A§9.2` is a future pipeline.
- **Built:** `ksub pull/analyze/sym` shipped as commands emitting template YAML —
  presenting a pipeline that isn't there.
- **Direction:** OVERSTATED. **Severity:** Low (truthfulness).
- **Status:** PARTIAL (this session) — doc now marks them scaffolding.
- **Remediation:** R5.

### D12 — Repository split
- **Spec:** `A§2` — `kinstaller-repo` is "package distribution only"; runtime +
  toolchain *source* live in a separate `kindle-substrate` repo; only `.kpkg`
  artifacts land here.
- **Built:** runtime and toolchain source now live in `bd452/kindle-substrate`;
  `kinstaller-repo` consumes a pinned submodule and publishes the packages.
- **Direction:** RESOLVED. **Severity:** was Low. **Status:** DONE.
- **Remediation:** R6.

### D13 — Smaller spec deltas
- Tweak install path: `A§8.1` `/var/local/kmc/tweaks/` vs built default
  `…/packages/com.bd452.ksubstrate/tweaks` (two conventions). — R4.
- Header hand-written vs cbindgen-generated (`plan:ksubstrate-engine`). — R6.
- Bootstrap doesn't call the optional `ksubstrate_init` (`A§4.3`); relies on
  `.init_array` only. — R6.
- `ksub new` doesn't scaffold the KPM skeleton (`A§9.1`). — R5.
- **Direction:** mixed. **Severity:** Low. **Status:** OPEN.

### Faithful to spec (no action)
Bootstrap filter-match + `dlopen` with per-tweak fail-closed; USB sentinel
(`A§7` L2) checked first; enable/disable/toggle daemon shape; CLI `run`; package
wiring; C ABI + `MSHookFunction`/`MSFindSymbol` aliases; v1 non-goals respected
(no global preload, no ptrace, no power-button safe mode).

---

## 2. Remediation roadmap (priority order)

### R1 — Vendor Dobby as the inline backend  *(D1, D3, D9)*
- `plan:submodule-dobby`: add Dobby at `apps/com.bd452.ksubstrate/vendor/Dobby`
  + `.gitmodules` (mirror the FBInk submodule).
- `plan:dobby-sys`: `rust/dobby-sys` crate; `build.rs` builds `libdobby.a` via
  CMake against the koxtoolchain (`CROSS_TC`), static-links, bindgens `dobby.h`.
  Gate to the on-device cross build; `links = "dobby"`.
- Rewrite `backend.rs` `arm_linux` to call `DobbyHook`/`DobbyDestroy`; keep the
  host mock behind `#[cfg(not(arm))]`. `kh_hook_function` becomes a thin wrapper.
- Keep `kh_hook_function_checked`'s prologue verification in front of the patch.
- **Exit test:** demo shows `compute → 42` on-device via a real inline hook on a
  non-trivial (multi-instruction, PC-relative) prologue.

### R2 — PLT/GOT hooking behind the ABI  *(D2)*
- Add a GOT-rewrite path (parse `.rela.plt`/GOT of a loaded image, swap the entry,
  return the original) as the **preferred** mechanism per `A§4.2`.
- ABI: `kh_hook_import(image, symbol, repl, &orig)`. Implemented as a native ELF
  rewriter in `ksubstrate::plt` — Dobby's `ImportTableReplace` is Darwin/Mach-O
  only and is not used on Kindle Linux.
- **Exit test:** hook a libc/lipc call from the demo target via GOT, no inline
  patch involved.

### R3 — Symbol addressing + versioned DB (minimum viable)  *(D4)*
- Runtime: resolve a module's load base from `/proc/self/maps`; add
  `kh_resolve_rva(image, rva)` → `base + rva`; feed `kh_hook_function_checked`
  with a prologue from the DB.
- Host: `ksub-syms` already parses/emits the DB; add the `readelf --dyn-syms` /
  `nm -D` + imports + strings extraction the plan scoped for later, or (v1-honest)
  ship the manual Ghidra→RVA workflow as docs only.
- **Exit test:** hook one known non-exported function by name against a pinned
  firmware DB, with the prologue check refusing a mismatched build.

### R4 — Injection & session correctness  *(D5, D6, D7, D8, D13-path)*
- **R4a (bind-mount):** replace rename-in-place with a volatile bind-mount over
  each spawn root (wrapper script under `/var/local/kmc/…`, `mount --bind` onto
  the rootfs path). Restores `A§14.1`: reboot drops the mounts → clean boot.
  Fall back to rename only where bind-mount is unavailable, and add a boot-time
  reconciler if any persistent variant is kept.
- **R4b (blacklist):** enforce the `A§10` blacklist in `compute_spawn_roots` /
  `resolve_root_paths`; never wrap `powerd`, `sshd`, `dbus`, OTA, storage,
  networking core, regardless of `.ksfilter`. **Do this before shipping Tier-2.**
- **R4c (crash guard):** observe framework-process health (pillow/home respawn
  rate), not just monitor restarts.
- **R4d (inheritance):** ship the `A§6.4` probe tweak; document/auto-add a Tier-2
  wrapper when env is stripped.
- **R4e (paths):** converge tweak dir on `A§8.1` `/var/local/kmc/tweaks/`.

### R5 — Toolchain honesty + Logos  *(D10, D11, D13-new/scaffold)*
- Implement `%hookf` and `KSYM("name")` in `ksub-logos`, resolving against the DB
  from R3.
- Keep `pull/analyze/sym` clearly labeled scaffolding (done in the doc) until R3's
  extraction exists; make `ksub new` scaffold the KPM skeleton.

### R6 — Structure & process  *(D12, D13-header/init)*
- Decide and record: keep the monorepo (ratify the `A§2` deviation in
  [kindle-substrate.md](kindle-substrate.md)) **or** split runtime/toolchain into
  the separate `kindle-substrate` repo and publish only `.kpkg` here.
- Generate `ksubstrate.h` via cbindgen; have the bootstrap call the optional
  `ksubstrate_init` after `dlopen` per `A§4.3`.

---

## 3. Dependency order

```
R4b (blacklist)  ── ship immediately; it's a safety gate, no deps
R1 (Dobby) ───────┐
R3 (addressing) ──┼──► R5 (%hookf/KSYM needs R3)
R2 (PLT/GOT) ─────┘
R4a,R4c,R4d (session correctness) ── parallel to R1–R3
R6 (structure) ── any time; ratify before more source moves
```

Items that separate "loads code into a process I launch" from "Substrate-class
platform": **R1, R2, R3, and R4a/R4b.**

---

## 4. Already changed this session (context for reviewers)

- D9 demo rewired to real `kh_hook_function`; exposed the D1/D3 8-byte-window
  fragility.
- D7 crash-loop guard added (partial — monitor-restart based).
- Tier-2 filter-derived spawn roots added — **which introduced D6**; R4b must land
  with or before any release.
- `kh_hook_function_checked` length invariant, safe rename (dotted names),
  rollback-on-partial-wrap, unified log dir, comm-truncation filter match,
  default-launch wrapping, `ksub deploy` real copy, and doc honesty (D11) — see
  the working tree diff.

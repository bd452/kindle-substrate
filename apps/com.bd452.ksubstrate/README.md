# Kindle Substrate

KPM package for the Kindle Substrate runtime.

`build.sh` cross-compiles `libksubstrate.so`, `libksubstrate-bootstrap.so`,
`ksubstrated`, `ksubstrate`, and `ksubstrate.h` from the Rust workspace, then
stages them under `package/` before packing via `scripts/pack-app.sh`.

Inline hooks use the vendored [Dobby](https://github.com/jmpews/Dobby) engine
(git submodule at `vendor/Dobby`). PLT/GOT hooks use the runtime's native ELF
jump-slot rewriter (`kh_hook_import`). An optional inheritance probe ships under
`package/diagnostics/` (not auto-loaded).

Expected staged files (produced by `build.sh`, not committed):

```text
package/lib/<platform>/libksubstrate.so
package/lib/<platform>/libksubstrate-bootstrap.so
package/bin/<platform>/ksubstrated
package/bin/<platform>/ksubstrate
package/include/ksubstrate.h
package/diagnostics/com.bd452.ksubstrateprobe/
  lib/kindlehf/tweak.so
  lib/kindlepw2/tweak.so
```

The installed package creates a **Kindle Substrate** menu in KUAL for enabling,
disabling, reframing, and viewing the session status. If the demo package is
installed, the same menu includes **Run Substrate Demo**. See [`../../docs/kindle-substrate.md`](../../docs/kindle-substrate.md)
for the full architecture and package contract.

Build with the rest of the repo:

```bash
git submodule update --init --recursive
./scripts/build-in-container.sh   # or ./build.sh on Linux + koxtoolchain
```

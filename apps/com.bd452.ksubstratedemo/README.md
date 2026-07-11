# Kindle Substrate Demo

Self-contained demo package for validating Kindle Substrate without touching
Kindle framework processes.

The package depends on `com.bd452.ksubstrate`. `build.sh` cross-compiles the
demo target binary and sample tweak from the Rust workspace, then stages them
under `package/` before packing.

Expected staged files:

```text
package/bin/kindlehf/ksubstrate-demo-target
package/bin/kindlepw2/ksubstrate-demo-target
package/tweaks/com.bd452.ksubstratedemo/tweak.so
```

The checked-in manifest explicitly targets `ksubstrate-demo-target`, so the installed
runtime bootstrap should load the sample tweak only for that target process.

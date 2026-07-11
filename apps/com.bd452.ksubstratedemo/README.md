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
package/tweaks/com.bd452.ksubstratedemo/lib/kindlehf/tweak.so
package/tweaks/com.bd452.ksubstratedemo/lib/kindlepw2/tweak.so
```

The installer selects the device ABI and atomically registers that build as
`/var/local/ksubstrate/tweaks/com.bd452.ksubstratedemo/tweak.so`. The checked-in
manifest explicitly targets `ksubstrate-demo-target`, so the runtime bootstrap
loads the sample tweak only for that target process.

This remains the standalone generic-hook validation package. The fileless Home
test entry ships directly in `com.bd452.ksubstrate`, so installing this package
is not required to test Home-card discovery or launch dispatch.

# Step 1 — Pull UI binaries from the Kindle

Target strings: "Home", "Library" (bottom chrome).
Likely processes: pillow, appmgrd, home booklet.

## Connect

USBNetwork (typical):
  Host: 192.168.15.244
  User: root
  Password: mario  (or your device's)

Or mount USB storage and copy from a rootfs dump if you have one.

## Pull set (run from Mac once SSH works)

```bash
OUT=analysis/pulled
mkdir -p "$OUT"/{bin,lib,opt,etc,usr}

# Core UI binaries
scp root@192.168.15.244:/usr/bin/pillow "$OUT/bin/"
scp root@192.168.15.244:/usr/bin/appmgrd "$OUT/bin/" 2>/dev/null || true
scp root@192.168.15.244:/usr/sbin/pillow "$OUT/bin/pillow.sbin" 2>/dev/null || true

# Shared libs pillow is likely linked against
ssh root@192.168.15.244 'ldd /usr/bin/pillow 2>/dev/null || ldd /usr/sbin/pillow 2>/dev/null' \
  | tee "$OUT/pillow.ldd"

# Blanket: framework + lipc + localization-ish trees (can be large)
ssh root@192.168.15.244 'tar -C / -cf - \
  usr/lib/liblipc* usr/lib/libwebkit* usr/lib/libpillow* \
  opt/amazon/ebook opt/amazon/framework \
  usr/share/webkit-1.0 2>/dev/null' \
  | tar -C "$OUT" -xf -

# Also grab any file that literally contains the chrome strings
ssh root@192.168.15.244 'grep -rslF "Library" /usr/share /opt/amazon /var/local 2>/dev/null | head -80' \
  | tee "$OUT/string-hits.txt"
```

When that finishes, say so and we'll run `ksub analyze` + string/xref triage.

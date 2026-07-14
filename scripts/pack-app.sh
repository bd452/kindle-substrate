#!/usr/bin/env bash
# Stage, validate, pack, and verify one app with the shared Kindle KPM devkit.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: pack-app.sh <app-directory>" >&2
    exit 1
fi

APP_ROOT="$(cd "$1" && pwd)"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$APP_ROOT/dist/pkg"
OUTPUT_DIR="$APP_ROOT/dist"
PACK_DIR="$OUTPUT_DIR/.kpm-dev-pack"

rm -rf "$PKG_DIR" "$PACK_DIR"
mkdir -p "$PKG_DIR" "$PACK_DIR"

echo "==> Staging $(basename "$APP_ROOT") in $PKG_DIR"
cp -R "$APP_ROOT/package/." "$PKG_DIR/"

while IFS= read -r -d '' hook; do
    chmod +x "$hook"
done < <(find "$PKG_DIR" -maxdepth 1 -name '*.sh' -print0)

while IFS= read -r -d '' binary; do
    chmod +x "$binary"
done < <(find "$PKG_DIR/bin" -type f -print0 2>/dev/null || true)

"$REPO_ROOT/scripts/kpm-dev" validate "$PKG_DIR"
"$REPO_ROOT/scripts/kpm-dev" pack "$PKG_DIR" --output "$PACK_DIR"

artifact=""
for candidate in "$PACK_DIR"/*.kpkg; do
    [[ -e "$candidate" ]] || continue
    if [[ -n "$artifact" ]]; then
        echo "error: kpm-dev produced more than one package for $(basename "$APP_ROOT")" >&2
        exit 1
    fi
    artifact="$candidate"
done

if [[ -z "$artifact" ]]; then
    echo "error: kpm-dev did not produce a .kpkg for $(basename "$APP_ROOT")" >&2
    exit 1
fi

"$REPO_ROOT/scripts/kpm-dev" verify "$artifact"
mv "$artifact" "$OUTPUT_DIR/$(basename "$artifact")"
rm -rf "$PACK_DIR"

echo "==> Package ready: $OUTPUT_DIR/$(basename "$artifact")"

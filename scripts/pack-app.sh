#!/usr/bin/env bash
# Stage and pack one app from apps/<id>/package into apps/<id>/dist/.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: pack-app.sh <app-directory>" >&2
    exit 1
fi

APP_ROOT="$(cd "$1" && pwd)"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$APP_ROOT/dist/pkg"
OUTPUT_DIR="$APP_ROOT/dist"

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR" "$OUTPUT_DIR"

echo "==> Staging $(basename "$APP_ROOT") in $PKG_DIR"
cp -R "$APP_ROOT/package/." "$PKG_DIR/"

while IFS= read -r -d '' hook; do
    chmod +x "$hook"
done < <(find "$PKG_DIR" -maxdepth 1 -name '*.sh' -print0)

while IFS= read -r -d '' binary; do
    chmod +x "$binary"
done < <(find "$PKG_DIR/bin" -type f -print0 2>/dev/null || true)

python3 "$REPO_ROOT/scripts/pack_kpkg.py" "$PKG_DIR" "$OUTPUT_DIR"

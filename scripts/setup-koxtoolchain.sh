#!/usr/bin/env bash
# Download KindleModding koxtoolchain cross-compilers (Linux x86_64 host).
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "koxtoolchain host tools are Linux x86_64 binaries." >&2
    echo "Run ./build.sh inside Linux (OrbStack/Docker) or on a Linux machine." >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/koxtoolchain.sh
source "$REPO_ROOT/scripts/koxtoolchain.sh"

KOX_BASE="${KOXTOOLCHAIN_ROOT:-$HOME/x-tools}"
mkdir -p "$KOX_BASE"

download() {
    local name=$1
    if [[ -x "$(kox_gcc "$name")" ]] && "$(kox_gcc "$name")" --version >/dev/null 2>&1; then
        echo "==> $name already present under $KOX_BASE/x-tools"
        return 0
    fi
    local url="https://github.com/KindleModding/koxtoolchain/releases/latest/download/${name}.tar.gz"
    local archive="$KOX_BASE/.${name}.tar.gz.$$"
    echo "==> Downloading $name"
    rm -f "$archive"
    curl -fsSL --retry 5 --retry-all-errors --retry-delay 3 \
        --connect-timeout 30 --max-time 600 -o "$archive" "$url"
    tar -xzf "$archive" -C "$KOX_BASE"
    rm -f "$archive"
}

download kindlehf
download kindlepw2

echo
echo "koxtoolchain ready under $KOX_BASE/x-tools"

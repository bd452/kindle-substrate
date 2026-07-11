#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [[ -f .gitmodules ]]; then
    git submodule update --init --recursive
fi

for app in apps/com.bd452.ksubstrate apps/com.bd452.ksubstratedemo; do
    echo "==> Building $app"
    "$REPO_ROOT/$app/build.sh"
done

echo "Kindle Substrate packages are ready under apps/*/dist/."

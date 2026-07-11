#!/usr/bin/env bash
# Keep the mount layer structurally incapable of ordinary persistent mutation.
set -euo pipefail
file="$(cd "$(dirname "$0")/.." && pwd)/rust/ksubstrated/src/mounts.rs"
if rg -n 'fs::(write|copy|rename|remove_file)|OpenOptions::(.*truncate|.*create\()' "$file"; then
    echo "ordinary mutation API found in mounts.rs" >&2
    exit 1
fi

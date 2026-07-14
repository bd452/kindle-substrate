#!/usr/bin/env bash
# Generate the registry-facing descriptor for the coupled runtime/demo release.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
    cat >&2 <<'EOF'
Usage: generate-release-metadata.sh \
  --base-url URL --repository OWNER/REPO --commit SHA --tag TAG --output FILE
EOF
    exit 2
}

base_url=""
repository=""
commit=""
tag=""
output=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --base-url) [[ $# -ge 2 ]] || usage; base_url=$2; shift 2 ;;
        --repository) [[ $# -ge 2 ]] || usage; repository=$2; shift 2 ;;
        --commit) [[ $# -ge 2 ]] || usage; commit=$2; shift 2 ;;
        --tag) [[ $# -ge 2 ]] || usage; tag=$2; shift 2 ;;
        --output) [[ $# -ge 2 ]] || usage; output=$2; shift 2 ;;
        *) usage ;;
    esac
done

[[ -n "$base_url" && -n "$repository" && -n "$commit" && -n "$tag" && -n "$output" ]] || usage

artifacts=()
for package_id in com.bd452.ksubstrate com.bd452.ksubstratedemo; do
    found=""
    for candidate in "$REPO_ROOT/apps/$package_id/dist/$package_id"_*.kpkg; do
        [[ -e "$candidate" ]] || continue
        if [[ -n "$found" ]]; then
            echo "error: multiple release artifacts found for $package_id; clean apps/$package_id/dist" >&2
            exit 1
        fi
        found="$candidate"
    done
    if [[ -z "$found" ]]; then
        echo "error: missing release artifact for $package_id" >&2
        exit 1
    fi
    artifacts+=("$found")
done

mkdir -p "$(dirname "$output")"
"$REPO_ROOT/scripts/kpm-dev" release-metadata "${artifacts[@]}" \
    --base-url "$base_url" \
    --repository "$repository" \
    --commit "$commit" \
    --tag "$tag" \
    --output "$output"

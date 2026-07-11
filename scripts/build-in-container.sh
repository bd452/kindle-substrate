#!/usr/bin/env bash
# Build this repo inside the kindle-substrate-build Docker image (linux/amd64).
#
# Use this on macOS (or any host that is not Linux x86_64): the KindleModding
# koxtoolchain binaries only run on Linux amd64.
#
# Usage (from repo root):
#   ./scripts/build-in-container.sh                  # full ./build.sh
#   ./scripts/build-in-container.sh apps/com.bd452.ksubstratedemo/build.sh
#   ./scripts/build-in-container.sh bash              # interactive shell
#   ./scripts/build-in-container.sh bash -lc '…'      # custom command
#
# First run builds the image (several minutes: apt + rustup + toolchain tarballs).
# Later runs reuse kindle-substrate-build:latest. If the local image predates this
# helper's expected toolchain, it is rebuilt automatically.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${KSUBSTRATE_BUILD_IMAGE:-kindle-substrate-build:latest}"
PLATFORM="${KSUBSTRATE_BUILD_PLATFORM:-linux/amd64}"

cd "$REPO_ROOT"

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker not found on PATH (Docker Desktop, OrbStack, or Podman)" >&2
    exit 1
fi

image_needs_rebuild() {
    if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
        return 0
    fi

    # Keep this cheap and explicit: bindgen requires libclang, and the build
    # image should include rustfmt so generated bindings can be formatted.
    if ! docker run --rm --platform "$PLATFORM" "$IMAGE" bash -lc \
        'command -v clang >/dev/null &&
         command -v cmake >/dev/null &&
         find /usr -name "libclang.so*" -o -name "libclang-*.so*" 2>/dev/null | grep -q . &&
         rustfmt --version >/dev/null' >/dev/null 2>&1; then
        echo "==> Existing $IMAGE is missing build prerequisites; rebuilding"
        return 0
    fi

    return 1
}

# Podman compatibility: `docker` may be a podman shim; --platform still applies.
if image_needs_rebuild; then
    echo "==> Building $IMAGE ($PLATFORM) from Dockerfile"
    docker build --platform "$PLATFORM" -t "$IMAGE" .
fi

if [[ $# -eq 0 ]]; then
    set -- ./build.sh
fi

# Persist cargo target dir on the host so rebuilds stay warm across containers.
# Separate from rust/target (host tests) via CARGO_TARGET_DIR.
mkdir -p "$REPO_ROOT/rust/target-kindle"

echo "==> docker run $IMAGE — $*"

# -it only when we have a TTY (agents/CI often don't). Keep separate branches:
# older Bash + `set -u` can treat an empty array expansion as unbound.
if [[ -t 0 && -t 1 ]]; then
    exec docker run --rm -it \
        --platform "$PLATFORM" \
        -v "$REPO_ROOT":/repo \
        -e KOXTOOLCHAIN_ROOT=/opt/x-tools \
        -e CARGO_TARGET_DIR=/repo/rust/target-kindle \
        -w /repo \
        "$IMAGE" \
        "$@"
else
    exec docker run --rm \
        --platform "$PLATFORM" \
        -v "$REPO_ROOT":/repo \
        -e KOXTOOLCHAIN_ROOT=/opt/x-tools \
        -e CARGO_TARGET_DIR=/repo/rust/target-kindle \
        -w /repo \
        "$IMAGE" \
        "$@"
fi

# Shared Kindle cross-build environment. Product-specific compile commands stay
# in this repository; the compiler/toolchain image is versioned independently.
ARG KPM_BUILD_IMAGE=ghcr.io/bd452/kindle-kpm-build:v0.1.0
FROM ${KPM_BUILD_IMAGE}

WORKDIR /work

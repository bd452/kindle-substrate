# Shared Kindle cross-build environment. Product-specific compile commands stay
# in this repository; the compiler/toolchain image is versioned independently.
ARG KPM_BUILD_IMAGE=ghcr.io/bd452/kindle-kpm-build:v0.1.0@sha256:c7bd7e4041717bb16765b97d6fe4f578f40d144fa3628fcad81271e22f18a69b
FROM ${KPM_BUILD_IMAGE}

WORKDIR /work

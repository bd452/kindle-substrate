# Cross-build image for this repo (Linux x86_64 toolchains + Rust).
#
# Build (from the repo root; always force amd64 on Apple Silicon):
#   docker build --platform linux/amd64 -t kinstaller-build .
#
# See README.md for build recipes.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        curl \
        file \
        git \
        libclang-dev \
        llvm-dev \
        make \
        python3 \
        wget \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable) + Kindle ARM targets.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/rustup/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup target add \
        armv7-unknown-linux-gnueabihf \
        armv7-unknown-linux-gnueabi \
    && rustup component add rustfmt

# KindleModding koxtoolchain. Release tarballs extract an `x-tools/` directory,
# so after unpacking into /opt/x-tools the gcc path is:
#   /opt/x-tools/x-tools/<prefix>/bin/<prefix>-gcc
# scripts/koxtoolchain.sh joins $KOXTOOLCHAIN_ROOT/x-tools/<prefix>/..., so the
# root must be /opt/x-tools (NOT /opt/x-tools/x-tools).
ENV KOXTOOLCHAIN_ROOT=/opt/x-tools

RUN mkdir -p /opt/x-tools \
    && wget -q https://github.com/KindleModding/koxtoolchain/releases/latest/download/kindlehf.tar.gz -O - \
        | tar -xzf - -C /opt/x-tools \
    && wget -q https://github.com/KindleModding/koxtoolchain/releases/latest/download/kindlepw2.tar.gz -O - \
        | tar -xzf - -C /opt/x-tools \
    && /opt/x-tools/x-tools/arm-kindlehf-linux-gnueabihf/bin/arm-kindlehf-linux-gnueabihf-gcc --version \
    && /opt/x-tools/x-tools/arm-kindlepw2-linux-gnueabi/bin/arm-kindlepw2-linux-gnueabi-gcc --version

WORKDIR /work

#!/usr/bin/env bash
set -eux

# Build the `lit` CLI inside an OLD-glibc Debian container so the resulting
# binary loads on hosts with an older glibc than the CI runner.
#
# The GitHub `ubuntu-22.04` runners ship glibc 2.35, which bakes a `GLIBC_2.35`
# symbol requirement into the binary and breaks it on AWS Lambda / Amazon Linux
# 2023 (glibc 2.34) and other older hosts. Debian bullseye ships glibc 2.31 —
# below the 2.34 Lambda floor — and glibc is backward-compatible, so a
# 2.31-baseline binary still runs on newer hosts. This only WIDENS
# compatibility; it is not a breaking change.
#
# Mirrors scripts/build-glibc-node.sh but produces the standalone CLI via
# `cargo build` instead of the napi module. tesseract-rs (build-tesseract)
# compiles Tesseract + Leptonica from source; on linux-gnu it uses
# g++/libstdc++, so no clang/libc++ bundling is required.

TARGET="${1:?usage: build-glibc-cli.sh <rust-target>}"

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
  build-essential cmake git curl pkg-config ca-certificates \
  libtesseract-dev libleptonica-dev \
  libpng-dev libjpeg-dev libtiff-dev zlib1g-dev

curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain 1.95.0 -t "$TARGET"
. "$HOME/.cargo/env"

cargo build --release --target "$TARGET" -p liteparse

BIN="target/$TARGET/release/lit"
echo "Built CLI: $BIN"
ls -la "$BIN"
echo "glibc symbol versions required by $BIN:"
objdump -T "$BIN" 2>/dev/null \
  | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -u -V || true

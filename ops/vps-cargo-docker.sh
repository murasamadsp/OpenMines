#!/usr/bin/env bash
# Вызывается внутри: docker run -v $REMOTE_DIR:/build -w /build rust:1.89-bookworm bash /build/ops/vps-cargo-docker.sh
set -euo pipefail
cd /build
export CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
cargo build --release
cp -f target/release/openmines-server /build/openmines-server
chmod 755 /build/openmines-server

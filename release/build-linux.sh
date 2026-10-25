#!/usr/bin/env bash
# The Linux release binary, built in the Steam Runtime 3 "sniper" SDK (glibc 2.31) so one build runs on every current
# distro: target/steamrt/dist/vana-client, copied to target/assets/vana-client-linux-x86_64 (the asset name the in-app
# Update looks for). The SDK has no Rust: the host's stable rustup toolchain (its binaries need only glibc 2.17) and
# cargo registry are mounted in and called directly (not through a rustup proxy, which may need the host's glibc).
# Cores 12..15 stay free (the LSB map server's watchdog).
set -euo pipefail
cd "$(dirname "$0")/.."
image=registry.gitlab.steamos.cloud/steamrt/sniper/sdk:latest
toolchain=/rustup/toolchains/stable-x86_64-unknown-linux-gnu
mkdir -p "$HOME/.cargo"
docker run --rm --cpuset-cpus 0-11 --user "$(id -u):$(id -g)" \
    -v "$PWD:/src" -v "$HOME/.cargo:/cargo" -v "$HOME/.rustup:/rustup:ro" \
    -e HOME=/tmp -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/steamrt -e PATH="$toolchain/bin:/usr/bin:/bin" \
    -e LIBCLANG_PATH=/usr/lib/llvm-11/lib -e RUSTFLAGS="-C target-cpu=x86-64-v2" \
    -w /src "$image" cargo build --profile dist
mkdir -p target/assets
cp target/steamrt/dist/vana-client target/assets/vana-client-linux-x86_64
echo "target/assets/vana-client-linux-x86_64"

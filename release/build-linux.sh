#!/usr/bin/env bash
# The Linux release binary for this machine's architecture (x86_64 here, aarch64 on the release workflow's ARM
# runner), built in the Steam Runtime 3 "sniper" SDK (glibc 2.31) so one build runs on every current distro:
# target/steamrt/dist/vana-client, copied to target/assets/vana-client-linux-ARCH (the asset name the in-app Update
# looks for). The SDK has no Rust: the host's stable rustup toolchain (its binaries need only glibc 2.17) and cargo
# registry are mounted in and called directly (not through a rustup proxy, which may need the host's glibc).
# VANA_CPUSET: the cores the build may use (default 0-11, leaving 12..15 to the LSB map server's watchdog; empty: all).
set -euo pipefail
cd "$(dirname "$0")/.."
arch=$(uname -m)
case "$arch" in
    x86_64)
        image=registry.gitlab.steamos.cloud/steamrt/sniper/sdk:latest
        rustflags="-C target-cpu=x86-64-v2"
        ;;
    aarch64)
        image=registry.gitlab.steamos.cloud/steamrt/sniper/sdk/arm64:latest
        rustflags=""
        ;;
    *)
        echo "no release build for $arch" >&2
        exit 1
        ;;
esac
toolchain=/rustup/toolchains/stable-$arch-unknown-linux-gnu
cpuset="${VANA_CPUSET-0-11}"
cpus=()
[ -n "$cpuset" ] && cpus=(--cpuset-cpus "$cpuset")
mkdir -p "$HOME/.cargo"
docker run --rm "${cpus[@]}" --user "$(id -u):$(id -g)" \
    -v "$PWD:/src" -v "$HOME/.cargo:/cargo" -v "$HOME/.rustup:/rustup:ro" \
    -e HOME=/tmp -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/steamrt -e PATH="$toolchain/bin:/usr/bin:/bin" \
    -e LIBCLANG_PATH=/usr/lib/llvm-11/lib -e RUSTFLAGS="$rustflags" \
    -w /src "$image" cargo build --profile dist --locked
mkdir -p target/assets
cp target/steamrt/dist/vana-client "target/assets/vana-client-linux-$arch"
echo "target/assets/vana-client-linux-$arch"

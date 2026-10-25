#!/usr/bin/env bash
# The Windows release binary, cross-built with MinGW (rustup target x86_64-pc-windows-gnu, x86_64-w64-mingw32-gcc):
# target/assets/vana-client-windows-x86_64.exe, the asset name the in-app Update looks for. The C runtime is linked in
# (+crt-static): without it the exe needs MinGW's libwinpthread-1.dll, which Windows does not have.
set -euo pipefail
cd "$(dirname "$0")/.."
RUSTFLAGS="-C target-cpu=x86-64-v2 -C target-feature=+crt-static" nice -n 19 taskset -c 0-11 cargo build --profile dist --target x86_64-pc-windows-gnu
mkdir -p target/assets
cp target/x86_64-pc-windows-gnu/dist/vana-client.exe target/assets/vana-client-windows-x86_64.exe
echo "target/assets/vana-client-windows-x86_64.exe"

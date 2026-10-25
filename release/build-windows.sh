#!/usr/bin/env bash
# The Windows release binary, cross-built with MinGW (rustup target x86_64-pc-windows-gnu, x86_64-w64-mingw32-gcc):
# target/assets/vana-client-windows-x86_64.exe, the asset name the in-app Update looks for. Everything is linked in
# (+crt-static, -static). unrar_sys asks for a dynamic -lpthread on a Linux host, so a static copy of winpthread sits
# first in the search path under that name: otherwise the exe needs libwinpthread-1.dll, which Windows does not have.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target/winstatic
cp "$(x86_64-w64-mingw32-gcc -print-file-name=libwinpthread.a)" target/winstatic/libpthread.a
RUSTFLAGS="-C target-cpu=x86-64-v2 -C target-feature=+crt-static -C link-arg=-static -L native=$PWD/target/winstatic" \
    nice -n 19 taskset -c 0-11 cargo build --profile dist --target x86_64-pc-windows-gnu
if x86_64-w64-mingw32-objdump -p target/x86_64-pc-windows-gnu/dist/vana-client.exe | grep -qi "DLL Name: libwinpthread"; then
    echo "the exe still imports libwinpthread-1.dll" >&2
    exit 1
fi
mkdir -p target/assets
cp target/x86_64-pc-windows-gnu/dist/vana-client.exe target/assets/vana-client-windows-x86_64.exe
echo "target/assets/vana-client-windows-x86_64.exe"

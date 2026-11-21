#!/usr/bin/env bash
# Cross-compile libvana_client.so for arm64 and package target/assets/vana-client-android-aarch64.apk (sideload with
# adb install -r). VANA_ANDROID_APK_VARIANT=release signs with the keystore in android/local.properties or the
# VANA_ANDROID_KEYSTORE* variables (android/README.md), else the debug key.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.1.12297006}"
export TMPDIR="${TMPDIR:-$ROOT/target/tmp}"
export TMP="$TMPDIR"
export TEMP="$TMPDIR"
mkdir -p "$TMPDIR"

STUB_DIR="$ROOT/release/android-link-stubs"
NDK_PREBUILT="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt"
NDK_HOST="$(ls -1 "$NDK_PREBUILT" 2>/dev/null | head -1 || true)"
if [[ -z "$NDK_HOST" ]]; then
  echo "NDK llvm prebuilt not found under $NDK_PREBUILT" >&2
  exit 1
fi
NDK_BIN="$NDK_PREBUILT/$NDK_HOST/bin"
if [[ ! -x "$NDK_BIN/clang" ]]; then
  echo "NDK clang missing: $NDK_BIN/clang (set ANDROID_NDK_HOME to a full SDK NDK install)" >&2
  exit 1
fi
if [[ ! -f "$STUB_DIR/libpthread.a" ]]; then
  "$NDK_BIN/aarch64-linux-android26-clang" -c -o "$STUB_DIR/pthread_stub.o" "$STUB_DIR/pthread_stub.c"
  "$NDK_BIN/llvm-ar" rcs "$STUB_DIR/libpthread.a" "$STUB_DIR/pthread_stub.o"
fi
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$NDK_BIN/aarch64-linux-android26-clang++"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android26-clang"
SYSROOT_LIB="$NDK_PREBUILT/$NDK_HOST/sysroot/usr/lib/aarch64-linux-android/26"
# unrar_sys (C++) needs libc++ at load time (else dlopen fails on std::length_error's typeinfo); it also asks for
# -lpthread, which bionic folds into libc, so an empty libpthread.a stands in.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-L$STUB_DIR -C link-arg=-L$SYSROOT_LIB -C link-arg=-lc++_shared"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"

apply_android_crate_patches() {
  local registry="${CARGO_HOME:-$HOME/.cargo}/registry/src" patched=0

  # unrar_sys: bionic has no lutimes.
  local ULINKS
  while IFS= read -r ULINKS; do
    [[ -z "$ULINKS" ]] && continue
    if grep -q 'lutimes' "$ULINKS"; then
      sed -i 's/lutimes(/utimes(/g' "$ULINKS"
      patched=1
    fi
  done < <(find "$registry" -path '*/unrar_sys-*/vendor/unrar/ulinks.cpp' 2>/dev/null)

  # unrar_sys: OpenArchiveDataEx must use ArcName (UTF-8) on Android, not ArcNameW (do not touch link lines).
  local UNRAR_SYS_LIB
  while IFS= read -r UNRAR_SYS_LIB; do
    [[ -z "$UNRAR_SYS_LIB" ]] && continue
    if grep -q 'target_os = "android"' "$UNRAR_SYS_LIB"; then
      continue
    fi
    python3 - "$UNRAR_SYS_LIB" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
o = '    #[cfg(any(target_os = "linux", target_os = "netbsd"))]\n    pub fn new(archive: *const c_char, mode: c_uint) -> Self {'
n = '    #[cfg(any(target_os = "linux", target_os = "netbsd", target_os = "android"))]\n    pub fn new(archive: *const c_char, mode: c_uint) -> Self {'
o2 = '    #[cfg(not(any(target_os = "linux", target_os = "netbsd")))]\n    pub fn new(archive: *const wchar_t, mode: c_uint) -> Self {'
n2 = '    #[cfg(not(any(target_os = "linux", target_os = "netbsd", target_os = "android")))]\n    pub fn new(archive: *const wchar_t, mode: c_uint) -> Self {'
if o not in text:
    sys.exit("unrar_sys lib.rs changed; update android patch")
open(path, "w").write(text.replace(o, n, 1).replace(o2, n2, 1))
PY
    patched=1
  done < <(find "$registry" -path '*/unrar_sys-0.5.8/src/lib.rs' 2>/dev/null)

  # A build of unrar_sys from before the patches would be reused.
  if [[ "$patched" == 1 ]]; then
    rm -rf "$ROOT"/target/aarch64-linux-android/release/build/unrar_sys-*
  fi
}

cd "$ROOT"
unset CARGO_TARGET_DIR
export ANDROID_NDK_HOME
if ! rustup target list --installed | grep -q '^aarch64-linux-android$'; then
  rustup target add aarch64-linux-android
fi

echo "Fetching dependencies (the registry patches need the crates on disk)..."
cargo fetch --target aarch64-linux-android --locked
apply_android_crate_patches

echo "Building libvana_client.so (release, arm64, no pipewire)..."
cargo ndk -t arm64-v8a build --release -p vana-client --lib --no-default-features

SO="$ROOT/target/aarch64-linux-android/release/libvana_client.so"
JNI="$ROOT/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$JNI"
cp -f "$SO" "$JNI/libvana_client.so"
CXX_SHARED="$(find "$ANDROID_NDK_HOME" -path '*/aarch64-linux-android/libc++_shared.so' 2>/dev/null | head -1)"
if [[ -z "${CXX_SHARED:-}" ]]; then
  echo "libc++_shared.so not found under ANDROID_NDK_HOME" >&2
  exit 1
fi
cp -f "$CXX_SHARED" "$JNI/libc++_shared.so"

if [[ -z "${GRADLE:-}" ]]; then
  GRADLE="$(command -v gradle || ls -1d "$HOME"/.gradle/wrapper/dists/gradle-*-bin/*/gradle-*/bin/gradle 2>/dev/null | sort -V | tail -1 || true)"
fi
if [[ -z "$GRADLE" ]] || ! command -v "$GRADLE" >/dev/null 2>&1; then
  echo "Gradle not found (install Gradle 8.9+ or set GRADLE); got: $GRADLE" >&2
  exit 1
fi

export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-17-temurin-jdk}"

VARIANT="${VANA_ANDROID_APK_VARIANT:-debug}"
if [[ "$VARIANT" == "release" ]]; then
  GRADLE_TASK=assembleRelease
  APK_SUB=release/app-release.apk
else
  GRADLE_TASK=assembleDebug
  APK_SUB=debug/app-debug.apk
fi

echo "Packaging APK ($VARIANT)..."
cd "$ROOT/android"
"$GRADLE" "$GRADLE_TASK"

mkdir -p "$ROOT/target/assets"
APK="$ROOT/target/assets/vana-client-android-aarch64.apk"
cp "$ROOT/android/app/build/outputs/apk/$APK_SUB" "$APK"
echo ""
echo "APK ready: $APK"
echo "Install on a connected device: adb install -r \"$APK\""

#!/usr/bin/env bash
# Create a sideload release keystore (not Play upload). Private material stays outside the repo.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG_DIR="${VANA_ANDROID_CONFIG:-$HOME/.config/vana-client-android}"
KEYSTORE="${VANA_ANDROID_KEYSTORE:-$CONFIG_DIR/release.keystore}"
ALIAS="${VANA_ANDROID_KEY_ALIAS:-vana-client}"
LOCAL_PROPS="$ROOT/android/local.properties"
VALIDITY_DAYS=10000

force=false
for arg in "$@"; do
  case "$arg" in
    --force) force=true ;;
    -h|--help)
      echo "Usage: android/gen-signing-key.sh [--force]"
      echo "Writes keystore to $KEYSTORE and android/local.properties (gitignored)."
      exit 0
      ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

if [[ -f "$KEYSTORE" && "$force" != true ]]; then
  echo "Keystore already exists: $KEYSTORE" >&2
  echo "Re-run with --force to replace it (you cannot upgrade installs signed with the old key)." >&2
  exit 1
fi

if ! command -v keytool >/dev/null 2>&1; then
  echo "keytool not found; install a JDK (e.g. java-17-openjdk) and try again." >&2
  exit 1
fi

mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"

if [[ -n "${VANA_ANDROID_KEYSTORE_PASSWORD:-}" ]]; then
  store_pass="$VANA_ANDROID_KEYSTORE_PASSWORD"
else
  store_pass="$(openssl rand -base64 24)"
fi
if [[ -n "${VANA_ANDROID_KEY_PASSWORD:-}" ]]; then
  key_pass="$VANA_ANDROID_KEY_PASSWORD"
else
  key_pass="$store_pass"
fi

if [[ "$force" == true && -f "$KEYSTORE" ]]; then
  rm -f "$KEYSTORE"
fi

keytool -genkeypair -v \
  -keystore "$KEYSTORE" \
  -alias "$ALIAS" \
  -keyalg RSA -keysize 2048 -validity "$VALIDITY_DAYS" \
  -storepass "$store_pass" \
  -keypass "$key_pass" \
  -dname "CN=Vana Client Android, OU=Sideload, O=Vana Client"

chmod 600 "$KEYSTORE"

# Preserve unrelated local.properties keys (sdk.dir, etc.)
touch "$LOCAL_PROPS"
chmod 600 "$LOCAL_PROPS"
python3 - "$LOCAL_PROPS" "$KEYSTORE" "$ALIAS" "$store_pass" "$key_pass" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
keystore, alias, store_pass, key_pass = sys.argv[2:6]
lines = path.read_text().splitlines() if path.exists() else []
keys = {
    "vana.keystore": keystore,
    "vana.key.alias": alias,
    "vana.keystore.password": store_pass,
    "vana.key.password": key_pass,
}
out = []
seen = set()
for line in lines:
    if not line.strip() or line.strip().startswith("#"):
        out.append(line)
        continue
    k = line.split("=", 1)[0].strip()
    if k in keys:
        out.append(f"{k}={keys[k]}")
        seen.add(k)
    else:
        out.append(line)
for k, v in keys.items():
    if k not in seen:
        out.append(f"{k}={v}")
path.write_text("\n".join(out) + "\n")
PY

echo "Keystore:     $KEYSTORE"
echo "Alias:        $ALIAS"
echo "Gradle props: $LOCAL_PROPS"
echo ""
echo "Back up the keystore and passwords somewhere safe. Losing them means users must uninstall before installing a newly signed APK."
if [[ -z "${VANA_ANDROID_KEYSTORE_PASSWORD:-}" ]]; then
  echo ""
  echo "Generated passwords (also in local.properties):"
  echo "  store: $store_pass"
  echo "  key:   $key_pass"
fi
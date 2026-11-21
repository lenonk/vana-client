#!/usr/bin/env bash
# Regenerate launcher mipmaps from crates/vana-client/assets/icon-256.png (same as desktop / taskbar).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/vana-client/assets/icon-256.png"
RES="$ROOT/android/app/src/main/res"
command -v magick >/dev/null || { echo "ImageMagick (magick) required" >&2; exit 1; }

mkdir -p "$RES/mipmap-mdpi" "$RES/mipmap-hdpi" "$RES/mipmap-xhdpi" \
  "$RES/mipmap-xxhdpi" "$RES/mipmap-xxxhdpi" "$RES/mipmap-anydpi-v26" "$RES/values"

for pair in 48:mdpi 72:hdpi 96:xhdpi 144:xxhdpi 192:xxxhdpi; do
  sz="${pair%%:*}"; dir="${pair##*:}"
  magick "$SRC" -filter Lanczos -resize "${sz}x${sz}" "$RES/mipmap-$dir/ic_launcher.png"
  cp -f "$RES/mipmap-$dir/ic_launcher.png" "$RES/mipmap-$dir/ic_launcher_round.png"
done

# Adaptive layers (canvas:inner ≈ 80% safe zone)
for spec in mdpi:108:86 hdpi:162:129 xhdpi:216:173 xxhdpi:324:259 xxxhdpi:432:346; do
  IFS=: read -r dir canvas inner <<< "$spec"
  magick "$SRC" -filter Lanczos -resize "${inner}x${inner}" -background none -gravity center \
    -extent "${canvas}x${canvas}" "$RES/mipmap-$dir/ic_launcher_foreground.png"
  magick "$SRC" -colorspace Gray -filter Lanczos -resize "${inner}x${inner}" -background none \
    -gravity center -extent "${canvas}x${canvas}" "$RES/mipmap-$dir/ic_launcher_monochrome.png"
done

echo "Wrote launcher icons under $RES"
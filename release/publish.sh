#!/usr/bin/env bash
# Publishes a GitHub release vVERSION of lenonk/vana-client with the assets release/build-*.sh made, and those
# release/fetch-ci-builds.sh downloaded (Linux aarch64, macOS) when present, each with its .sig. VERSION must be the
# crate's version (crates/vana-client/Cargo.toml), newer than the last release: clients offer the update when the tag
# is newer and its signature checks against release/signing-key.pub, built in.
# The private key: VANA_SIGNING_KEY, or ~/.config/vana-client-release/signing.pem (Ed25519; never in the repo).
#   release/publish.sh 0.1.1 [notes file]
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: release/publish.sh VERSION [NOTES]}"
crate=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/vana-client/Cargo.toml | head -1)
[ "$crate" = "$version" ] || { echo "Cargo.toml says $crate, not $version" >&2; exit 1; }
assets=(target/assets/vana-client-linux-x86_64 target/assets/vana-client-windows-x86_64.exe)
for a in target/assets/vana-client-linux-aarch64 target/assets/vana-client-macos-aarch64 target/assets/vana-client-macos-x86_64; do
    if [ -f "$a" ]; then assets+=("$a"); else echo "note: no $a (release/fetch-ci-builds.sh); left out" >&2; fi
done
key="${VANA_SIGNING_KEY:-$HOME/.config/vana-client-release/signing.pem}"
[ -f "$key" ] || { echo "no signing key at $key" >&2; exit 1; }
pub=$(tr -d '[:space:]' < release/signing-key.pub)
[ "$(openssl pkey -in "$key" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n')" = "$pub" ] \
    || { echo "$key is not the key of release/signing-key.pub" >&2; exit 1; }
sigs=()
for a in "${assets[@]}"; do
    [ -f "$a" ] || { echo "missing $a (run release/build-linux.sh and release/build-windows.sh)" >&2; exit 1; }
    # A binary built without this key could never update itself to the next release.
    grep -a -q "$pub" "$a" || { echo "$a was not built with release/signing-key.pub" >&2; exit 1; }
    # Its updater's user agent: an asset left from another version is never signed as this one.
    grep -a -q "vana-client/$version" "$a" || { echo "$a is not version $version" >&2; exit 1; }
    # What the client checks (updater::signed_text): the tag, the asset's name, its SHA-256.
    text="$a.text"
    printf 'vana-client release\nv%s\n%s\n%s\n' "$version" "$(basename "$a")" "$(sha256sum "$a" | cut -d' ' -f1)" > "$text"
    openssl pkeyutl -sign -rawin -inkey "$key" -in "$text" -out "$a.sig"
    rm -f "$text"
    sigs+=("$a.sig")
done
notes=(--generate-notes)
[ -n "${2:-}" ] && notes=(--notes-file "$2")
git tag "v$version"
git push origin "v$version"
gh release create "v$version" -R lenonk/vana-client --title "vana-client $version" "${notes[@]}" "${assets[@]}" "${sigs[@]}"

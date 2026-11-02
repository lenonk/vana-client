#!/usr/bin/env bash
# Publishes a GitHub release vVERSION of akreao/vana-client (the repo the updater asks, updater.rs RELEASE_REPO) with the
# assets release/build-*.sh made, each with a <asset>.sig: its Ed25519 signature by the release key, which the updater
# checks against release/signing-key.pub (built in) before installing. The private key is VANA_SIGNING_KEY, by default
# ~/.config/vana-client-release/signing-key.pem; it never goes in the repository. VERSION must be
# the crate's version (crates/vana-client/Cargo.toml), newer than the last release: clients compare the tag with their
# own version and offer the update when it is newer.
#   release/publish.sh 0.1.1 [notes file]
set -euo pipefail
cd "$(dirname "$0")/.."
version="${1:?usage: release/publish.sh VERSION [NOTES]}"
crate=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/vana-client/Cargo.toml | head -1)
[ "$crate" = "$version" ] || { echo "Cargo.toml says $crate, not $version" >&2; exit 1; }
assets=(target/assets/vana-client-linux-x86_64 target/assets/vana-client-windows-x86_64.exe)
for a in "${assets[@]}"; do
    [ -f "$a" ] || { echo "missing $a (run release/build-linux.sh and release/build-windows.sh)" >&2; exit 1; }
done
key="${VANA_SIGNING_KEY:-$HOME/.config/vana-client-release/signing-key.pem}"
[ -f "$key" ] || { echo "missing the release signing key $key (VANA_SIGNING_KEY)" >&2; exit 1; }
# The key must be the one the binaries were built with, or no client would accept the release.
pub=$(openssl pkey -in "$key" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n')
[ "$pub" = "$(tr -d ' \n' < release/signing-key.pub)" ] || { echo "$key is not the key in release/signing-key.pub" >&2; exit 1; }
sigs=()
for a in "${assets[@]}"; do
    openssl pkeyutl -sign -rawin -inkey "$key" -in "$a" -out "$a.sig"
    sigs+=("$a.sig")
done
notes=(--generate-notes)
[ -n "${2:-}" ] && notes=(--notes-file "$2")
git tag "v$version"
git push origin "v$version"
gh release create "v$version" -R akreao/vana-client --title "vana-client $version" "${notes[@]}" "${assets[@]}" "${sigs[@]}"

#!/usr/bin/env bash
# Publishes a GitHub release vVERSION of lenonk/vana-client with the assets release/build-*.sh made. VERSION must be
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
notes=(--generate-notes)
[ -n "${2:-}" ] && notes=(--notes-file "$2")
git tag "v$version"
git push origin "v$version"
gh release create "v$version" -R lenonk/vana-client --title "vana-client $version" "${notes[@]}" "${assets[@]}"

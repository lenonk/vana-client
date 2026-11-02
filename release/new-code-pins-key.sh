#!/usr/bin/env bash
# Makes a new code-pins key (Ed25519): its public half goes in release/code-pins-key.pub (built into the client),
# its private half becomes the CODE_PINS_KEY secret of akreao/vana-client for the Code pins workflow and is then
# deleted here. It is not the release signing key (release/publish.sh); that one never goes to CI.
# Needs openssl 3 and gh signed in with access to the repository's secrets. Run it again to rotate: commit the new
# .pub, release a client built with it, then run the workflow so the list is signed with the new key.
#   release/new-code-pins-key.sh
set -euo pipefail
cd "$(dirname "$0")/.."
repo=akreao/vana-client
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
umask 077
openssl genpkey -algorithm ed25519 -out "$dir/key.pem"
openssl pkey -in "$dir/key.pem" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n' > release/code-pins-key.pub
gh secret set CODE_PINS_KEY -R "$repo" < "$dir/key.pem"
echo "CODE_PINS_KEY set on $repo; public key in release/code-pins-key.pub: $(cat release/code-pins-key.pub)"
echo "Commit release/code-pins-key.pub, then run the Code pins workflow (gh workflow run code-pins.yml -R $repo)."

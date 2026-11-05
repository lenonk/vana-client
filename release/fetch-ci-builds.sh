#!/usr/bin/env bash
# Runs .github/workflows/release-builds.yml on master (HEAD, pushed), waits for it and puts its binaries (Linux
# aarch64, macOS aarch64 and x86_64) in target/assets, where release/publish.sh signs them with the rest.
#   release/fetch-ci-builds.sh
set -euo pipefail
cd "$(dirname "$0")/.."
repo=lenonk/vana-client
names=(vana-client-linux-aarch64 vana-client-macos-aarch64 vana-client-macos-x86_64)
git fetch -q origin master
head=$(git rev-parse HEAD)
[ "$(git rev-parse origin/master)" = "$head" ] || { echo "HEAD is not origin/master: push the release commit first" >&2; exit 1; }
start=$(date -u +%Y-%m-%dT%H:%M:%SZ)
gh workflow run release-builds.yml -R "$repo" --ref master
# The run is listed a few seconds after the request: the first one for HEAD made since.
run=""
for _ in $(seq 60); do
    run=$(gh run list -R "$repo" --workflow release-builds.yml --branch master --event workflow_dispatch --limit 10 \
        --json databaseId,headSha,createdAt \
        --jq "[.[] | select(.headSha == \"$head\" and .createdAt >= \"$start\")] | last | .databaseId // empty")
    [ -n "$run" ] && break
    sleep 5
done
[ -n "$run" ] || { echo "the release-builds run did not start" >&2; exit 1; }
echo "release-builds run $run"
gh run watch "$run" -R "$repo" --exit-status --interval 30
for a in "${names[@]}"; do
    rm -f "target/assets/$a" "target/assets/$a.sig"
done
rm -rf target/ci-assets
gh run download "$run" -R "$repo" -D target/ci-assets
mkdir -p target/assets
for a in "${names[@]}"; do
    # Artifacts come zipped, without the executable bit.
    install -m 755 "target/ci-assets/$a/$a" "target/assets/$a"
    echo "target/assets/$a"
done
rm -rf target/ci-assets

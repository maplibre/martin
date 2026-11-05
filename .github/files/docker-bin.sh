#!/usr/bin/env bash
# docker-bin.sh <platform> <image> <binary> [args...]: runs it as e2e tests run a local binary,
# in the caller's directory and terminal.
set -euo pipefail

platform=$1
image=$2
binary=$3
shift 3

workspace=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tmp=${TMPDIR:-/tmp}

terminal=()
if [[ -t 0 ]]; then
  terminal=(--interactive --tty)
fi

exec docker run --rm --net host --platform "$platform" --user "$(id -u):$(id -g)" \
  -e DATABASE_URL -e AWS_REGION=eu-central-1 -e RUST_LOG_FORMAT=bare -e RUST_LOG \
  -v "$workspace:$workspace" -v "$tmp:$tmp" -w "$PWD" "${terminal[@]}" \
  --entrypoint "$binary" "$image" "$@"

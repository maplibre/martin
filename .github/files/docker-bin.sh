#!/usr/bin/env bash
# docker-bin.sh <platform> <image> <binary> [args...]: runs it as e2e tests run a local binary,
# in the caller's directory and terminal, with a writable HOME like a local run has.
set -euo pipefail

platform=$1
image=$2
binary=$3
shift 3

workspace=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tmp=${TMPDIR:-/tmp}

# Keeps the caller's HOME when the container sees it too, such as /proc/self or a mounted directory, and otherwise uses the writable $tmp.
home=$tmp
case "$HOME" in
  /proc/* | "$workspace" | "$workspace"/* | /tmp | /tmp/* | "$tmp" | "$tmp"/*) home=$HOME ;;
esac

# Mounts /tmp, where tests keep their files, and TMPDIR when a caller points it elsewhere.
mounts=(-v "$workspace:$workspace" -v /tmp:/tmp)
case "$tmp" in
  /tmp | /tmp/*) ;;
  *) mounts+=(-v "$tmp:$tmp") ;;
esac

terminal=()
if [[ -t 0 ]]; then
  terminal=(--interactive --tty)
fi

exec docker run --rm --net host --platform "$platform" --user "$(id -u):$(id -g)" \
  -e DATABASE_URL -e MARTIN_HEALTH_URL -e AWS_REGION=eu-central-1 -e RUST_LOG_FORMAT=bare -e RUST_LOG -e HOME="$home" \
  -e TMPDIR -e XDG_CACHE_HOME -e MESA_SHADER_CACHE_DIR -e MESA_SHADER_CACHE_DISABLE -e __EGL_VENDOR_LIBRARY_FILENAMES \
  "${mounts[@]}" -w "$PWD" "${terminal[@]}" \
  --entrypoint "$binary" "$image" "$@"

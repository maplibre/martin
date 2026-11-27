#!/usr/bin/env bash
# docker-healthcheck.sh <image>: serves a fixture with the image's own HEALTHCHECK and waits for it to report healthy.
set -euo pipefail

image=$1
workspace=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
name="martin-healthcheck-$$"

docker run --detach --name "$name" --health-interval 1s --health-start-period 30s \
  --volume "$workspace/tests/fixtures/pmtiles2:/data:ro" "$image" /data >/dev/null
trap 'docker rm --force "$name" >/dev/null' EXIT

for _ in $(seq 60); do
  status=$(docker inspect --format '{{.State.Health.Status}}' "$name")
  if [[ "$status" == healthy ]]; then
    echo "$image is healthy"
    exit 0
  fi
  sleep 1
done

echo "$image did not become healthy, last status: $status" >&2
docker inspect --format '{{json .State.Health}}' "$name" >&2
docker logs "$name" >&2
exit 1

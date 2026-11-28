#!/usr/bin/env bash
# Serves the fixtures with a prebuilt martin (nightly image, or MARTIN_BIN), so the UI tests never compile Rust.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
port="${MARTIN_E2E_PORT:-3010}"
sources=(
  tests/fixtures/geojson
  tests/fixtures/pmtiles
  tests/fixtures/cog/usda_naip_256_lzw_z3.tif
  --sprite tests/fixtures/sprites/src1
  --font tests/fixtures/fonts
  --style crates/martin-ui/e2e/fixtures/styles
)

cd "$repo"
if [[ -n "${MARTIN_BIN:-}" ]]; then
  exec "$MARTIN_BIN" "${sources[@]}" --listen-addresses "127.0.0.1:$port"
fi

container="martin-ui-e2e-$port"
docker rm --force "$container" >/dev/null 2>&1 || true
exec docker run --rm --init --name "$container" \
  --publish "127.0.0.1:$port:3000" \
  --volume "$repo:/work:ro" --workdir /work \
  "${MARTIN_E2E_IMAGE:-ghcr.io/maplibre/martin:nightly}" \
  "${sources[@]}"

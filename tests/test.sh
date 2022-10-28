#!/usr/bin/env bash
set -euo pipefail

DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BIN="${MARTIN_BIN:-cargo run --}"

function wait_for_martin {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS http://localhost:3000/healthz

    echo "Waiting for Martin to start..."
    n=0
    until [ "$n" -ge 100 ]; do
       timeout -k 20s 20s curl -sSf http://localhost:3000/healthz 2>/dev/null >/dev/null && break
       n=$((n+1))
       sleep 0.2
    done
    echo "Martin has started."
}

curl --version

$MARTIN_BIN --default-srid 900913 &
PROCESS_ID=$!
trap "kill $PROCESS_ID || true" EXIT
wait_for_martin
tests/test-auto-sources.sh
kill $PROCESS_ID

$MARTIN_BIN --config tests/config.yaml "$DATABASE_URL" &
PROCESS_ID=$!
trap "kill $PROCESS_ID || true" EXIT
wait_for_martin
tests/test-configured-sources.sh
kill $PROCESS_ID

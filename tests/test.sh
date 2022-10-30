#!/usr/bin/env bash
set -euo pipefail

DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BIN="${MARTIN_BIN:-cargo run --}"

function wait_for_martin {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS http://localhost:3000/healthz

    echo "Waiting for Martin to start..."
    for i in {1..10}; do
        if timeout -k 5s 5s curl -sSf http://localhost:3000/healthz 2>/dev/null >/dev/null; then
            echo "Martin is up!"
            curl -s http://localhost:3000/healthz
            return
        fi
        sleep 0.2
    done

    echo "Martin did not start in time"
    exit 1
}

curl --version

$MARTIN_BIN --default-srid 900913 &
PROCESS_ID=$!
trap "kill $PROCESS_ID || true" EXIT
wait_for_martin
echo "Test auto configured Martin"
tests/test-auto-sources.sh
kill $PROCESS_ID

$MARTIN_BIN --config tests/config.yaml "$DATABASE_URL" &
PROCESS_ID=$!
trap "kill $PROCESS_ID || true" EXIT
wait_for_martin
echo "Test pre-configured Martin"
tests/test-configured-sources.sh
kill $PROCESS_ID

#!/usr/bin/env bash
set -euo pipefail

echo "Testing the /refresh endpoint"

TEST_NAME="test-refresh"

LOG_DIR="${LOG_DIR:-target/test_logs}"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"

TEST_OUT_BASE_DIR="$(dirname "$0")/output"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"

mkdir -p "$TEST_OUT_DIR"

MARTIN_PORT="${MARTIN_PORT:-3111}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_ARGS="${MARTIN_ARGS:---listen-addresses localhost:${MARTIN_PORT}}"
MARTIN_BIN="${MARTIN_BIN:-target/debug/martin} ${MARTIN_ARGS}"

MARTIN_DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
unset DATABASE_URL

MARTIN_BUILD_ALL="${MARTIN_BUILD_ALL:-cargo build}"

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl --silent --show-error --fail --compressed}


test_jsn()
{
  FILENAME="$TEST_OUT_DIR/$1.json"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" | jq -e > "$FILENAME"
}

function wait_for {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS "$MARTIN_URL/health"
    PROCESS_ID=$1
    PROC_NAME=$2
    TEST_URL=$3
    echo "Waiting for $PROC_NAME ($PROCESS_ID) to start by checking $TEST_URL to be valid..."
    for i in {1..60}; do
        if $CURL "$TEST_URL" 2>/dev/null >/dev/null; then
            echo "$PROC_NAME is up!"
            if [[ "$PROC_NAME" == "Martin" ]]; then
              $CURL "$TEST_URL"
            fi
            return
        fi
        if ps -p $PROCESS_ID > /dev/null ; then
            echo "$PROC_NAME is not up yet, waiting for $TEST_URL ..."
            sleep 1
        else
            echo "$PROC_NAME died!"
            ps au
            lsof -i || true;
            exit 1
        fi
    done
    echo "$PROC_NAME did not start in time"
    ps au
    lsof -i || true;
    exit 1
}

set -x

# Prepare test environment

# Make sure all targets are built - this way it won't timeout while waiting for it to start
# If set to "-", skip this step (e.g. when testing a pre-built binary)
if [[ "$MARTIN_BUILD_ALL" != "-" ]]; then
  rm -rf "$MARTIN_BIN"
  $MARTIN_BUILD_ALL
fi

# Start martin

echo "Starting martin"

export DATABASE_URL="$MARTIN_DATABASE_URL"

mkdir -p tests/tmp
cp -f tests/config.yaml tests/tmp/config.yaml

ARG=(--config tests/tmp/config.yaml --max-feature-count 1000 -W 1)

$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=`jobs -p | tail -n 1`
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for $MARTIN_PROC_ID Martin "$MARTIN_URL/health"
unset DATABASE_URL

# Fetch and verify the catalog json before refresh calling
echo "Fetch catalog"
test_jsn catalog_before_refresh catalog

# Update config and database
cp -f tests/config-for-refresh.yaml tests/tmp/config.yaml
# todo use psql to alter database
$CURL  -X  POST "$MARTIN_URL/refresh"

# Fetch and verify the catalog json after refresh calling

test_jsn catalog_after_refresh catalog

#!/usr/bin/env bash
set -euo pipefail



MARTIN_DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
unset DATABASE_URL

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl --silent --show-error --fail --compressed}

MARTIN_BUILD_ALL="${MARTIN_BUILD_ALL:-cargo build}"

STATICS_URL="${STATICS_URL:-http://localhost:5412}"
MARTIN_PORT="${MARTIN_PORT:-3111}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_ARGS="${MARTIN_ARGS:---listen-addresses localhost:${MARTIN_PORT}}"

# Using direct compiler output paths to avoid extra log entries
MARTIN_BIN="${MARTIN_BIN:-target/debug/martin} ${MARTIN_ARGS}"

TEST_OUT_BASE_DIR="$(dirname "$0")/output"
LOG_DIR="${LOG_DIR:-target/test_logs}"
mkdir -p "$LOG_DIR"

TEST_TEMP_DIR="$(dirname "$0")/mbtiles_temp_files"
rm -rf "$TEST_TEMP_DIR"
mkdir -p "$TEST_TEMP_DIR"

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

function kill_process {
    PROCESS_ID=$1
    PROC_NAME=$2
    echo "Waiting for $PROC_NAME ($PROCESS_ID) to stop..."
    kill $PROCESS_ID
    for i in {1..50}; do
        if ps -p $PROCESS_ID > /dev/null ; then
            sleep 0.1
        else
            echo "$PROC_NAME ($PROCESS_ID) has stopped"
            return
        fi
    done
    echo "$PROC_NAME did not stop in time, killing it"
    kill -9 $PROCESS_ID
    # wait for it to die using timeout and wait
    timeout -k 1s 1s wait $PROCESS_ID || true;
}

# start the server
echo "------------------------------------------------------------------------------------------------------------------------"
echo "Starting Martin..."
TEST_NAME="watched"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
mkdir -p "$TEST_OUT_DIR"

ARG=(--config tests/config.yaml --max-feature-count 1000 -W 1)

$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=`jobs -p | tail -n 1`

# get the catalog before update config
$CURL "$MARTIN_URL/catalog" | jq -e > "$TEST_OUT_DIR/before_update.json"

# update the config



# call the /refresh to update catalog



# get the catalog after update config


# verfiy that the catalog is expected




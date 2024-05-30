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

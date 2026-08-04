#!/usr/bin/env bash
set -euo pipefail

MARTIN_DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
unset DATABASE_URL

export RUST_LOG_FORMAT=bare

MARTIN_BUILD_ALL="${MARTIN_BUILD_ALL:-cargo build}"

MARTIN_PORT="${MARTIN_PORT:-3111}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_ARGS="${MARTIN_ARGS:---listen-addresses localhost:${MARTIN_PORT}}"

# Using direct compiler output paths to avoid extra log entries
MARTIN_BIN="${MARTIN_BIN:-target/debug/martin} ${MARTIN_ARGS}"

TEST_OUT_BASE_DIR="$(dirname "$0")/output"
LOG_DIR="${LOG_DIR:-target/test_logs}"
mkdir -p "$LOG_DIR"

# Verify the tools used in the tests are available
# todo add more verification for other tools like jq file curl sqlite3...
if sed --version > /dev/null 2>&1; then
  SED=${SED:-sed}
elif gsed --version > /dev/null 2>&1; then
  SED=${SED:-gsed}
else
  echo 'GNU sed is required for testing'
  exit 1
fi

# curl must support Brotli so the server's preferred encoding (br) is used,
# keeping test output consistent across platforms.
# On macOS the system curl lacks Brotli; prefer the Homebrew-installed one.
if [[ -z "${CURL_BIN:-}" ]]; then
  for candidate in curl /opt/homebrew/opt/curl/bin/curl /usr/local/opt/curl/bin/curl; do
    if command -v "$candidate" > /dev/null 2>&1 && "$candidate" --version 2>/dev/null | grep -q brotli; then
      CURL_BIN="$candidate"
      break
    fi
  done
fi
if [[ -z "${CURL_BIN:-}" ]]; then
  echo 'curl with Brotli support is required for testing.'
  echo 'On macOS, install it with: brew install curl'
  exit 1
fi
# --connect-timeout keeps the wait_for retry loop snappy when the server is not up yet;
# --max-time caps a wedged response so a single hung request fails the job in minutes
# instead of running into the 6h CI ceiling (see the maplibre-native render-deadlock).
CURL="${CURL:-$CURL_BIN --silent --show-error --fail --compressed --connect-timeout 10 --max-time 120}"

function wait_for {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS "$MARTIN_URL/health"
    PROCESS_ID=$1
    PROC_NAME=$2
    TEST_URL=$3
    echo "Waiting for $PROC_NAME ($PROCESS_ID) to start by checking $TEST_URL to be valid..."
    for _ in {1..60}; do
        if $CURL "$TEST_URL" 2>/dev/null >/dev/null; then
            echo "$PROC_NAME is up!"
            return
        fi
        if ps -p "$PROCESS_ID" > /dev/null ; then
            echo "$PROC_NAME is not up yet, waiting for $TEST_URL ..."
            sleep 1
        else
            echo "$PROC_NAME died!"
            ps au
            if command -v lsof > /dev/null; then lsof -i || true; fi
            exit 1
        fi
    done
    echo "$PROC_NAME did not start in time"
    ps au
    if command -v lsof > /dev/null; then lsof -i || true; fi
    exit 1
}

function kill_process {
    PROCESS_ID=$1
    PROC_NAME=$2
    echo "Waiting for $PROC_NAME ($PROCESS_ID) to stop..."
    kill "$PROCESS_ID"
    for _ in {1..50}; do
        if ps -p "$PROCESS_ID" > /dev/null ; then
            sleep 0.1
        else
            echo "$PROC_NAME ($PROCESS_ID) has stopped"
            return
        fi
    done
    echo "$PROC_NAME did not stop in time, killing it"
    kill -9 "$PROCESS_ID"
    # wait for it to die using timeout and wait
    timeout -k 1s 1s wait "$PROCESS_ID" || true;
}

# Delete line from a file $1 that matches parameter $2 and log the action
remove_lines() {
  FILE="$1"
  LINE_TO_REMOVE="$2"
  >&2 echo "Removing line '$LINE_TO_REMOVE' from $FILE"
  quietly_remove_lines "$FILE" "$LINE_TO_REMOVE"
}

# Delete line from a file $1 that matches parameter $2
quietly_remove_lines() {
  FILE="$1"
  LINE_TO_REMOVE="$2"
  grep -v "$LINE_TO_REMOVE" "${FILE}" > "${FILE}.tmp"
  mv "${FILE}.tmp" "${FILE}"
}

test_log_has_str() {
  LOG_FILE="$1"
  EXPECTED_TEXT="$2"
  if ! grep -q "$EXPECTED_TEXT" "$LOG_FILE"; then
    echo "ERROR: $LOG_FILE log file does not have: '$EXPECTED_TEXT'"
    exit 1
  else
    >&2 echo "OK: $LOG_FILE contains expected text: '$EXPECTED_TEXT'"
    quietly_remove_lines "$LOG_FILE" "$EXPECTED_TEXT"
  fi
}

validate_log() {
  LOG_FILE="$1"
  >&2 echo "Validating log file $LOG_FILE"

  # Older versions of PostGIS don't support the margin parameter, so we need to remove it from the log
  remove_lines "$LOG_FILE" 'Margin parameter in ST_TileEnvelope is not supported'
  remove_lines "$LOG_FILE" 'PostgreSQL is older than the recommended minimum 12.0.0'
  remove_lines "$LOG_FILE" 'In the used version, some geometry may be hidden on some zoom levels.'
  remove_lines "$LOG_FILE" 'Unable to deserialize SQL comment on public.points2 as tilejson, the automatically generated tilejson would be used: expected value at line 1 column 1'
  # Debug builds are slower; table discovery may exceed the default bounds timeout on slow runners
  remove_lines "$LOG_FILE" 'Discovering tables in PostgreSQL database .* is taking too long'
  # Tables/views without a usable spatial index or statistics fall back from the quick ST_EstimatedExtent to the exact bounds calculation
  remove_lines "$LOG_FILE" 'ST_EstimatedExtent on .* failed, trying slower method to compute bounds'

  echo "Checking for no other warnings or errors in the log"
  if grep -e ' ERROR ' -e ' WARN ' "$LOG_FILE"; then
    echo "Log file $LOG_FILE has unexpected warnings or errors"
    exit 1
  fi
}

echo "::group::versions"
curl --version
grep --version | head -1

# Make sure all targets are built - this way it won't timeout while waiting for it to start
# If set to "-", skip this step (e.g. when testing a pre-built binary)
if [[ "$MARTIN_BUILD_ALL" != "-" ]]; then
  echo "::group::Make sure all targets are built. Set MARTIN_BUILD_ALL=- to skip this step."
  rm -rf "$MARTIN_BIN"
  $MARTIN_BUILD_ALL
  echo "::endgroup::"
fi

# Prepare MBTiles from SQL fixtures
echo "::group::Prepare .mbtiles fixtures from .sql"
FOLDERS=("tests/fixtures/files" "tests/fixtures/mbtiles")

for folder in "${FOLDERS[@]}"; do
    echo "Processing folder: $folder"

    # Remove existing .mbtiles files before recreating them
    rm -f "$folder"/*.mbtiles

    for sql_file in "$folder"/*.sql; do
        [ -e "$sql_file" ] || continue

        mbtiles_file="${sql_file%.sql}.mbtiles"
        echo "Creating: $mbtiles_file from $sql_file"
        sqlite3 "$mbtiles_file" < "$sql_file"
    done
done
echo "::endgroup::"

echo "::group::Test auto configured Martin"
TEST_NAME="auto"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
mkdir -p "$TEST_OUT_DIR"


ARG=(--default-srid 900913 --auto-bounds calc --save-config "${TEST_OUT_DIR}/save_config.yaml" tests/fixtures/mbtiles tests/fixtures/pmtiles --sprite tests/fixtures/sprites/src1 --font tests/fixtures/fonts/overpass-mono-regular.ttf --font tests/fixtures/fonts --style tests/fixtures/styles/maplibre_demo.json --style tests/fixtures/styles/src2 --style tests/fixtures/styles/relative_urls.json --tilejson-url-version-param version )
export DATABASE_URL="$MARTIN_DATABASE_URL"

set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"
unset DATABASE_URL

kill_process "$MARTIN_PROC_ID" Martin

test_log_has_str "$LOG_FILE" 'Table public.table_source has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'Table public.table_source_geog has no spatial index on column geog'
test_log_has_str "$LOG_FILE" 'Table public.mat_view has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'Ignoring duplicate font: already configured from another path.*font.name=Overpass Mono Regular'
test_log_has_str "$LOG_FILE" 'source.id.new=stamen_toner__raster_CC-BY-ODbL_z3'
test_log_has_str "$LOG_FILE" 'source.id.new=table_source_multiple_geom.1'
test_log_has_str "$LOG_FILE" 'source.id.new=-function.withweired---_-characters'
test_log_has_str "$LOG_FILE" 'source.id.new=.-Points-----------quote'
test_log_has_str "$LOG_FILE" 'source.id.new=table_name_existing_two_schemas.1'
test_log_has_str "$LOG_FILE" 'source.id.new=view_name_existing_two_schemas.1'
test_log_has_str "$LOG_FILE" 'source.id.new=table_and_view_two_schemas.1'
test_log_has_str "$LOG_FILE" 'Defaulting `pmtiles.allow_http` to `true`. This is likely to become an error in the future for better security.'
test_log_has_str "$LOG_FILE" 'Environment variable AWS_SKIP_CREDENTIALS is deprecated. Please use pmtiles.skip_signature in the configuration file instead.'
test_log_has_str "$LOG_FILE" 'Environment variable AWS_REGION is deprecated. Please use pmtiles.region in the configuration file instead.'
validate_log "$LOG_FILE"
remove_lines "${TEST_OUT_DIR}/save_config.yaml" " connection_string: "
echo "::endgroup::"

echo "::group::Test pre-configured Martin"
TEST_NAME="configured"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
mkdir -p "$TEST_OUT_DIR"

ARG=(--config tests/config.yaml --max-feature-count 1000 --save-config "${TEST_OUT_DIR}/save_config.yaml" -W 1)
export DATABASE_URL="$MARTIN_DATABASE_URL"
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"
unset DATABASE_URL

# Test style rendering (only available on Linux with the rendering feature)
RENDERING_AVAILABLE=0
if [[ $OSTYPE == linux* ]] && $CURL "$MARTIN_URL/style/maplibre/0/0/0.png" > /dev/null 2>&1; then
  >&2 echo "***** Test server-side style rendering *****"
  RENDERING_AVAILABLE=1
  # PNG rendering
  $CURL "$MARTIN_URL/style/maplibre/0/0/0.png" > /dev/null
  $CURL "$MARTIN_URL/style/maplibre/1/0/0.png" > /dev/null
  $CURL "$MARTIN_URL/style/maplibre/1/1/0.png" > /dev/null
  # JPEG rendering
  $CURL "$MARTIN_URL/style/maplibre/0/0/0.jpeg" > /dev/null
  $CURL "$MARTIN_URL/style/maplibre/1/0/0.jpg" > /dev/null
  echo "Style rendering smoke tests passed (PNG + JPEG)"
fi

kill_process "$MARTIN_PROC_ID" Martin
test_log_has_str "$LOG_FILE" 'Table public.table_source has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'Table public.table_source_geog has no spatial index on column geog'
test_log_has_str "$LOG_FILE" 'Table public.mat_view has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'Ignoring duplicate font: already configured from another path.*font.name=Overpass Mono Regular'
# rendering: true produces different warnings depending on whether the rendering feature is compiled in
if [[ "$RENDERING_AVAILABLE" == "1" ]]; then
  test_log_has_str "$LOG_FILE" 'experimental feature rendering is enabled'
else
  test_log_has_str "$LOG_FILE" "Ignoring unrecognized configuration key 'styles.rendering'. Please check your configuration file for typos."
fi
test_log_has_str "$LOG_FILE" 'Defaulting `pmtiles.allow_http` to `true`. This is likely to become an error in the future for better security.'
test_log_has_str "$LOG_FILE" 'Environment variable AWS_SKIP_CREDENTIALS is deprecated. Please use pmtiles.skip_signature in the configuration file instead.'
test_log_has_str "$LOG_FILE" 'Environment variable AWS_REGION is deprecated. Please use pmtiles.region in the configuration file instead.'
validate_log "$LOG_FILE"
remove_lines "${TEST_OUT_DIR}/save_config.yaml" " connection_string: "
echo "::endgroup::"

# If we don't do this, rounding differences on CI and local machines are a problem
echo "::group::redact unnecessary precision in *_config.yaml"
for file in $(find ./tests/output/ ./tests/expected/ -name "*_config.yaml" -type f); do
    echo "truncating floats in $file"
    "$SED" --regexp-extended --in-place 's/(-?[0-9]+\.[0-9]{10})[0-9]+$/\1 # truncated to 10 digits/g' "$file"
    "$SED" --regexp-extended --in-place 's/0+ # truncated/ # truncated/g' "$file"
done
echo "::endgroup::"

>&2 echo "All integration tests have passed"

#!/usr/bin/env bash
set -euo pipefail

MARTIN_DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
unset DATABASE_URL

export RUST_LOG_FORMAT=bare

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

# Verify the tools used in the tests are available
# todo add more verification for other tools like jq file curl sqlite3...
if ! command -v mvt > /dev/null; then
  echo "the 'mvt' CLI is required for testing (used to dump vector tiles)"
  echo "Install it with one of these:"
  echo "   cargo binstall fast-mvt"
  echo "   cargo install fast-mvt --features=cli"
  exit 1
fi

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

cleanup_json_floats() {
  # round numbers to $1 decimal places, with optional $2 jq cmd
  jq --sort-keys --exit-status --argjson PREC "$1" \
     "${2:-}"'walk( if type == "number" then (. * $PREC | round | . / $PREC) else . end )'
}

cleanup_json_ints() {
  # jq before 1.6 had a different float->int behavior, so trying to make it consistent in all
  jq --sort-keys --exit-status \
     "${1:-}"'walk( if type == "number" then .+0.0 else . end )'
}

test_jsn() {
  FILENAME="$TEST_OUT_DIR/$1.json"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL  --dump-header  "$FILENAME.headers" "$URL" | cleanup_json_ints > "$FILENAME"
  clean_headers_dump "$FILENAME.headers"
}

test_metrics() {
  FILENAME="$TEST_OUT_DIR/$1"
  URL="$MARTIN_URL/_/metrics"

  echo "Testing $1 from $URL"
  $CURL --dump-header  "$FILENAME.headers" "$URL" | $SED --regexp-extended 's/^(martin_.*?) [\.0-9]+$/\1 NUMBER/g' > "$FILENAME.txt"
  clean_headers_dump "$FILENAME.headers"
  $CURL --dump-header  "$FILENAME.fetched_with_compression.headers" --compressed "$URL" | $SED --regexp-extended 's/^(martin_.*?) [\.0-9]+$/\1 NUMBER/g' > "$FILENAME.fetched_with_compression.txt"
  clean_headers_dump "$FILENAME.fetched_with_compression.headers"
  # due to slight timing differences, these might be slightly different
  $SED --regexp-extended --in-place 's/^content-length: [\.0-9]+$/content-length: NUMBER/g' "$FILENAME.headers"
  $SED --regexp-extended --in-place 's/^content-length: [\.0-9]+$/content-length: NUMBER/g' "$FILENAME.fetched_with_compression.headers"
}

test_mvt() {
  FILENAME="$TEST_OUT_DIR/$1.mvt"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL --dump-header  "$FILENAME.headers" "$URL" > "$FILENAME"
  clean_headers_dump "$FILENAME.headers"

  # Dump the vector tile into a human-readable, diffable text form. `mvt dump` parses the tile, so
  # it also validates the protobuf (it exits non-zero on a malformed tile). Only the dump is kept
  # under version control - the raw .mvt is a build artifact and is removed.
  mvt dump "$FILENAME" > "$FILENAME.txt"
  rm "$FILENAME"
}

test_png() {
  # 3rd argument is optional, .png by default
  FILENAME="$TEST_OUT_DIR/$1.${3:-png}"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL --dump-header  "$FILENAME.headers" "$URL" > "$FILENAME"
  clean_headers_dump "$FILENAME.headers"

  if [[ $OSTYPE == linux* || $OSTYPE == darwin* ]]; then
    # some 'file' versions are more verbose, but CI is not
    # we must reduce this to match their output
    file "$FILENAME" | $SED 's#Web/P image, with alpha, 511+1x511+1#Web/P image#' > "$FILENAME.txt"
  fi
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

# if we dump a headers file via curl, this is otherwise not reproducible
clean_headers_dump() {
  FILE="$1"
  # now we need to strip the date header as it is undeterministic
  $SED --regexp-extended --in-place "s/date: .+//" "$FILE"
  # the http version is not an "header" that we want to assert
  $SED --regexp-extended --in-place "s/HTTP.+//" "$FILE"
  # need to remove entirely empty lines, \r\n and leading/trailing whitespace
  # sorting is arbitrary => sort here
  tr -s '\r\n' '\n' < "$FILE" | sort > "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
  # we need to remove the first line as squeezing repeat newlines makes does not remove this empty line
  $SED --in-place '1d' "$FILE"
}

# Stage the fixture outside the watched directory and rename it in, so it appears atomically.
# A plain `cp` writes in place, letting the reload watcher read a 0-byte file mid-copy.
# Staging inside the watched directory is not enough either: the watcher still observes the
# intermediate `.staging` file and warns when it is renamed away mid-scan.
# The staging path is in the destination's parent directory, which shares its filesystem, so the
# rename is atomic and the watcher only ever sees the finished file appear in one step.
install_watched_fixture() {
  SRC="$1"
  DEST="$2"
  STAGING="$(dirname "$DEST")/../$(basename "$DEST").staging"
  cp "$SRC" "$STAGING"
  mv "$STAGING" "$DEST"
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

wait_for_catalog_source_removed() {
  SOURCE_ID="$1"
  echo "Waiting for source '$SOURCE_ID' to be removed from catalog..."
  for _ in {1..30}; do
    if ! $CURL "$MARTIN_URL/catalog" 2>/dev/null | jq -e --arg id "$SOURCE_ID" '.tiles | has($id)' > /dev/null 2>&1; then
      echo "Source '$SOURCE_ID' has been removed from catalog."
      return 0
    fi
    sleep 1
  done
  echo "ERROR: Source '$SOURCE_ID' was not removed from catalog within 30s"
  exit 1
}

wait_for_log_str() {
  WAIT_LOG_FILE="$1"
  EXPECTED="$2"
  echo "Waiting for '$EXPECTED' in $WAIT_LOG_FILE..."
  for _ in {1..30}; do
    if grep -q "$EXPECTED" "$WAIT_LOG_FILE" 2>/dev/null; then
      echo "Found '$EXPECTED' in log."
      return 0
    fi
    sleep 1
  done
  echo "ERROR: '$EXPECTED' not found in $WAIT_LOG_FILE within 30s"
  exit 1
}

echo "::group::versions"
curl --version
jq --version
grep --version | head -1

# Make sure all targets are built - this way it won't timeout while waiting for it to start
# If set to "-", skip this step (e.g. when testing a pre-built binary)
if [[ "$MARTIN_BUILD_ALL" != "-" ]]; then
  echo "::group::Make sure all targets are built. Set MARTIN_BUILD_ALL=- to skip this step."
  rm -rf "$MARTIN_BIN"
  $MARTIN_BUILD_ALL
  echo "::endgroup::"
fi

echo "::group::Check HTTP server is running"
if ! $CURL --head "$STATICS_URL/webp2.pmtiles"; then
    echo "ERROR: pmtiles fileserver is not reachable at $STATICS_URL."
    echo "       Start it with 'just start-pmtiles-server' before running this script."
    exit 1
fi
echo "::endgroup::"

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


ARG=(--default-srid 900913 --auto-bounds calc --save-config "${TEST_OUT_DIR}/save_config.yaml" tests/fixtures/mbtiles tests/fixtures/pmtiles tests/fixtures/cog "$STATICS_URL/webp2.pmtiles" s3://pmtilestest/cb_2018_us_zcta510_500k.pmtiles --sprite tests/fixtures/sprites/src1 --font tests/fixtures/fonts/overpass-mono-regular.ttf --font tests/fixtures/fonts --style tests/fixtures/styles/maplibre_demo.json --style tests/fixtures/styles/src2 --style tests/fixtures/styles/relative_urls.json --tilejson-url-version-param version )
export DATABASE_URL="$MARTIN_DATABASE_URL"

set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"
unset DATABASE_URL

>&2 echo "***** Test server response for PMTiles source *****"
test_jsn pmt         stamen_toner__raster_CC-BY-ODbL_z3
test_png pmt_3_4_2   stamen_toner__raster_CC-BY-ODbL_z3/3/4/2
test_png webp2_1_0_0 webp2/1/0/0  # HTTP pmtiles
test_mvt s3_1_0_0    cb_2018_us_zcta510_500k/1/0/0  # HTTP pmtiles via s3

# TODO: enable below once unstable-cog is stable
#>&2 echo "***** Test server response for COG(Cloud Optimized GeoTiff) source *****"
#test_jsn rgb_u8       rgb_u8
#test_png rgb_u8_0_0_0 rgb_u8/0/0/0
#test_png rgb_u8_3_0_0 rgb_u8/3/0/0
#test_png rgb_u8_3_1_1 rgb_u8/3/1/1

#test_jsn rgba_u8       rgba_u8
#test_png rgba_u8_0_0_0 rgba_u8/0/0/0
#test_png rgba_u8_3_0_0 rgba_u8/3/0/0
#test_png rgba_u8_3_1_1 rgba_u8/3/1/1

#test_jsn rgba_u8_nodata       rgba_u8_nodata
#test_png rgba_u8_nodata_0_0_0 rgba_u8_nodata/0/0/0
#test_png rgba_u8_nodata_1_0_0 rgba_u8_nodata/1/0/0

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

>&2 echo "Test catalog"
test_jsn catalog_cfg  catalog
test_jsn cmp          table_source,points1,points2

# Test tile sources
test_mvt tbl_0_0_0    table_source/0/0/0
test_mvt cmp_0_0_0    points1,points2/0/0/0
test_mvt fnc_0_0_0    function_zxy_query/0/0/0
test_mvt fnc2_0_0_0   function_zxy_query_test/0/0/0?token=martin
test_png pmt_0_0_0    pmt/0/0/0
test_png pmt2_0_0_0   pmt2/0/0/0  # HTTP pmtiles

# Test comments override
test_jsn tbl_comment_cfg  MixPoints
test_jsn fnc_comment_cfg  function_Mixed_Name

>&2 echo "***** Test observability outputs (metrics, logs) *****"

test_metrics "metrics_1"

# Test style rendering (only available on Linux with the rendering feature)
# Run AFTER metrics collection to avoid adding rendering-specific metric entries to expected output
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
echo "::group::redact unnecessary precision in *_config.yaml and *.json"
for file in $(find ./tests/output/ ./tests/expected/ -name "*_config.yaml" -type f); do
    echo "truncating floats in $file"
    "$SED" --regexp-extended --in-place 's/(-?[0-9]+\.[0-9]{10})[0-9]+$/\1 # truncated to 10 digits/g' "$file"
    "$SED" --regexp-extended --in-place 's/0+ # truncated/ # truncated/g' "$file"
done
for file in $(find ./tests/output/ ./tests/expected/ -name "*.json" -type f); do
    echo "truncating floats in $file"
    cat "$file" | cleanup_json_floats 10000000000 > "$file.tmp"

    # update headers if content changed
    if ! cmp -s "$file" "$file.tmp"; then
        if [[ -f "$file.headers" ]]; then
            "$SED" --regexp-extended --in-place 's/^etag: .*/etag: "unstable due to floating-point rounding"/g' "$file.headers"
        fi
    fi

    mv "$file.tmp" "$file"
done
echo "::endgroup::"

# The COG reloader is only active when compiled with --features unstable-cog.
# Detect this at runtime by copying a .tif file and checking whether it appears in the catalog.
echo "::group::Test COG hot reload"
TEST_NAME="cog_reload"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
COG_RELOAD_WATCH_DIR="${TEST_TEMP_DIR}/cog_reload_watch"
mkdir -p "$TEST_OUT_DIR" "$COG_RELOAD_WATCH_DIR"

ARG=("$COG_RELOAD_WATCH_DIR")
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"

COG_SOURCE_ID="usda_naip_128_none_z2"
install_watched_fixture "tests/fixtures/cog/${COG_SOURCE_ID}.tif" "$COG_RELOAD_WATCH_DIR/${COG_SOURCE_ID}.tif"

COG_ENABLED=0
for _ in {1..10}; do
  if $CURL "$MARTIN_URL/catalog" 2>/dev/null | jq -e --arg id "$COG_SOURCE_ID" '.tiles | has($id)' > /dev/null 2>&1; then
    COG_ENABLED=1
    break
  fi
  sleep 1
done

if [[ "$COG_ENABLED" == "1" ]]; then
  >&2 echo "COG reloader is active - running hot reload tests"
  test_jsn cog_reload_catalog_added catalog

  >&2 echo "Test COG reload: updating a COG file triggers source update"
  touch "$COG_RELOAD_WATCH_DIR/${COG_SOURCE_ID}.tif"
  wait_for_log_str "$LOG_FILE" "Updated source source.id=${COG_SOURCE_ID}"
  test_jsn cog_reload_catalog_updated catalog

  >&2 echo "Test COG reload: removing a COG file triggers source removal"
  rm "$COG_RELOAD_WATCH_DIR/${COG_SOURCE_ID}.tif"
  wait_for_catalog_source_removed "$COG_SOURCE_ID"
  $CURL "$MARTIN_URL/catalog" | jq --sort-keys > "$TEST_OUT_DIR/catalog_after_remove.json"

  kill_process "$MARTIN_PROC_ID" Martin
  trap - EXIT HUP INT TERM

  test_log_has_str "$LOG_FILE" "Added source source.id=${COG_SOURCE_ID}"
  test_log_has_str "$LOG_FILE" "Updated source source.id=${COG_SOURCE_ID}"
  test_log_has_str "$LOG_FILE" "Removed source source.id=${COG_SOURCE_ID}"
else
  >&2 echo "COG reloader not active (binary not compiled with unstable-cog) - skipping COG reload tests"
  rm -f "$COG_RELOAD_WATCH_DIR/${COG_SOURCE_ID}.tif"
  kill_process "$MARTIN_PROC_ID" Martin
  trap - EXIT HUP INT TERM
fi

test_log_has_str "$LOG_FILE" 'WARN Defaulting `pmtiles.allow_http` to `true`. This is likely to become an error in the future for better security.'
test_log_has_str "$LOG_FILE" 'WARN Environment variable AWS_SKIP_CREDENTIALS is deprecated. Please use pmtiles.skip_signature in the configuration file instead.'
test_log_has_str "$LOG_FILE" 'WARN Environment variable AWS_REGION is deprecated. Please use pmtiles.region in the configuration file instead.'
validate_log "$LOG_FILE"
echo "::endgroup::"

rm -rf "$TEST_TEMP_DIR"

>&2 echo "All integration tests have passed"

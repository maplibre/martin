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
MARTIN_CP_BIN="${MARTIN_CP_BIN:-target/debug/martin-cp}"
MBTILES_BIN="${MBTILES_BIN:-target/debug/mbtiles}"

TEST_OUT_BASE_DIR="$(dirname "$0")/output"
LOG_DIR="${LOG_DIR:-target/test_logs}"
mkdir -p "$LOG_DIR"

TEST_TEMP_DIR="$(dirname "$0")/mbtiles_temp_files"
rm -rf "$TEST_TEMP_DIR"
mkdir -p "$TEST_TEMP_DIR"

# Verify the tools used in the tests are available
# todo add more verification for other tools like jq file curl sqlite3...
if [[ $OSTYPE == linux* ]]; then # We only used ogrmerge.py on Linux see the test_pbf() function
  if ! command -v ogrmerge.py > /dev/null; then
  echo "gdal-bin is required for testing"
  echo "For Ubuntu, you could install it with sudo apt update && sudo apt install gdal-bin -y"
  echo "see more at https://gdal.org/en/stable/download.html#binaries"
  exit 1
  fi
fi

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
            if [[ "$PROC_NAME" == "Martin" ]]; then
              $CURL "$TEST_URL"
            fi
            return
        fi
        if ps -p "$PROCESS_ID" > /dev/null ; then
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

test_jsn() {
  FILENAME="$TEST_OUT_DIR/$1.json"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  # jq before 1.6 had a different float->int behavior, so trying to make it consistent in all
  $CURL "$URL" | jq --sort-keys -e 'walk(if type == "number" then .+0.0 else . end)' > "$FILENAME"
}

test_pbf() {
  FILENAME="$TEST_OUT_DIR/$1.pbf"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"

  if [[ $OSTYPE == linux* ]]; then
    ./tests/fixtures/vtzero-check "$FILENAME"
    # see https://gdal.org/en/stable/programs/ogrmerge.html#ogrmerge
    ogrmerge.py -o "$FILENAME.geojson" "$FILENAME" -single -src_layer_field_name "source_mvt_layer" -src_layer_field_content "{LAYER_NAME}" -f "GeoJSON" -overwrite_ds
    jq --sort-keys '.features |= sort_by(.properties.source_mvt_layer, .properties.gid) | walk(if type == "number" then .+0.0 else . end)' "$FILENAME.geojson" > "$FILENAME.sorted.geojson"
    mv "$FILENAME.sorted.geojson" "$FILENAME.geojson"
  fi
}

test_png() {
  # 3rd argument is optional, .png by default
  FILENAME="$TEST_OUT_DIR/$1.${3:-png}"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"

  if [[ $OSTYPE == linux* ]]; then
    file "$FILENAME" > "$FILENAME.txt"
  fi
}

test_jpg() {
  # test_png can test any image format, but this is a separate function to make it easier to find all the jpeg tests
  test_png "$1" "$2" jpg
}

test_font() {
  FILENAME="$TEST_OUT_DIR/$1.pbf"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"
}

# Delete a line from a file $1 that matches parameter $2
remove_line() {
  FILE="$1"
  LINE_TO_REMOVE="$2"
  >&2 echo "Removing line '$LINE_TO_REMOVE' from $FILE"
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
  fi
  remove_line "$LOG_FILE" "$EXPECTED_TEXT"
}

test_martin_cp() {
  TEST_NAME="$1"
  ARG=("${@:2}")

  LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
  SAVE_CONFIG_FILE="${TEST_OUT_DIR}/${TEST_NAME}_save_config.yaml"
  SUMMARY_FILE="$TEST_OUT_DIR/${TEST_NAME}_summary.txt"
  TEST_FILE="${TEST_TEMP_DIR}/cp_${TEST_NAME}.mbtiles"
  ARG_EXTRAS=(--output-file "$TEST_FILE" --save-config "$SAVE_CONFIG_FILE")

  set -x
  $MARTIN_CP_BIN "${ARG[@]}" "${ARG_EXTRAS[@]}" 2>&1 | tee "$LOG_FILE"
  $MBTILES_BIN validate --agg-hash off "$TEST_FILE" 2>&1 | tee "$TEST_OUT_DIR/${TEST_NAME}_validate.txt"
  $MBTILES_BIN summary "$TEST_FILE" 2>&1 | tee "$SUMMARY_FILE"
  $MBTILES_BIN meta-all "$TEST_FILE" 2>&1 | tee "$TEST_OUT_DIR/${TEST_NAME}_metadata.txt"
  { set +x; } 2> /dev/null

  remove_line "$SAVE_CONFIG_FILE" " connection_string: "
  # These tend to vary between runs. In theory, vacuuming might make it the same.
  remove_line "$SUMMARY_FILE" "File size: "
  remove_line "$SUMMARY_FILE" "Page count: "
}

validate_log() {
  LOG_FILE="$1"
  >&2 echo "Validating log file $LOG_FILE"

  # Older versions of PostGIS don't support the margin parameter, so we need to remove it from the log
  remove_line "$LOG_FILE" 'Margin parameter in ST_TileEnvelope is not supported'
  remove_line "$LOG_FILE" 'Source IDs must be unique'
  remove_line "$LOG_FILE" 'PostgreSQL 11.10.0 is older than the recommended minimum 12.0.0'
  remove_line "$LOG_FILE" 'In the used version, some geometry may be hidden on some zoom levels.'

  echo "Checking for no other warnings or errors in the log"
  if grep -e ' ERROR ' -e ' WARN ' "$LOG_FILE"; then
    echo "Log file $LOG_FILE has unexpected warnings or errors"
    exit 1
  fi
}

compare_sql_dbs() {
  DB_FILE="$1"
  EXPECTED_DB_FILE="$2"
  LOG_FILE="$3"

  if ! command -v sqldiff > /dev/null; then
    echo "ERROR: sqldiff is required for testing, install it with   apt install sqlite3-tools"
    exit 1
  fi

  >&2 echo "Comparing $DB_FILE with the expected $EXPECTED_DB_FILE"

  sqldiff "$DB_FILE" "$EXPECTED_DB_FILE" 2>&1 | tee "$LOG_FILE" \
    || {
         echo "ERROR: sqldiff failed. To accept changes, run this command:"
         echo "   cp $DB_FILE $EXPECTED_DB_FILE"
         exit 1
       }
}

echo "------------------------------------------------------------------------------------------------------------------------"
curl --version
jq --version
grep --version

# Make sure all targets are built - this way it won't timeout while waiting for it to start
# If set to "-", skip this step (e.g. when testing a pre-built binary)
if [[ "$MARTIN_BUILD_ALL" != "-" ]]; then
  rm -rf "$MARTIN_BIN" "$MARTIN_CP_BIN" "$MBTILES_BIN"
  $MARTIN_BUILD_ALL
fi

echo "------------------------------------------------------------------------------------------------------------------------"
echo "Check HTTP server is running"
$CURL --head "$STATICS_URL/webp2.pmtiles"

echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test auto configured Martin"

TEST_NAME="auto"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
mkdir -p "$TEST_OUT_DIR"

ARG=(--default-srid 900913 --auto-bounds calc --save-config "${TEST_OUT_DIR}/save_config.yaml" tests/fixtures/mbtiles tests/fixtures/pmtiles tests/fixtures/cog "$STATICS_URL/webp2.pmtiles" --sprite tests/fixtures/sprites/src1 --font tests/fixtures/fonts/overpass-mono-regular.ttf --font tests/fixtures/fonts --style tests/fixtures/styles/maplibre_demo.json --style tests/fixtures/styles/src2 )
export DATABASE_URL="$MARTIN_DATABASE_URL"

set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)
{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"
unset DATABASE_URL

>&2 echo "Test catalog"
test_jsn catalog_auto catalog

>&2 echo "***** Test server response for table source *****"
test_jsn table_source             table_source
test_pbf tbl_0_0_0                table_source/0/0/0
test_pbf tbl_6_57_29              table_source/6/57/29
test_pbf tbl_12_3673_1911         table_source/12/3673/1911
test_pbf tbl_13_7346_3822         table_source/13/7346/3822
test_pbf tbl_14_14692_7645        table_source/14/14692/7645
test_pbf tbl_17_117542_61161      table_source/17/117542/61161
test_pbf tbl_18_235085_122323     table_source/18/235085/122323

>&2 echo "***** Test server response for composite source *****"
test_jsn cmp                      table_source,points1,points2
test_pbf cmp_0_0_0                table_source,points1,points2/0/0/0
test_pbf cmp_6_57_29              table_source,points1,points2/6/57/29
test_pbf cmp_12_3673_1911         table_source,points1,points2/12/3673/1911
test_pbf cmp_13_7346_3822         table_source,points1,points2/13/7346/3822
test_pbf cmp_14_14692_7645        table_source,points1,points2/14/14692/7645
test_pbf cmp_17_117542_61161      table_source,points1,points2/17/117542/61161
test_pbf cmp_18_235085_122323     table_source,points1,points2/18/235085/122323

>&2 echo "***** Test server response for function source *****"
test_jsn fnc                      function_zxy_query
test_pbf fnc_0_0_0                function_zxy_query/0/0/0
test_pbf fnc_6_57_29              function_zxy_query/6/57/29
test_pbf fnc_12_3673_1911         function_zxy_query/12/3673/1911
test_pbf fnc_13_7346_3822         function_zxy_query/13/7346/3822
test_pbf fnc_14_14692_7645        function_zxy_query/14/14692/7645
test_pbf fnc_17_117542_61161      function_zxy_query/17/117542/61161
test_pbf fnc_18_235085_122323     function_zxy_query/18/235085/122323

test_jsn fnc_token                function_zxy_query_test
test_pbf fnc_token_0_0_0          function_zxy_query_test/0/0/0?token=martin

test_jsn fnc_b                    function_zxy_query_jsonb
test_pbf fnc_b_6_38_20            function_zxy_query_jsonb/6/57/29

>&2 echo "***** Test server response for different function call types *****"
test_pbf fnc_zoom_xy_6_57_29      function_zoom_xy/6/57/29
test_pbf fnc_zxy_6_57_29          function_zxy/6/57/29
test_pbf fnc_zxy2_6_57_29         function_zxy2/6/57/29
test_pbf fnc_zxy_query_6_57_29    function_zxy_query/6/57/29
test_pbf fnc_zxy_row_6_57_29      function_zxy_row/6/57/29
test_pbf fnc_zxy_row2_6_57_29     function_Mixed_Name/6/57/29
test_pbf fnc_zxy_row_key_6_57_29  function_zxy_row_key/6/57/29

>&2 echo "***** Test server response for table source with different SRID *****"
test_jsn points3857_srid          points3857
test_pbf points3857_srid_0_0_0    points3857/0/0/0

>&2 echo "***** Test server response for PMTiles source *****"
test_jsn pmt         stamen_toner__raster_CC-BY-ODbL_z3
test_png pmt_3_4_2   stamen_toner__raster_CC-BY-ODbL_z3/3/4/2
test_png webp2_1_0_0 webp2/1/0/0  # HTTP pmtiles

>&2 echo "***** Test server response for MbTiles source *****"
test_jsn mb_jpg       geography-class-jpg
test_jpg mb_jpg_0_0_0 geography-class-jpg/0/0/0
test_jsn mb_png       geography-class-png
test_png mb_png_0_0_0 geography-class-png/0/0/0
test_jsn mb_mvt       world_cities
test_pbf mb_mvt_2_3_1 world_cities/2/3/1

>&2 echo "***** Test server response for COG(Cloud Optimized GeoTiff) source *****"
test_jsn rgb_u8       rgb_u8
test_png rgb_u8_0_0_0 rgb_u8/0/0/0
test_png rgb_u8_3_0_0 rgb_u8/3/0/0
test_png rgb_u8_3_1_1 rgb_u8/3/1/1

test_jsn rgba_u8       rgba_u8
test_png rgba_u8_0_0_0 rgba_u8/0/0/0
test_png rgba_u8_3_0_0 rgba_u8/3/0/0
test_png rgba_u8_3_1_1 rgba_u8/3/1/1

test_jsn rgba_u8_nodata       rgba_u8_nodata
test_png rgba_u8_nodata_0_0_0 rgba_u8_nodata/0/0/0
test_png rgba_u8_nodata_1_0_0 rgba_u8_nodata/1/0/0

>&2 echo "***** Test server response for table source with empty SRID *****"
test_pbf points_empty_srid_0_0_0  points_empty_srid/0/0/0

>&2 echo "***** Test server response for comments *****"
test_jsn tbl_comment              MixPoints
test_jsn fnc_comment              function_Mixed_Name

kill_process "$MARTIN_PROC_ID" Martin

test_log_has_str "$LOG_FILE" 'WARN  martin::pg::query_tables] Table public.table_source has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'WARN  martin::pg::query_tables] Table public.table_source_geog has no spatial index on column geog'
test_log_has_str "$LOG_FILE" 'WARN  martin::fonts] Ignoring duplicate font Overpass Mono Regular from tests'
validate_log "$LOG_FILE"
remove_line "${TEST_OUT_DIR}/save_config.yaml" " connection_string: "


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test minimum auto configured Martin"

TEST_NAME="auto_mini"
LOG_FILE="${LOG_DIR}/${TEST_NAME}.txt"
TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
mkdir -p "$TEST_OUT_DIR"

ARG=(--save-config "${TEST_OUT_DIR}/save_config.yaml" tests/fixtures/pmtiles2)
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "$LOG_FILE" &
MARTIN_PROC_ID=$(jobs -p | tail -n 1)

{ set +x; } 2> /dev/null
trap "echo 'Stopping Martin server $MARTIN_PROC_ID...'; kill -9 $MARTIN_PROC_ID 2> /dev/null || true; echo 'Stopped Martin server $MARTIN_PROC_ID';" EXIT HUP INT TERM
wait_for "$MARTIN_PROC_ID" Martin "$MARTIN_URL/health"

>&2 echo "Test catalog"
test_jsn catalog_auto catalog

kill_process "$MARTIN_PROC_ID" Martin
validate_log "$LOG_FILE"


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test pre-configured Martin"

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
test_pbf tbl_0_0_0    table_source/0/0/0
test_pbf cmp_0_0_0    points1,points2/0/0/0
test_pbf fnc_0_0_0    function_zxy_query/0/0/0
test_pbf fnc2_0_0_0   function_zxy_query_test/0/0/0?token=martin
test_png pmt_0_0_0    pmt/0/0/0
test_png pmt2_0_0_0   pmt2/0/0/0  # HTTP pmtiles

# Test sprites
test_jsn spr_src1      sprite/src1.json
test_jsn sdf_spr_src1  sdf_sprite/src1.json
test_png spr_src1      sprite/src1.png
test_png sdf_spr_src1  sdf_sprite/src1.png
test_jsn spr_src1_2x   sprite/src1@2x.json
test_jsn sdf_spr_src1_ sdf_sprite/src1@2x.json
test_png spr_src1_2x   sprite/src1@2x.png
test_png sdf_spr_src1_ sdf_sprite/src1@2x.png
test_jsn spr_mysrc     sprite/mysrc.json
test_jsn sdf_spr_mysrc sdf_sprite/mysrc.json
test_png spr_mysrc     sprite/mysrc.png
test_png sdf_spr_mysrc sdf_sprite/mysrc.png
test_jsn spr_mysrc_2x  sprite/mysrc@2x.json
test_jsn sdf_spr_mysrc sdf_sprite/mysrc@2x.json
test_png spr_mysrc_2x  sprite/mysrc@2x.png
test_png sdf_spr_mysrc sdf_sprite/mysrc@2x.png
test_jsn spr_cmp       sprite/src1,mysrc.json
test_jsn sdf_spr_cmp   sdf_sprite/src1,mysrc.json
test_png spr_cmp       sprite/src1,mysrc.png
test_png sdf_spr_cmp   sdf_sprite/src1,mysrc.png
test_jsn spr_cmp_2x    sprite/src1,mysrc@2x.json
test_jsn sdf_spr_cmp_2 sdf_sprite/src1,mysrc@2x.json
test_png spr_cmp_2x    sprite/src1,mysrc@2x.png
test_png sdf_spr_cmp_2 sdf_sprite/src1,mysrc@2x.png

# Test styles
test_jsn style_src2_maptiler_basic    style/maptiler_basic
test_jsn style_src2_maptiler_basic.1  style/maptiler_basic.json
test_jsn style_maplibre_demo          style/maplibre
test_jsn style_maplibre_demo.1        style/maplibre.json

# Test fonts
test_font font_1      font/Overpass%20Mono%20Light/0-255
test_font font_2      font/Overpass%20Mono%20Regular/0-255
test_font font_3      font/Overpass%20Mono%20Regular,Overpass%20Mono%20Light/0-255

# Test comments override
test_jsn tbl_comment_cfg  MixPoints
test_jsn fnc_comment_cfg  fnc_Mixed_Name

kill_process "$MARTIN_PROC_ID" Martin
test_log_has_str "$LOG_FILE" 'WARN  martin::pg::query_tables] Table public.table_source has no spatial index on column geom'
test_log_has_str "$LOG_FILE" 'WARN  martin::pg::query_tables] Table public.table_source_geog has no spatial index on column geog'
test_log_has_str "$LOG_FILE" 'WARN  martin::fonts] Ignoring duplicate font Overpass Mono Regular from tests'
validate_log "$LOG_FILE"
remove_line "${TEST_OUT_DIR}/save_config.yaml" " connection_string: "


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test martin-cp"

if [[ "$MARTIN_CP_BIN" != "-" ]]; then
  TEST_NAME="martin-cp"
  TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
  mkdir -p "$TEST_OUT_DIR"

  export DATABASE_URL="$MARTIN_DATABASE_URL"
  CFG=(--default-srid 900913 --auto-bounds calc tests/fixtures/mbtiles tests/fixtures/pmtiles tests/fixtures/pmtiles2)

  test_martin_cp "flat" "${CFG[@]}" \
      --source table_source --mbtiles-type flat --concurrency 3 \
      --min-zoom 0 --max-zoom 6 "--bbox=-2,-1,142.84,45" \
      --set-meta "generator=martin-cp v0.0.0"
  test_martin_cp "flat-with-hash" "${CFG[@]}" \
      --source function_zxy_query_test --url-query 'foo=bar&token=martin' --encoding 'identity' --mbtiles-type flat-with-hash --concurrency 3 \
      --min-zoom 0 --max-zoom 6 "--bbox=-2,-1,142.84,45" \
      --set-meta "generator=martin-cp v0.0.0"
  test_martin_cp "normalized" "${CFG[@]}" \
      --source geography-class-png --mbtiles-type normalized --concurrency 3 \
      --min-zoom 0 --max-zoom 6 "--bbox=-2,-1,142.84,45" \
      --set-meta "generator=martin-cp v0.0.0" --set-meta "name=normalized" --set-meta=center=0,0,0

  unset DATABASE_URL

else
  echo "Skipping martin-cp tests"
fi


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test mbtiles utility"
if [[ "$MBTILES_BIN" != "-" ]]; then

  TEST_NAME="mbtiles"
  TEST_OUT_DIR="${TEST_OUT_BASE_DIR}/${TEST_NAME}"
  mkdir -p "$TEST_OUT_DIR"

  set -x

  $MBTILES_BIN summary ./tests/fixtures/mbtiles/world_cities.mbtiles 2>&1 | tee "$TEST_OUT_DIR/summary.txt"
  $MBTILES_BIN meta-all --help 2>&1 | tee "$TEST_OUT_DIR/meta-all_help.txt"
  $MBTILES_BIN meta-all ./tests/fixtures/mbtiles/world_cities.mbtiles 2>&1 | tee "$TEST_OUT_DIR/meta-all.txt"
  $MBTILES_BIN meta-get --help 2>&1 | tee "$TEST_OUT_DIR/meta-get_help.txt"
  $MBTILES_BIN meta-get ./tests/fixtures/mbtiles/world_cities.mbtiles name 2>&1 | tee "$TEST_OUT_DIR/meta-get_name.txt"
  $MBTILES_BIN meta-get ./tests/fixtures/mbtiles/world_cities.mbtiles missing_value 2>&1 | tee "$TEST_OUT_DIR/meta-get_missing_value.txt"
  $MBTILES_BIN validate ./tests/fixtures/mbtiles/zoomed_world_cities.mbtiles 2>&1 | tee "$TEST_OUT_DIR/validate-ok.txt"

  if $MBTILES_BIN validate ./tests/fixtures/files/invalid-tile-idx.mbtiles 2>&1 | tee "$TEST_OUT_DIR/validate-bad-tiles.txt"; then
    echo "ERROR: validate with invalid-tile-idx.mbtiles should have failed"
    exit 1
  fi
  if $MBTILES_BIN validate ./tests/fixtures/files/bad_hash.mbtiles 2>&1 | tee "$TEST_OUT_DIR/validate-bad-hash.txt"; then
    echo "ERROR: validate with bad_hash.mbtiles should have failed"
    exit 1
  fi

  cp ./tests/fixtures/files/bad_hash.mbtiles "$TEST_TEMP_DIR/fix_bad_hash.mbtiles"
  $MBTILES_BIN validate --agg-hash update "$TEST_TEMP_DIR/fix_bad_hash.mbtiles" 2>&1 | tee "$TEST_OUT_DIR/validate-fix.txt"
  $MBTILES_BIN validate "$TEST_TEMP_DIR/fix_bad_hash.mbtiles" 2>&1 | tee "$TEST_OUT_DIR/validate-fix2.txt"

  # Create diff file
  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities.mbtiles \
    "$TEST_TEMP_DIR/world_cities_diff.mbtiles" \
    --diff-with-file ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
    2>&1 | tee "$TEST_OUT_DIR/copy_diff.txt"
  $MBTILES_BIN diff \
       ./tests/fixtures/mbtiles/world_cities.mbtiles \
       ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
       "$TEST_TEMP_DIR/world_cities_diff2.mbtiles" \
       2>&1 | tee "$TEST_OUT_DIR/copy_diff2.txt"

  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities.mbtiles \
    --diff-with-file ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
    "$TEST_TEMP_DIR/world_cities_bindiff.mbtiles" \
    --patch-type bin-diff-gz \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff.txt"
  test_log_has_str "$TEST_OUT_DIR/copy_bindiff.txt" '.*Processing bindiff patches using .* threads...'

  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities.mbtiles \
    --apply-patch "$TEST_TEMP_DIR/world_cities_bindiff.mbtiles" \
    "$TEST_TEMP_DIR/world_cities_modified2.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff2.txt"
  test_log_has_str "$TEST_OUT_DIR/copy_bindiff2.txt" '.*Processing bindiff patches using .* threads...'

  # Ensure that world_cities_modified and world_cities_modified2 are identical (regular diff is empty)
  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
    --diff-with-file "$TEST_TEMP_DIR/world_cities_modified2.mbtiles" \
    "$TEST_TEMP_DIR/world_cities_bindiff_modified.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff3.txt"
  $MBTILES_BIN summary "$TEST_TEMP_DIR/world_cities_bindiff_modified.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff4.txt"

  # See if the stored bindiff file can also be applied to produce the same result
  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities.mbtiles \
    --apply-patch ./tests/fixtures/mbtiles/world_cities_bindiff.mbtiles \
    "$TEST_TEMP_DIR/world_cities_modified3.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff5.txt"
  test_log_has_str "$TEST_OUT_DIR/copy_bindiff5.txt" '.*Processing bindiff patches using .* threads...'

  # Ensure that world_cities_modified and world_cities_modified3 are identical (regular diff is empty)
  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
    --diff-with-file "$TEST_TEMP_DIR/world_cities_modified3.mbtiles" \
    "$TEST_TEMP_DIR/world_cities_bindiff_modified2.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff6.txt"
  $MBTILES_BIN summary "$TEST_TEMP_DIR/world_cities_bindiff_modified2.mbtiles" \
    2>&1 | tee "$TEST_OUT_DIR/copy_bindiff7.txt"

  if command -v sqlite3 > /dev/null; then

    compare_sql_dbs "$TEST_TEMP_DIR/world_cities_bindiff.mbtiles" \
      ./tests/fixtures/mbtiles/world_cities_bindiff.mbtiles \
      "$TEST_OUT_DIR/copy_bindiff_diff.txt"

    # Apply this diff to the original version of the file
    cp ./tests/fixtures/mbtiles/world_cities.mbtiles "$TEST_TEMP_DIR/world_cities_copy.mbtiles"

    sqlite3 "$TEST_TEMP_DIR/world_cities_copy.mbtiles" \
      -bail \
      -cmd ".parameter set @diffDbFilename $TEST_TEMP_DIR/world_cities_diff.mbtiles" \
      "ATTACH DATABASE @diffDbFilename AS diffDb;" \
      "DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);" \
      "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"

    # Ensure that applying the diff resulted in the modified version of the file
    $MBTILES_BIN copy \
      --diff-with-file "$TEST_TEMP_DIR/world_cities_copy.mbtiles" \
      ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
      "$TEST_TEMP_DIR/world_cities_diff_modified.mbtiles" \
      2>&1 | tee "$TEST_OUT_DIR/copy_diff2.txt"

    sqlite3 "$TEST_TEMP_DIR/world_cities_diff_modified.mbtiles" \
      "SELECT COUNT(*) FROM tiles;" \
      2>&1 | tee "$TEST_OUT_DIR/copy_apply.txt"

  else
    echo "---------------------------------------------------------"
    echo "##### sqlite3 is not installed, skipping apply test #####"
    # Copy expected output files as if they were generated by the test
    EXPECTED_DIR="$(dirname "$0")/expected/mbtiles"
    cp "$EXPECTED_DIR/copy_bindiff_diff.txt" "$TEST_OUT_DIR/copy_bindiff_diff.txt"
    cp "$EXPECTED_DIR/copy_diff2.txt" "$TEST_OUT_DIR/copy_diff2.txt"
    cp "$EXPECTED_DIR/copy_apply.txt" "$TEST_OUT_DIR/copy_apply.txt"
  fi

  { set +x; } 2> /dev/null
else
  echo "Skipping mbtiles utility tests"
fi

rm -rf "$TEST_TEMP_DIR"

>&2 echo "All integration tests have passed"

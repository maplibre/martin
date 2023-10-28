#!/usr/bin/env bash
set -euo pipefail

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl --silent --show-error --fail --compressed}
DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BUILD="${MARTIN_BUILD:-cargo build}"
MARTIN_PORT="${MARTIN_PORT:-3111}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_ARGS="${MARTIN_ARGS:---listen-addresses localhost:${MARTIN_PORT}}"
MARTIN_BIN="${MARTIN_BIN:-cargo run --} ${MARTIN_ARGS}"

MBTILES_BUILD="${MBTILES_BUILD:-cargo build -p martin-mbtiles}"
MBTILES_BIN="${MBTILES_BIN:-target/debug/mbtiles}"

TMP_DIR="${TMP_DIR:-target/tmp}"
mkdir -p "$TMP_DIR"

function wait_for_martin {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS "$MARTIN_URL/health"
    PROCESS_ID=$1
    echo "Waiting for Martin ($PROCESS_ID) to start by checking $MARTIN_URL/health to be valid..."
    for i in {1..60}; do
        if $CURL "$MARTIN_URL/health" 2>/dev/null >/dev/null; then
            echo "Martin is up!"
            $CURL "$MARTIN_URL/health"
            return
        fi
        if ps -p $PROCESS_ID > /dev/null ; then
            echo "Martin is not up yet, waiting for $MARTIN_URL/health ..."
            sleep 1
        else
            echo "Martin died!"
            ps au
            lsof -i || true
            exit 1
        fi
    done
    echo "Martin did not start in time"
    ps au
    lsof -i || true
    exit 1
}

function kill_process {
    PROCESS_ID=$1
    echo "Waiting for Martin ($PROCESS_ID) to stop..."
    kill $PROCESS_ID
    for i in {1..50}; do
        if ps -p $PROCESS_ID > /dev/null ; then
            sleep 0.1
        else
            echo "Martin ($PROCESS_ID) has stopped"
            return
        fi
    done
    echo "Martin did not stop in time, killing it"
    kill -9 $PROCESS_ID
    # wait for it to die using timeout and wait
    timeout -k 1s 1s wait $PROCESS_ID || true
}

test_jsn()
{
  FILENAME="$TEST_OUT_DIR/$1.json"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" | jq -e > "$FILENAME"
}

test_pbf()
{
  FILENAME="$TEST_OUT_DIR/$1.pbf"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"

  if [[ $OSTYPE == linux* ]]; then
    ./tests/fixtures/vtzero-check "$FILENAME"
    ./tests/fixtures/vtzero-show "$FILENAME" > "$FILENAME.txt"
  fi
}

test_png()
{
  FILENAME="$TEST_OUT_DIR/$1.png"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"

  if [[ $OSTYPE == linux* ]]; then
    file "$FILENAME" > "$FILENAME.txt"
  fi
}

test_font(){
  FILENAME="$TEST_OUT_DIR/$1.pbf"
  URL="$MARTIN_URL/$2"

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME" 
}

# Delete a line from a file $1 that matches parameter $2
remove_line()
{
  FILE="$1"
  LINE_TO_REMOVE="$2"
  >&2 echo "Removing line '$LINE_TO_REMOVE' from $FILE"
  grep -v "$LINE_TO_REMOVE" "${FILE}" > "${FILE}.tmp"
  mv "${FILE}.tmp" "${FILE}"
}

test_log_has_str()
{
  LOG_FILE="$1"
  EXPECTED_TEXT="$2"
  echo "Checking $LOG_FILE for expected text: '$EXPECTED_TEXT'"
  grep -q "$EXPECTED_TEXT" "$LOG_FILE"
  remove_line "$LOG_FILE" "$EXPECTED_TEXT"
}

validate_log()
{
  LOG_FILE="$1"
  >&2 echo "Validating log file $LOG_FILE"

  # Older versions of PostGIS don't support the margin parameter, so we need to remove it from the log
  remove_line "$LOG_FILE" 'Margin parameter in ST_TileEnvelope is not supported'
  remove_line "$LOG_FILE" 'Source IDs must be unique'

  # Make sure the log has just the expected warnings, remove them, and test that there are no other ones
  test_log_has_str "$LOG_FILE" 'WARN  martin::pg::table_source] Table public.table_source has no spatial index on column geom'
  test_log_has_str "$LOG_FILE" 'WARN  martin::fonts] Ignoring duplicate font Overpass Mono Regular from tests/fixtures/fonts/overpass-mono-regular.ttf because it was already configured from tests/fixtures/fonts/overpass-mono-regular.ttf'

  echo "Checking for no other warnings or errors in the log"
  if grep -e ' ERROR ' -e ' WARN ' "$LOG_FILE"; then
    echo "Log file $LOG_FILE has unexpected warnings or errors"
    exit 1
  fi
}

curl --version

# Make sure martin and mbtiles are built - this way it won't timeout while waiting for it to start
# If set to "-", don't build
if [[ "$MARTIN_BUILD" != "-" ]]; then
  $MARTIN_BUILD
fi
if [[ "$MBTILES_BUILD" != "-" ]]; then
  $MBTILES_BUILD
fi


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test auto configured Martin"

TEST_OUT_DIR="$(dirname "$0")/output/auto"
mkdir -p "$TEST_OUT_DIR"

ARG=(--default-srid 900913 --auto-bounds calc --save-config "$(dirname "$0")/output/generated_config.yaml" tests/fixtures/mbtiles tests/fixtures/pmtiles --font tests/fixtures/fonts/overpass-mono-regular.ttf --font tests/fixtures/fonts)
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "${TMP_DIR}/test_log_1.txt" &
PROCESS_ID=`jobs -p`

{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

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
test_jsn pmt       stamen_toner__raster_CC-BY-ODbL_z3
test_png pmt_3_4_2 stamen_toner__raster_CC-BY-ODbL_z3/3/4/2

>&2 echo "***** Test server response for MbTiles source *****"
test_jsn mb_jpg       geography-class-jpg
test_png mb_jpg_0_0_0 geography-class-jpg/0/0/0
test_jsn mb_png       geography-class-png
test_png mb_png_0_0_0 geography-class-png/0/0/0
test_jsn mb_mvt       world_cities
test_pbf mb_mvt_2_3_1 world_cities/2/3/1

>&2 echo "***** Test server response for table source with empty SRID *****"
test_pbf points_empty_srid_0_0_0  points_empty_srid/0/0/0

kill_process $PROCESS_ID
validate_log "${TMP_DIR}/test_log_1.txt"


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test pre-configured Martin"
TEST_OUT_DIR="$(dirname "$0")/output/configured"
mkdir -p "$TEST_OUT_DIR"

ARG=(--config tests/config.yaml --max-feature-count 1000 --save-config "$(dirname "$0")/output/given_config.yaml" -W 1)
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee "${TMP_DIR}/test_log_2.txt" &
PROCESS_ID=`jobs -p`
{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

>&2 echo "Test catalog"
test_jsn catalog_cfg catalog

test_pbf tbl_0_0_0   table_source/0/0/0
test_pbf cmp_0_0_0   points1,points2/0/0/0
test_pbf fnc_0_0_0   function_zxy_query/0/0/0
test_pbf fnc2_0_0_0  function_zxy_query_test/0/0/0?token=martin
test_png pmt_0_0_0   pmt/0/0/0

test_jsn spr_src1     sprite/src1.json
test_png spr_src1     sprite/src1.png
test_jsn spr_src1_2x  sprite/src1@2x.json
test_png spr_src1_2x  sprite/src1@2x.png
test_jsn spr_mysrc    sprite/mysrc.json
test_png spr_mysrc    sprite/mysrc.png
test_jsn spr_mysrc_2x sprite/mysrc@2x.json
test_png spr_mysrc_2x sprite/mysrc@2x.png
test_jsn spr_cmp      sprite/src1,mysrc.json
test_png spr_cmp      sprite/src1,mysrc.png
test_jsn spr_cmp_2x   sprite/src1,mysrc@2x.json
test_png spr_cmp_2x   sprite/src1,mysrc@2x.png

test_font font_1      font/Overpass%20Mono%20Light/0-255
test_font font_2      font/Overpass%20Mono%20Regular/0-255
test_font font_3      font/Overpass%20Mono%20Regular%2COverpass%20Mono%20Light/0-255

kill_process $PROCESS_ID
validate_log "${TMP_DIR}/test_log_2.txt"

remove_line "$(dirname "$0")/output/given_config.yaml"       " connection_string: "
remove_line "$(dirname "$0")/output/generated_config.yaml"   " connection_string: "


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test mbtiles utility"
if [[ "$MBTILES_BIN" != "-" ]]; then
  TEST_OUT_DIR="$(dirname "$0")/output/mbtiles"
  mkdir -p "$TEST_OUT_DIR"
  TEST_TEMP_DIR="$(dirname "$0")/temp"
  if [[ -d "$TEST_TEMP_DIR" ]]; then
    echo "ERROR: ./$TEST_TEMP_DIR already exists. Please remove it first."
    exit 1
  else
    mkdir -p "$TEST_TEMP_DIR"
  fi

  set -x

  $MBTILES_BIN --help 2>&1 | tee "$TEST_OUT_DIR/help.txt"
  $MBTILES_BIN meta-all --help 2>&1 | tee "$TEST_OUT_DIR/meta-all_help.txt"
  $MBTILES_BIN meta-all ./tests/fixtures/mbtiles/world_cities.mbtiles 2>&1 | tee "$TEST_OUT_DIR/meta-all.txt"
  $MBTILES_BIN meta-get --help 2>&1 | tee "$TEST_OUT_DIR/meta-get_help.txt"
  $MBTILES_BIN meta-get ./tests/fixtures/mbtiles/world_cities.mbtiles name 2>&1 | tee "$TEST_OUT_DIR/meta-get_name.txt"
  $MBTILES_BIN meta-get ./tests/fixtures/mbtiles/world_cities.mbtiles missing_value 2>&1 | tee "$TEST_OUT_DIR/meta-get_missing_value.txt"
  $MBTILES_BIN validate ./tests/fixtures/mbtiles/zoomed_world_cities.mbtiles 2>&1 | tee "$TEST_OUT_DIR/validate-ok.txt"

  set +e
  $MBTILES_BIN validate ./tests/fixtures/files/bad_hash.mbtiles 2>&1 | tee "$TEST_OUT_DIR/validate-bad.txt"
  if [[ $? -eq 0 ]]; then
    echo "ERROR: validate with bad_hash should have failed"
    exit 1
  fi
  set -e

  cp ./tests/fixtures/files/bad_hash.mbtiles "$TEST_TEMP_DIR/fix_bad_hash.mbtiles"
  $MBTILES_BIN validate --update-agg-tiles-hash "$TEST_TEMP_DIR/fix_bad_hash.mbtiles" 2>&1 | tee "$TEST_OUT_DIR/validate-fix.txt"
  $MBTILES_BIN validate "$TEST_TEMP_DIR/fix_bad_hash.mbtiles" 2>&1 | tee "$TEST_OUT_DIR/validate-fix2.txt"

  # Create diff file
  $MBTILES_BIN copy \
    ./tests/fixtures/mbtiles/world_cities.mbtiles \
    "$TEST_TEMP_DIR/world_cities_diff.mbtiles" \
    --diff-with-file ./tests/fixtures/mbtiles/world_cities_modified.mbtiles \
    2>&1 | tee "$TEST_OUT_DIR/copy_diff.txt"

  if command -v sqlite3 > /dev/null; then
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
    cp "$EXPECTED_DIR/copy_diff2.txt" "$TEST_OUT_DIR/copy_diff2.txt"
    cp "$EXPECTED_DIR/copy_apply.txt" "$TEST_OUT_DIR/copy_apply.txt"
  fi

  rm -rf "$TEST_TEMP_DIR"

  { set +x; } 2> /dev/null
else
  echo "Skipping mbtiles utility tests"
fi

>&2 echo "All integration tests have passed"

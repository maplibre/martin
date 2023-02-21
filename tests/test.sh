#!/usr/bin/env bash
set -euo pipefail

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl --silent --show-error --fail --compressed}
DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BUILD="${MARTIN_BUILD:-cargo build --all-features}"
MARTIN_PORT="${MARTIN_PORT:-3111}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_ARGS="${MARTIN_ARGS:---listen-addresses localhost:${MARTIN_PORT}}"
MARTIN_BIN="${MARTIN_BIN:-cargo run --all-features --} ${MARTIN_ARGS}"

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

  # Make sure the log has just the expected warnings, remove them, and test that there are no other ones
  test_log_has_str "$LOG_FILE" 'WARN  martin::pg::table_source] Table public.table_source has no spatial index on column geom'

  echo "Checking for no other warnings or errors in the log"
  if grep -e ' ERROR ' -e ' WARN ' "$LOG_FILE"; then
    echo "Log file $LOG_FILE has unexpected warnings or errors"
    exit 1
  fi
}

curl --version

# Make sure martin is built - this way it won't timeout while waiting for it to start
# If MARTIN_BUILD is set to "-", don't build
if [[ "$MARTIN_BUILD" != "-" ]]; then
  $MARTIN_BUILD
fi


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test auto configured Martin"

TEST_OUT_DIR="$(dirname "$0")/output/auto"
mkdir -p "$TEST_OUT_DIR"

ARG=(--default-srid 900913 --disable-bounds --save-config "$(dirname "$0")/output/generated_config.yaml" tests/fixtures/files)
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee test_log_1.txt &
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
validate_log test_log_1.txt


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test pre-configured Martin"
TEST_OUT_DIR="$(dirname "$0")/output/configured"
mkdir -p "$TEST_OUT_DIR"

ARG=(--config tests/config.yaml --max-feature-count 1000 --save-config "$(dirname "$0")/output/given_config.yaml" -W 1)
set -x
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee test_log_2.txt &
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

kill_process $PROCESS_ID
validate_log test_log_2.txt

remove_line "$(dirname "$0")/output/given_config.yaml"       " connection_string: "
remove_line "$(dirname "$0")/output/generated_config.yaml"   " connection_string: "

>&2 echo "All integration tests have passed"
